use core::num;
use std::any::Any;
use anyhow::Result;
use tokio::sync::watch;
use std::sync::{Arc, Mutex};
use std::collections::{BinaryHeap, HashMap};
use std::cmp::{Ordering, Reverse};
use std::thread;
use std::time::{Duration, Instant};
use futures::future::BoxFuture;
use tokio::sync::mpsc::{channel, Sender, Receiver, error};
use uuid::Uuid;

pub struct Message {
    data: Box<dyn Any + Send + Sync>,
    timestamp: f64,
    from: usize,
    to: usize,

}

impl PartialEq for Message {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl Eq for Message {}

impl PartialOrd for Message {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Message {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.partial_cmp(&other.timestamp).unwrap()
    }
}

pub struct Mailbox {
    tx: Sender<Message>,
    rx: Receiver<Message>,
    mailbox: Vec<Message>,
    pause_rx: watch::Receiver<bool>,
}

impl Mailbox {
    pub fn new(buffer_size: usize, pause_rx: watch::Receiver<bool>) -> Self {
        let (tx, rx) = channel(buffer_size);
        Mailbox {
            tx,
            rx,
            mailbox: Vec::new(),
            pause_rx,
        }
    }

    pub fn is_paused(&self) -> bool {
        *self.pause_rx.borrow()
    }

    pub async fn wait_for_resume(&mut self) {
        while self.is_paused() {
            self.pause_rx.changed().await.unwrap();
        }
    }

    pub async fn send(&self, msg: Message) -> Result<(), error::SendError<Message>> {
        self.tx.send(msg).await
    }

    pub async fn receive(&mut self) -> Option<Message> {
        self.rx.recv().await
    }

    pub fn peek_messages(&self) -> &[Message] {
        &self.mailbox
    }

    pub async fn collect_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.mailbox.push(msg);
        }
    }
}

pub trait Agent: Send {
    fn step<'a>(&'a mut self, state: &'a mut Option<State>, time: f64, mailbox: &'a mut Mailbox) -> BoxFuture<'a, Event>;
}

// Shared state between agents
pub type State = Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>;

#[derive(Clone, Debug)]
pub enum Action {
    Timeout(f64),
    Schedule(f64),
    Trigger { time: f64, idx: usize },
    Wait,
}

#[derive(Clone, Debug)]
pub struct Event {
    time: f64,
    agent: usize,
    yield_: Action,
}

impl Event {
    pub fn new(time: f64, agent: usize, yield_: Action) -> Self {
        Self {
            time,
            agent,
            yield_,
        }
    }
}


impl PartialEq for Event {
    fn eq(&self, other: &Self) -> bool {
        self.time == other.time
    }
}
impl Eq for Event {}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Event {
    fn cmp(&self, other: &Self) -> Ordering {
        self.time.partial_cmp(&other.time).unwrap()
    }
}

struct Time {
    time: f64,
    step: usize,
    timestep: f64,
    timescale: f64, // 1.0 = real-time, 0.5 = half-time, 2.0 = double-time
    terminal: Option<f64>,
}

pub enum SimError {
    TimeTravel,
    PastTerminal,
    NoState,
}

pub struct World {
    pending: BinaryHeap<Reverse<Event>>,
    logged: BinaryHeap<Event>,
    savedmail: BinaryHeap<Message>,
    agents: Vec<Box<dyn Agent>>,
    mailbox: Mailbox,
    time: Time,
    state: Option<State>,
    sender: Sender<Event>,
    runtime: Receiver<Event>,
    pause_tx: watch::Sender<bool>,
    pause_rx: watch::Receiver<bool>,
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

impl World {
    pub fn create(timestep: f64, terminal: Option<f64>, buffer_size: usize, mailbox_size: usize) -> Self {
        let (sim_tx, sim_rx) = channel(buffer_size);
        let time = Time {
            time: 0.0,
            step: 0,
            timestep,
            timescale: 1.0,
            terminal,
        };
        let (pause_tx, pause_rx) = watch::channel(false);
        let mailbox = Mailbox::new(mailbox_size, pause_rx.clone());
        World {
            pending: BinaryHeap::new(),
            logged: BinaryHeap::new(),
            savedmail: BinaryHeap::new(),
            agents: Vec::new(),
            mailbox,
            time,
            state: None,
            sender: sim_tx,
            runtime: sim_rx,
            pause_tx,
            pause_rx,
        }
    }

    pub fn spawn(&mut self, agent: Box<dyn Agent>) -> usize {
        self.agents.push(agent);
        self.agents.len() - 1
    }

    pub fn pause(&self) {
        self.pause_tx.send(true).unwrap();
    }

    pub fn resume(&self) {
        self.pause_tx.send(false).unwrap();
    }

    pub async fn run(&mut self, live: bool) -> Result<(), SimError> {
        loop {
            if *self.pause_rx.borrow() {
                self.pause_rx.changed().await.unwrap();
                continue;
            }
            while let Ok(event) = self.runtime.try_recv() {
                self.pending.push(Reverse(event));
            }
            if let Some(event) = self.pending.pop() {
                if live {
                    thread::sleep(Duration::from_millis((self.time.timestep * 1000.0 / self.time.timescale) as u64));
                }
                let event = event.0;
                if event.time > self.time.terminal.unwrap_or(f64::INFINITY) {
                    return Err(SimError::PastTerminal);
                }
                if event.time < self.time.time {
                    return Err(SimError::TimeTravel);
                }
                self.mailbox.collect_messages().await;
                self.time.time = event.time;
                self.time.step += 1;
                let agent = &mut self.agents[event.agent];
                let event = agent.step(&mut self.state, event.time.clone(), &mut self.mailbox).await;
                match event.yield_ {
                    Action::Timeout(time) => {
                        self.pending.push(Reverse(Event::new(self.time.time + time, event.agent, Action::Wait)));
                    }
                    Action::Schedule(time) => {
                        self.pending.push(Reverse(Event::new(time, event.agent, Action::Wait)));
                    }
                    Action::Trigger { time, idx  } => {
                        self.pending.push(Reverse(Event::new(time, idx, Action::Wait)));
                    }
                    Action::Wait => {
                
                    }
                }
                self.logged.push(event);
                continue;
            }
            if let Some(event) = self.runtime.recv().await {
                self.pending.push(Reverse(event));
                continue;
            }
            break;
        }
        Ok(())
    }

    pub fn now(&self) -> f64 {
        self.time.time
    }

    pub fn step_counter(&self) -> usize {
        self.time.step
    }

    async fn schedule(sender: Sender<Event>, time: f64, agent: usize) -> Result<(), error::SendError<Event>> {
        sender.send(Event::new(time, agent, Action::Wait)).await
    }

    pub fn block_agent(&mut self, idx: usize, until: Option<f64>) {
        if until.is_none() {
            self.pending.retain(|x| x.0.agent != idx);
        }
        self.pending.retain(|x| x.0.agent != idx && x.0.time < until.unwrap());
    }

    pub fn remove_event(&mut self, idx: usize, time: f64) {
        self.pending.retain(|x| x.0.agent != idx && x.0.time != time);
    }

    pub fn log_mail(&mut self, msg: Message) {
        self.savedmail.push(msg);
    }
}
use anyhow::Result;
use futures::future::BoxFuture;
use std::any::Any;
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::{channel, error, Receiver, Sender};
use tokio::sync::watch;

use crate::logger::Logger;

pub struct Message {
    pub data: Box<dyn Any + Send + Sync>,
    pub timestamp: f64,
    pub from: usize,
    pub to: usize,
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
    fn step<'a>(
        &'a mut self,
        state: &'a mut Option<State>,
        time: &f64,
        mailbox: &'a mut Mailbox,
    ) -> BoxFuture<'a, Event>;
    fn as_any(&self) -> &dyn Any;
}

pub trait Loggable: Agent {
    fn get_state(&self) -> State;
}

// Thread-safe loggable generic state type
pub type State = Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>;

#[derive(Clone, Debug)]
pub enum Action {
    Timeout(f64),
    Schedule(f64),
    Trigger { time: f64, idx: usize },
    Wait,
    Break,
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

#[derive(Debug, Clone)]
pub enum SimError {
    TimeTravel,
    PastTerminal,
    NoState,
    ScheduleFailed,
    PauseError,
    InvalidIndex,
    TokioError(String),
}

pub enum ControlCommand {
    Pause,
    Resume,
    SetTimeScale(f64),
    Quit,
}

pub struct World {
    pending: BTreeSet<Reverse<Event>>,
    savedmail: BTreeSet<Message>,
    agents: Vec<Box<dyn Agent>>,
    mailbox: Mailbox,
    time: Time,
    state: Option<State>,
    pub sender: Sender<Event>,
    runtime: Receiver<Event>,
    pub pause_tx: watch::Sender<bool>,
    pub pause_rx: watch::Receiver<bool>,
    pub logger: Logger,
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

impl World {
    pub fn create(
        timestep: f64,
        terminal: Option<f64>,
        buffer_size: usize,
        mailbox_size: usize,
    ) -> Self {
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
            pending: BTreeSet::new(),
            savedmail: BTreeSet::new(),
            agents: Vec::new(),
            mailbox,
            time,
            state: None,
            sender: sim_tx,
            runtime: sim_rx,
            pause_tx,
            pause_rx,
            logger: Logger::new(),
        }
    }

    pub fn spawn(&mut self, agent: Box<dyn Agent>) -> usize {
        self.agents.push(agent);
        self.agents.len() - 1
    }

    pub fn pause(&self) -> Result<(), SimError> {
        let pause = self.pause_tx.send(true);
        if pause.is_err() {
            return Err(SimError::PauseError);
        }
        Ok(())
    }

    pub fn resume(&self) -> Result<(), SimError> {
        let resume = self.pause_tx.send(false);
        if resume.is_err() {
            return Err(SimError::PauseError);
        }
        Ok(())
    }

    pub async fn run(&mut self, live: bool, logs: bool) -> Result<(), SimError> {
        // Command line interface for real-time simulation
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(100);
        if live {
            let cmd_tx_clone = cmd_tx.clone();
            tokio::spawn(async move {
                let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
                let mut line = String::new();
                loop {
                    line.clear();
                    if reader.read_line(&mut line).await.is_ok() {
                        let cmd = match line.trim() {
                            "pause" => ControlCommand::Pause,
                            "resume" => ControlCommand::Resume,
                            "quit" => ControlCommand::Quit,
                            cmd if cmd.starts_with("speed ") => {
                                if let Some(scale) = cmd
                                    .split_whitespace()
                                    .nth(1)
                                    .and_then(|s| Some(s.parse::<f64>().unwrap()))
                                {
                                    ControlCommand::SetTimeScale(scale)
                                } else {
                                    continue;
                                }
                            }
                            _ => continue,
                        };
                        if cmd_tx_clone.send(cmd).await.is_err() {
                            break;
                        }
                    }
                }
            });
        }

        loop {
            if live {
                if let Some(cmd) = cmd_rx.recv().await {
                    match cmd {
                        ControlCommand::Pause => self.pause()?,
                        ControlCommand::Resume => self.resume()?,
                        ControlCommand::SetTimeScale(scale) => self.rescale_time(scale),
                        ControlCommand::Quit => break,
                    }
                }
                if *self.pause_rx.borrow() {
                    self.pause_rx.changed().await.unwrap();
                    continue;
                }
            }
            while let Ok(event) = self.runtime.try_recv() {
                self.pending.insert(Reverse(event));
            }
            if let Some(event) = self.pending.pop_first() {
                if live {
                    tokio::time::sleep(Duration::from_millis(
                        (self.time.timestep * 1000.0 / self.time.timescale) as u64,
                    ))
                    .await;
                }
                let event = event.0;
                if event.time > self.time.terminal.unwrap_or(f64::INFINITY) {
                    break;
                }
                if event.time < self.time.time {
                    return Err(SimError::TimeTravel);
                }
                self.mailbox.collect_messages().await;
                self.time.time = event.time;
                self.time.step += 1;
                let agent = &mut self.agents[event.agent];
                let event = agent
                    .step(&mut self.state, &event.time, &mut self.mailbox)
                    .await;
                if logs {
                    let agent_states: BTreeMap<usize, Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>> =
                        self.agents
                            .iter()
                            .enumerate()
                            .filter_map(|(i, agt)| {
                                agt.as_any()
                                    .downcast_ref::<Box<dyn Loggable>>()
                                    .map(|loggable| (i, loggable.get_state()))
                            })
                            .collect();
                    self.logger
                        .log(self.now(), self.state.clone(), agent_states, event.clone());
                }
                match event.yield_ {
                    Action::Timeout(time) => {
                        self.pending.insert(Reverse(Event::new(
                            self.time.time + time,
                            event.agent,
                            Action::Wait,
                        )));
                    }
                    Action::Schedule(time) => {
                        self.pending
                            .insert(Reverse(Event::new(time, event.agent, Action::Wait)));
                    }
                    Action::Trigger { time, idx } => {
                        self.pending
                            .insert(Reverse(Event::new(time, idx, Action::Wait)));
                    }
                    Action::Wait => {}
                    Action::Break => {
                        break;
                    }
                }
                if live {
                    if let Some(cmd) = cmd_rx.recv().await {
                        match cmd {
                            ControlCommand::Pause => self.pause()?,
                            ControlCommand::Resume => self.resume()?,
                            ControlCommand::SetTimeScale(scale) => self.rescale_time(scale),
                            ControlCommand::Quit => break,
                        }
                    }
                }
            } else {
                if let Some(event) = self.runtime.recv().await {
                    self.pending.insert(Reverse(event));
                } else {
                    break;
                }
            }
        }
        Ok(())
    }

    pub fn now(&self) -> f64 {
        self.time.time
    }

    pub fn step_counter(&self) -> usize {
        self.time.step
    }

    pub fn block_agent(&mut self, idx: usize, until: Option<f64>) -> Result<(), SimError> {
        if self.agents.len() <= idx {
            return Err(SimError::InvalidIndex);
        }
        if until.is_none() {
            self.pending.retain(|x| x.0.agent != idx);
        }
        self.pending
            .retain(|x| x.0.agent != idx && x.0.time < until.unwrap());
        Ok(())
    }

    pub fn remove_event(&mut self, idx: usize, time: f64) -> Result<(), SimError> {
        if self.agents.len() <= idx {
            return Err(SimError::InvalidIndex);
        } else if self.time.time > time {
            return Err(SimError::TimeTravel);
        }
        self.pending
            .retain(|x| x.0.agent != idx && x.0.time != time);
        Ok(())
    }

    pub fn log_mail(&mut self, msg: Message) {
        self.savedmail.insert(msg);
    }

    pub fn rescale_time(&mut self, timescale: f64) {
        self.time.timescale = timescale;
    }

    pub fn check_status(&self) -> bool {
        self.pause_rx.borrow().clone()
    }

    pub async fn schedule(
        &self,
        sender: Sender<Event>,
        time: f64,
        agent: usize,
    ) -> Result<(), SimError> {
        if time < self.time.time {
            return Err(SimError::TimeTravel);
        } else if time > self.time.terminal.unwrap_or(f64::INFINITY) {
            return Err(SimError::PastTerminal);
        }
        let result = sender.send(Event::new(time, agent, Action::Wait)).await;
        if result.is_err() {
            return Err(SimError::ScheduleFailed);
        }
        Ok(())
    }
}

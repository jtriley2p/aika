use anyhow::Result;
use futures::future::BoxFuture;
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use std::any::Any;
use std::cmp::{Ordering, Reverse};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::thread::{self, ScopedJoinHandle};
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc::{channel, error, Receiver, Sender};
use tokio::sync::watch;

use crate::logger::Logger;

/// A message that can be sent between agents.
pub struct Message<T: Send + Sync + Clone> {
    pub data: T,
    pub timestamp: f64,
    pub from: usize,
    pub to: usize,
}

impl<T: Send + Sync + Clone> Message<T> {
    pub fn new(data: T, timestamp: f64, from: usize, to: usize) -> Self {
        Message {
            data,
            timestamp,
            from,
            to,
        }
    }
}

impl<T: Send + Sync + Clone> PartialEq for Message<T> {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl<T: Send + Sync + Clone> Eq for Message<T> {}

impl<T: Send + Sync + Clone> PartialOrd for Message<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Send + Sync + Clone> Ord for Message<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.partial_cmp(&other.timestamp).unwrap()
    }
}

/// A mailbox for agents to send and receive messages. WIP
pub struct Mailbox<T: Send + Sync + Clone> {
    tx: Sender<Message<T>>,
    rx: Receiver<Message<T>>,
    mailbox: Vec<Message<T>>,
    pause_rx: watch::Receiver<bool>,
}

impl<T: Send + Sync + Clone> Mailbox<T> {
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

    pub async fn send(&self, msg: Message<T>) -> Result<(), error::SendError<Message<T>>> {
        self.tx.send(msg).await
    }

    pub async fn receive(&mut self) -> Option<Message<T>> {
        self.rx.recv().await
    }

    pub fn peek_messages(&self) -> &[Message<T>] {
        &self.mailbox
    }

    pub async fn collect_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.mailbox.push(msg);
        }
    }
}

/// An agent that can be run in a simulation.
pub trait Agent<T: Send + Sync + Clone>: Send {
    fn step<'a>(
        &'a mut self,
        state: &'a mut Option<State>,
        time: &f64,
        mailbox: &'a mut Mailbox<T>,
    ) -> BoxFuture<'a, Event>;
    fn as_any(&self) -> &dyn Any;
}

pub trait Loggable<T: Send + Sync + Clone>: Agent<T> {
    fn get_state(&self) -> State;
}

/// Thread-safe loggable generic state type
pub type State = Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>;

/// A scheduling action that an agent can take.
#[derive(Clone, Debug)]
pub enum Action {
    Timeout(f64),
    Schedule(f64),
    Trigger { time: f64, idx: usize },
    Wait,
    Break,
}

/// An event that can be scheduled in a simulation. This is used to trigger an agent, or schedule another event.
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

/// The relevant time information for the simulation.
struct Time {
    time: f64,
    step: usize,
    timestep: f64,
    timescale: f64, // 1.0 = real-time, 0.5 = half-time, 2.0 = double-time
    terminal: Option<f64>,
}

/// Error enum for provide feedback on simulation errors
#[derive(Debug, Clone)]
pub enum SimError {
    TimeTravel,
    PastTerminal,
    ScheduleFailed,
    PlaybackFroze,
    NoState,
    NoEvents,
    NoClock,
    InvalidIndex,
    TokioError(String),
}

/// Control commands for the real-time simulation
pub enum ControlCommand {
    Pause,
    Resume,
    SetTimeScale(f64),
    Quit,
    Schedule(f64, usize),
}

/// A world that can contain multiple agents and run a simulation.
pub struct World<T: Send + Sync + Clone> {
    overflow: BTreeSet<Reverse<Event>>,
    clock: Clock,
    savedmail: BTreeSet<Message<T>>,
    agents: Vec<Box<dyn Agent<T>>>,
    mailbox: Mailbox<T>,
    state: Option<State>,
    runtype: (bool, bool, bool, bool),
    runtime: Receiver<Event>,
    pub sender: Sender<Event>,
    pub pause_tx: watch::Sender<bool>,
    pub pause_rx: watch::Receiver<bool>,
    pub logger: Logger,
}

unsafe impl<T: Send + Sync + Clone> Send for World<T> {}
unsafe impl<T: Send + Sync + Clone> Sync for World<T> {}

/// Configuration for the world
#[derive(Clone)]
pub struct Config {
    pub timestep: f64,
    pub terminal: Option<f64>,
    pub buffer_size: usize,
    pub mailbox_size: usize,
    pub live: bool,
    pub logs: bool,
    pub mail: bool,
    pub asyncronous: bool,
}

impl Config {
    pub fn new(
        timestep: f64,
        terminal: Option<f64>,
        buffer_size: usize,
        mailbox_size: usize,
        live: bool,
        logs: bool,
        mail: bool,
        asyncronous: bool,
    ) -> Self {
        Config {
            timestep,
            terminal,
            buffer_size,
            mailbox_size,
            live,
            logs,
            mail,
            asyncronous,
        }
    }
}

impl<T: Send + Sync + Clone + 'static> World<T> {
    /// Create a new world with the given configuration.
    /// By default, this will include a toggleable CLI for real-time simulation control, a logger for state logging, an asynchronous runtime, and a mailbox for message passing between agents.
    pub fn create(config: Config) -> Self {
        let (sim_tx, sim_rx) = channel(config.buffer_size);
        let (pause_tx, pause_rx) = watch::channel(false);
        let mailbox = Mailbox::new(config.mailbox_size, pause_rx.clone());
        World {
            overflow: BTreeSet::new(),
            clock: Clock::new(256, 1, config.timestep, config.terminal).unwrap(),
            savedmail: BTreeSet::new(),
            agents: Vec::new(),
            mailbox,
            runtype: (config.live, config.logs, config.mail, config.asyncronous),
            state: None,
            sender: sim_tx,
            runtime: sim_rx,
            pause_tx,
            pause_rx,
            logger: Logger::new(),
        }
    }
    /// Spawn a new agent into the world.
    pub fn spawn(&mut self, agent: Box<dyn Agent<T>>) -> usize {
        self.agents.push(agent);
        self.agents.len() - 1
    }

    fn spawn_cli(&self, cmd_tx: Sender<ControlCommand>) {
        tokio::spawn(async move {
            let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).await.is_ok() {
                    let cmd = match line.trim() {
                        "pause" => ControlCommand::Pause,
                        "resume" => ControlCommand::Resume,
                        "quit" => ControlCommand::Quit,
                        cmd if cmd.starts_with("speed ") => {
                            if let Some(scale) = cmd
                                .split_whitespace()
                                .nth(1)
                                .and_then(|s| s.parse::<f64>().ok())
                            {
                                ControlCommand::SetTimeScale(scale)
                            } else {
                                continue;
                            }
                        }
                        cmd if cmd.starts_with("schedule ") => {
                            let parts: Vec<_> = cmd.split_whitespace().collect();
                            if parts.len() >= 3 {
                                if let (Some(time), Some(idx)) =
                                    (parts[1].parse::<f64>().ok(), parts[2].parse::<usize>().ok())
                                {
                                    ControlCommand::Schedule(time, idx)
                                } else {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    };
                    if cmd_tx.send(cmd).await.is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        });
    }

    fn log_mail(&mut self, msg: Message<T>) {
        self.savedmail.insert(msg);
    }

    fn commit(&mut self, event: Event) {
        let event_maybe = self.clock.insert(event);
        if event_maybe.is_err() {
            self.overflow.insert(Reverse(event_maybe.err().unwrap()));
        }
    }
    /// Pause the real-time simulation.
    pub fn pause(&self) -> Result<(), SimError> {
        let pause = self.pause_tx.send(true);
        if pause.is_err() {
            return Err(SimError::PlaybackFroze);
        }
        Ok(())
    }
    /// Resume the real-time simulation.
    pub fn resume(&self) -> Result<(), SimError> {
        let resume = self.pause_tx.send(false);
        if resume.is_err() {
            return Err(SimError::PlaybackFroze);
        }
        Ok(())
    }
    /// speed up/slow down the simulation playback.
    pub fn rescale_time(&mut self, timescale: f64) {
        self.clock.time.timescale = timescale;
    }
    /// Get the current time of the simulation.
    pub fn now(&self) -> f64 {
        self.clock.time.time
    }
    /// Get the current step of the simulation.
    pub fn step_counter(&self) -> usize {
        self.clock.time.step
    }
    /// Clone the current state of the simulation.
    pub fn state(&self) -> Option<State> {
        self.state.clone()
    }

    /// Block the long term actions of agents !!this is broken since the shift to the timing wheel!!
    // pub fn block_agent(&mut self, idx: usize, until: Option<f64>) -> Result<(), SimError> {
    //     if self.agents.len() <= idx {
    //         return Err(SimError::InvalidIndex);
    //     }
    //     if until.is_none() {
    //         self.overflow.retain(|x| x.0.agent != idx);
    //     }
    //     self.overflow
    //         .retain(|x| x.0.agent != idx && x.0.time < until.unwrap());
    //     Ok(())
    // }
    // /// remove a particular pending event !!this is broken since the shift to the timing wheel!!
    // pub fn remove_event(&mut self, idx: usize, time: f64) -> Result<(), SimError> {
    //     if self.agents.len() <= idx {
    //         return Err(SimError::InvalidIndex);
    //     } else if self.clock.time.time > time {
    //         return Err(SimError::TimeTravel);
    //     }
    //     self.overflow
    //         .retain(|x| x.0.agent != idx && x.0.time != time);
    //     Ok(())
    // }

    /// Schedule an event for an agent at a given time.
    pub fn schedule(&mut self, time: f64, agent: usize) -> Result<(), SimError> {
        if time < self.clock.time.time {
            return Err(SimError::TimeTravel);
        } else if time > self.clock.time.terminal.unwrap_or(f64::INFINITY) {
            return Err(SimError::PastTerminal);
        }
        self.commit(Event::new(time, agent, Action::Wait));
        Ok(())
    }

    /// Run the simulation.
    pub async fn run(&mut self) -> Result<(), SimError> {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::channel(100);
        if self.runtype.0 {
            self.spawn_cli(cmd_tx.clone());
        }
        loop {
            if self.clock.time.time + self.clock.time.timestep
                > self.clock.time.terminal.unwrap_or(f64::INFINITY)
            {
                break;
            }
            if self.runtype.0 {
                while let Ok(cmd) = cmd_rx.try_recv() {
                    match cmd {
                        ControlCommand::Pause => self.pause()?,
                        ControlCommand::Resume => self.resume()?,
                        ControlCommand::SetTimeScale(scale) => self.rescale_time(scale),
                        ControlCommand::Quit => break,
                        ControlCommand::Schedule(time, idx) => {
                            self.commit(Event::new(time, idx, Action::Wait));
                        }
                    }
                }
                if *self.pause_rx.borrow() {
                    tokio::select! {
                        _ = self.pause_rx.changed() => {},
                        cmd = cmd_rx.recv() => {
                            if let Some(cmd) = cmd {
                                match cmd {
                                    ControlCommand::Pause => self.pause()?,
                                    ControlCommand::Resume => self.resume()?,
                                    ControlCommand::SetTimeScale(scale) => self.rescale_time(scale),
                                    ControlCommand::Quit => break,
                                    ControlCommand::Schedule(time, idx) => {
                                        self.commit(Event::new(time, idx, Action::Wait));
                                    }
                                }
                            }
                        }
                    }
                    continue;
                }
            }
            if self.runtype.3 {
                while let Ok(event) = self.runtime.try_recv() {
                    self.commit(event);
                }
            }
            match self.clock.tick(&mut self.overflow) {
                Ok(events) => {
                    // if self.clock.time.step % 10000 == 0 {
                    //     println!("Processing events {:?}", self.clock.time.time);
                    // }
                    for event in events {
                        if self.runtype.0 {
                            tokio::time::sleep(Duration::from_millis(
                                (self.clock.time.timestep * 1000.0 / self.clock.time.timescale)
                                    as u64,
                            ))
                            .await;
                        }
                        if event.time > self.clock.time.terminal.unwrap_or(f64::INFINITY) {
                            break;
                        }
                        if self.runtype.2 {
                            self.mailbox.collect_messages().await;
                        }
                        let agent = &mut self.agents[event.agent];
                        let event = agent
                            .step(&mut self.state, &event.time, &mut self.mailbox)
                            .await;
                        if self.runtype.1 {
                            let agent_states: BTreeMap<
                                usize,
                                Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>,
                            > = self
                                .agents
                                .iter()
                                .enumerate()
                                .filter_map(|(i, agt)| {
                                    agt.as_any()
                                        .downcast_ref::<Box<dyn Loggable<T>>>()
                                        .map(|loggable| (i, loggable.get_state()))
                                })
                                .collect();
                            self.logger.log(
                                self.now(),
                                self.state.clone(),
                                agent_states,
                                event.clone(),
                            );
                        }
                        match event.yield_ {
                            Action::Timeout(time) => {
                                if self.clock.time.time + time
                                    > self.clock.time.terminal.unwrap_or(f64::INFINITY)
                                {
                                    continue;
                                }
                                let new = Event::new(
                                    self.clock.time.time + time,
                                    event.agent,
                                    Action::Wait,
                                );
                                self.commit(new);
                            }
                            Action::Schedule(time) => {
                                let new = Event::new(time, event.agent, Action::Wait);
                                self.commit(new);
                            }
                            Action::Trigger { time, idx } => {
                                let new = Event::new(time, idx, Action::Wait);
                                self.commit(new);
                            }
                            Action::Wait => {}
                            Action::Break => {
                                break;
                            }
                        }
                        if self.runtype.0 {
                            while let Ok(cmd) = cmd_rx.try_recv() {
                                match cmd {
                                    ControlCommand::Pause => self.pause()?,
                                    ControlCommand::Resume => self.resume()?,
                                    ControlCommand::SetTimeScale(scale) => self.rescale_time(scale),
                                    ControlCommand::Quit => break,
                                    ControlCommand::Schedule(time, idx) => {
                                        self.commit(Event::new(time, idx, Action::Wait));
                                    }
                                }
                            }
                        }
                    }
                }
                Err(SimError::NoEvents) => {
                    if self.runtype.3 {
                        if let Some(event) = self.runtime.recv().await {
                            self.commit(event);
                            continue;
                        }
                    }
                    continue;
                }
                Err(_) => {}
            }
        }
        Ok(())
    }
}

/// A hierarchical timing wheel for scheduling events in a simulation.
struct Clock {
    wheels: Vec<VecDeque<Vec<Event>>>,
    slots: usize,
    height: usize,
    epoch: usize,
    time: Time,
}

impl Clock {
    fn new(
        slots: usize,
        height: usize,
        timestep: f64,
        terminal: Option<f64>,
    ) -> Result<Self, SimError> {
        if height < 1 {
            return Err(SimError::NoClock);
        }
        let mut wheels = Vec::new();
        for _ in 0..height {
            let mut wheel = VecDeque::new();
            for _ in 0..slots {
                wheel.push_back(Vec::new());
            }
            wheels.push(wheel);
        }
        Ok(Clock {
            wheels,
            slots,
            height,
            epoch: 0,
            time: Time {
                time: 0.0,
                step: 0,
                timestep: timestep,
                timescale: 1.0,
                terminal: terminal,
            },
        })
    }

    /// Insert an event into the timing wheel.
    /// checks how many timesteps into the future the event is and selects the appropriate wheel before finding a slot. If too far into the future, the event is returned.
    fn insert(&mut self, event: Event) -> Result<(), Event> {
        let time = event.time;
        let delta = time - self.time.time - self.time.timestep;

        for k in 0..self.height {
            let startidx = ((self.slots).pow(1 + k as u32) - self.slots) / (self.slots - 1);
            let futurestep = (delta / self.time.timestep) as usize;
            if futurestep >= startidx {
                if futurestep
                    >= ((self.slots).pow(1 + self.height as u32) - self.slots) / (self.slots - 1)
                {
                    return Err(event);
                }
                let offset = (futurestep - startidx) / self.slots.pow(k as u32);
                self.wheels[k][offset].push(event);
                return Ok(());
            }
        }
        Err(event)
    }

    /// Pop the next timestep's events from the timing wheel and roll the wheel forward.
    fn tick(&mut self, overflow: &mut BTreeSet<Reverse<Event>>) -> Result<Vec<Event>, SimError> {
        let events: Vec<Event> = self.wheels[0].pop_front().unwrap();
        if !events.is_empty() && events[0].time < self.time.time {
            return Err(SimError::TimeTravel);
        }
        self.wheels[0].push_back(Vec::new());
        self.time.time += self.time.timestep;
        self.time.step += 1;
        if (self.time.time / self.time.timestep) as u64 % self.slots as u64 == 0 {
            self.epoch += 1;
            self.rotate(overflow);
        }
        if events.is_empty() {
            return Err(SimError::NoEvents);
        }
        Ok(events)
    }

    /// Rotate the timing wheel, moving events from the k-th wheel to fill the (k-1)-th wheel.
    fn rotate(&mut self, overflow: &mut BTreeSet<Reverse<Event>>) {
        let current_step = self.time.step as u64 + 1;

        for k in 1..self.height {
            let wheel_period = self.slots.pow(k as u32);
            if current_step % (wheel_period as u64) == 0 {
                if self.height == k {
                    for _ in 0..self.slots.pow(self.height as u32 - 1) {
                        overflow.pop_first().map(|event| self.insert(event.0));
                    }
                    return;
                }
                if let Some(higher_events) = self.wheels[k].pop_front() {
                    for event in higher_events {
                        self.insert(event).unwrap();
                    }
                    self.wheels[k].push_back(Vec::new());
                }
            }
        }
    }
}

use std::any::Any;
use std::sync::{Arc, Mutex};
use std::collections::BinaryHeap;
use std::cmp::{Ordering, Reverse};
use futures::future::Future;
use futures::task::{Context, Poll};
use std::pin::Pin;

pub trait Agent: Send {
    fn step(&mut self, state: &mut Option<State>, time: f64) -> Event;
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
        Event { time, agent, yield_ }
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
    terminal: Option<f64>,
}

enum SimError {
    TimeTravel,
    PastTerminal,
}

pub struct World {
    pending: BinaryHeap<Reverse<Event>>,
    logged: BinaryHeap<Event>,
    agents: Vec<Box<dyn Agent>>,
    time: Time,
    state: Option<State>,
}

unsafe impl Send for World {}
unsafe impl Sync for World {}

impl World {
    pub fn create(timestep: f64, terminal: Option<f64>) -> Self {
        let time = Time {
            time: 0.0,
            step: 0,
            timestep,
            terminal,
        };
        World {
            pending: BinaryHeap::new(),
            logged: BinaryHeap::new(),
            agents: Vec::new(),
            time,
            state: None,
        }
    }

    pub fn spawn(&mut self, agent: Box<dyn Agent>) {
        self.agents.push(agent);
    }

    pub fn run(&mut self) -> Result<(), SimError> {
        while let Some(event) = self.pending.pop() {
            let event = event.0;
            if event.time > self.time.terminal.unwrap_or(f64::INFINITY) {
                return Err(SimError::PastTerminal);
            }
            if event.time < self.time.time {
                return Err(SimError::TimeTravel);
            }
            self.time.time = event.time;
            self.time.step += 1;
            let agent = &mut self.agents[event.agent];

            let event = agent.step(&mut self.state, event.time.clone());
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
        }
        Ok(())
    }

    pub fn now(&self) -> f64 {
        self.time.time
    }

    pub fn step_counter(&self) -> usize {
        self.time.step
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
}
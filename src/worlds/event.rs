use std::cmp::Ordering;

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
    pub time: f64,
    pub agent: usize,
    pub yield_: Action,
}

impl Event {
    pub fn new(time: f64, agent: usize, yield_: Action) -> Self {
        Self {
            time,
            agent,
            yield_,
        }
    }

    pub fn time(&self) -> f64 {
        self.time
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

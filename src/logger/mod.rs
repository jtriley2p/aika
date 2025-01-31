use crate::worlds::{AgentState, Event, State};
use std::collections::{BTreeMap, BinaryHeap};

mod snapshot;

pub use snapshot::Snapshot;

/// A logger for recording snapshots of the world.
pub struct Logger<U: Send + Sync + Clone> {
    snapshots: BinaryHeap<Snapshot<U>>,
    events: BinaryHeap<Event>,
}

impl<U: Send + Sync + Clone> Logger<U> {
    /// Create a new logger.
    pub fn new() -> Self {
        Logger {
            snapshots: BinaryHeap::new(),
            events: BinaryHeap::new(),
        }
    }
    /// Log a snapshot of the world.
    pub fn log(
        &mut self,
        timestamp: f64,
        shared_state: Option<State<U>>,
        agent_states: Vec<&'static dyn AgentState>,
        event: Event,
    ) {
        self.snapshots.push(Snapshot {
            timestamp,
            shared_state,
            agent_states,
        });
        self.events.push(event);
    }
    /// Get the events logged.
    pub fn get_snapshots(&self) -> BinaryHeap<Snapshot<U>> {
        self.snapshots.clone()
    }
}

use crate::worlds::{AgentState, State};
use std::collections::BTreeMap;

/// A snapshot of the world at a given time.
#[derive(Clone)]
pub struct Snapshot<U> {
    pub timestamp: f64,
    pub shared_state: Option<State<U>>,
    pub agent_states: Vec<&'static dyn AgentState>,
}

impl<U: Send + Sync + Clone> PartialEq for Snapshot<U> {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp
    }
}

impl<U: Send + Sync + Clone> Eq for Snapshot<U> {}

impl<U: Send + Sync + Clone> PartialOrd for Snapshot<U> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<U: Send + Sync + Clone> Ord for Snapshot<U> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.timestamp.partial_cmp(&other.timestamp).unwrap()
    }
}

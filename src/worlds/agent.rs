use futures::future::BoxFuture;

use super::{Event, Mailbox, State};

/// An agent that can be run in a simulation.
pub trait Agent<U: Send + Sync + Clone, T: Send + Sync + Clone>: Send {
    fn step<'a>(
        &'a mut self,
        state: &'a mut Option<State<U>>,
        time: &f64,
        mailbox: &'a mut Mailbox<T>,
    ) -> BoxFuture<'a, Event>;
    fn get_state<'a>(&self) -> &'a dyn AgentState;
}

pub trait AgentState: Send + Sync {}
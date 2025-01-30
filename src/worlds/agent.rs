use futures::future::BoxFuture;
use std::any::Any;

use super::{Event, Mailbox, State};

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

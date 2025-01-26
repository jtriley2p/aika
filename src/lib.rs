use futures::future::BoxFuture;
use worlds::{Action, Agent, Event, Mailbox, State};

extern crate tokio;

mod logger;
mod universes;
mod worlds;

pub struct TestAgent {
    pub id: usize,
    pub name: String,
}

impl TestAgent {
    pub fn new(id: usize, name: String) -> Self {
        TestAgent { id, name }
    }
}

impl Agent for TestAgent {
    fn step<'a>(
        &'a mut self,
        state: &'a mut Option<State>,
        time: f64,
        mailbox: &'a mut Mailbox,
    ) -> BoxFuture<'a, Event> {
        let event = Event::new(time, self.id, Action::Timeout(1.0));
        Box::pin(async { event })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::worlds::*;
    use super::*;
    use tokio::*;

    #[tokio::test(flavor = "current_thread")]
    async fn test_run() {
        let mut world = World::create(1.0, Some(20.0), 100, 100);
        let agent_test = TestAgent::new(0, "Test".to_string());
        world.spawn(Box::new(agent_test));
        world.rescale_time(5.0);
        schedule(world.sender.clone(), 0.0, 0).await.unwrap();
        assert!(world.run(true, false).await.unwrap() == ());
    }
}

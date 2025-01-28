use std::sync::{Arc, Mutex};

use futures::future::BoxFuture;
use worlds::{Action, Agent, Event, Loggable, Mailbox, Message, State};

extern crate tokio;

pub mod logger;
pub mod universes;
pub mod worlds;

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
        time: &f64,
        mailbox: &'a mut Mailbox,
    ) -> BoxFuture<'a, Event> {
        let event = Event::new(*time, self.id, Action::Timeout(1.0));
        Box::pin(async { event })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::worlds::*;
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn test_offline_run() {
        let mut world = World::create(1.0, Some(2000000.0), 100, 100);
        let agent_test = TestAgent::new(0, "Test".to_string());
        world.spawn(Box::new(agent_test));
        world.schedule(0.0, 0).unwrap();
        assert!(world.run(false, false, false).await.unwrap() == ());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_bench() {
        // Benchmark parameters
        let duration_secs = 2000000;
        let timestep = 1.0; // 1ms timestep for fine-grained events
        let terminal = Some(duration_secs as f64);

        // Create world with smaller buffers for faster processing
        let mut world = World::create(timestep, terminal, 1000, 1000);

        // Add multiple agents to generate more events
        let agent = TestAgent::new(0, format!("Test{}", 0));
        world.spawn(Box::new(agent));
        // Schedule initial events for each agent
        world.schedule(0.0, 0).unwrap();

        // Start timing
        let start = Instant::now();

        // Run simulation without live mode or logging for maximum performance
        world.run(false, false, false).await.unwrap();

        let elapsed = start.elapsed();
        let total_steps = world.step_counter();

        println!("Benchmark Results:");
        println!("Total time: {:.2?}", elapsed);
        println!("Total events processed: {}", total_steps);
        println!(
            "Events per second: {:.2}",
            total_steps as f64 / elapsed.as_secs_f64()
        );
        println!(
            "Average event processing time: {:.3?} per event",
            elapsed / total_steps as u32
        );
    }
}

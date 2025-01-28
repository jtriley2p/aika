use aika::logger::*;
use aika::universes::*;
use aika::worlds::*;
use aika::TestAgent;

#[tokio::main]
async fn main() {
    let mut world = World::create(1.0, Some(2000000.0), 100, 100);
    let agent_test = TestAgent::new(0, "Test".to_string());
    world.spawn(Box::new(agent_test));
    world.schedule(0.0, 0).unwrap();
    world.run(true, true, false).await.unwrap();
    // for testing real-time run command line features like pause, resume, and speed up and slow down
}

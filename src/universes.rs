use crate::worlds::SimError;
use anyhow::Result;
use futures::executor::block_on;
use rayon::prelude::*;

use super::worlds::*;

pub struct Universe {
    pub worlds: Vec<World>,
}

impl Universe {
    pub fn new() -> Self {
        Universe { worlds: Vec::new() }
    }

    pub fn add_world(&mut self, world: World) {
        self.worlds.push(world);
    }

    pub fn run_parallel(&mut self, live: bool, logs: bool) -> Result<Vec<Result<(), SimError>>> {
        let results: Vec<_> = self
            .worlds
            .par_iter_mut()
            .map(|world| block_on(world.run(live, logs)))
            .collect();
        Ok(results)
    }

    pub fn pause_all(&mut self) {
        self.worlds.par_iter().for_each(|world| world.pause());
    }

    pub fn resume_all(&mut self) {
        self.worlds.par_iter().for_each(|world| world.resume());
    }
}

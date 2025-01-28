use crate::worlds::SimError;
use anyhow::Result;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

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

    pub async fn run_parallel(
        &mut self,
        live: bool,
        logs: bool,
        mail: bool,
    ) -> Result<Vec<Result<(), SimError>>> {
        let mut handles = vec![];
        let worlds = std::mem::take(&mut self.worlds);
        for mut world in worlds {
            let handle = tokio::spawn(async move { world.run(live, logs, mail).await });
            handles.push(handle);
        }
        let results = futures::future::join_all(handles).await;
        let results = results
            .into_iter()
            .map(|result| match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(e)) => Err(e),
                Err(e) => Err(SimError::TokioError(e.to_string())),
            })
            .collect();

        Ok(results)
    }

    pub fn pause_all(&mut self) -> Result<(), Vec<SimError>> {
        let errors: Vec<_> = self
            .worlds
            .par_iter()
            .filter_map(|world| world.pause().err())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    pub fn resume_all(&mut self) -> Result<(), Vec<SimError>> {
        let errors: Vec<_> = self
            .worlds
            .par_iter()
            .filter_map(|world| world.resume().err())
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

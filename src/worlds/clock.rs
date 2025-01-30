use anyhow::Result;
use std::cmp::Reverse;
use std::collections::{BTreeSet, VecDeque};

use super::{Event, SimError};

/// The relevant time information for the simulation.
pub struct Time {
    pub time: f64,
    pub step: usize,
    pub timestep: f64,
    pub timescale: f64, // 1.0 = real-time, 0.5 = half-time, 2.0 = double-time
    pub terminal: Option<f64>,
}

/// A hierarchical timing wheel for scheduling events in a simulation.
pub struct Clock {
    wheels: Vec<VecDeque<Vec<Event>>>,
    slots: usize,
    height: usize,
    epoch: usize,
    pub time: Time,
}

impl Clock {
    pub fn new(
        slots: usize,
        height: usize,
        timestep: f64,
        terminal: Option<f64>,
    ) -> Result<Self, SimError> {
        if height < 1 {
            return Err(SimError::NoClock);
        }
        let mut wheels = Vec::new();
        for _ in 0..height {
            let mut wheel = VecDeque::new();
            for _ in 0..slots {
                wheel.push_back(Vec::new());
            }
            wheels.push(wheel);
        }
        Ok(Clock {
            wheels,
            slots,
            height,
            epoch: 0,
            time: Time {
                time: 0.0,
                step: 0,
                timestep: timestep,
                timescale: 1.0,
                terminal: terminal,
            },
        })
    }

    /// Insert an event into the timing wheel.
    /// checks how many timesteps into the future the event is and selects the appropriate wheel before finding a slot. If too far into the future, the event is returned.
    pub fn insert(&mut self, event: Event) -> Result<(), Event> {
        let time = event.time();
        let delta = time - self.time.time - self.time.timestep;

        for k in 0..self.height {
            let startidx = ((self.slots).pow(1 + k as u32) - self.slots) / (self.slots - 1);
            let futurestep = (delta / self.time.timestep) as usize;
            if futurestep >= startidx {
                if futurestep
                    >= ((self.slots).pow(1 + self.height as u32) - self.slots) / (self.slots - 1)
                {
                    return Err(event);
                }
                let offset = (futurestep - startidx) / self.slots.pow(k as u32);
                self.wheels[k][offset].push(event);
                return Ok(());
            }
        }
        Err(event)
    }

    /// Pop the next timestep's events from the timing wheel and roll the wheel forward.
    pub fn tick(
        &mut self,
        overflow: &mut BTreeSet<Reverse<Event>>,
    ) -> Result<Vec<Event>, SimError> {
        let events: Vec<Event> = self.wheels[0].pop_front().unwrap();
        if !events.is_empty() && events[0].time() < self.time.time {
            return Err(SimError::TimeTravel);
        }
        self.wheels[0].push_back(Vec::new());
        self.time.time += self.time.timestep;
        self.time.step += 1;
        if (self.time.time / self.time.timestep) as u64 % self.slots as u64 == 0 {
            self.epoch += 1;
            self.rotate(overflow);
        }
        if events.is_empty() {
            return Err(SimError::NoEvents);
        }
        Ok(events)
    }

    /// Rotate the timing wheel, moving events from the k-th wheel to fill the (k-1)-th wheel.
    pub fn rotate(&mut self, overflow: &mut BTreeSet<Reverse<Event>>) {
        let current_step = self.time.step as u64 + 1;

        for k in 1..self.height {
            let wheel_period = self.slots.pow(k as u32);
            if current_step % (wheel_period as u64) == 0 {
                if self.height == k {
                    for _ in 0..self.slots.pow(self.height as u32 - 1) {
                        overflow.pop_first().map(|event| self.insert(event.0));
                    }
                    return;
                }
                if let Some(higher_events) = self.wheels[k].pop_front() {
                    for event in higher_events {
                        self.insert(event).unwrap();
                    }
                    self.wheels[k].push_back(Vec::new());
                }
            }
        }
    }
}

use std::time::SystemTime;

use legion::{Resources, Schedule, World};

use crate::protos::components::{MoveIntent, Velocity};

use super::system;


pub struct Simulation {
    // World ECS state
    world: World,
    // Schedule for what goes down during a tick
    schedule: Schedule,
    // Current tick id to be or being processed
    tick: u64,
    // Timestamp of when the last tick started processing (not finished)
    last_tick_time: SystemTime,
    // Rate at which ticks occurr in seconds
    tick_rate_secs: f32,
    // How much time in seconds should the next tick occurr
    next_tick_start_secs: f32,
}

unsafe impl Send for Simulation {}
unsafe impl Sync for Simulation {}

impl Simulation {
    pub fn new(tick_rate_secs: f32) -> Self {
        Self {
            world: World::default(),
            schedule: system::new_schedule(),
            tick: 1u64,
            last_tick_time: SystemTime::now(),
            tick_rate_secs: tick_rate_secs,
            next_tick_start_secs: 0f32,
        }
    }

    //TODO: Protect incremeneting tick past max
    pub fn update_tick_data(
        &mut self,
        tick: u64,
        last_tick_time: SystemTime,
        next_tick_start_secs: f32,
    ) {
        self.tick = tick;
        self.last_tick_time = last_tick_time;
        self.next_tick_start_secs = next_tick_start_secs
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn last_tick_time(&self) -> SystemTime {
        self.last_tick_time
    }

    pub fn world_mut(&mut self) -> &mut World {
        &mut self.world
    }

    pub fn schedule_mut(&mut self) -> &mut Schedule {
        &mut self.schedule
    }

    pub fn tick_rate_secs(&self) -> f32 {
        self.tick_rate_secs
    }

    pub fn execute_schedule(&mut self, resources: &mut Resources) {
        self.schedule.execute(&mut self.world, resources);
    }

    pub fn next_tick_start_secs(&self) -> f32 {
        self.next_tick_start_secs
    }
}
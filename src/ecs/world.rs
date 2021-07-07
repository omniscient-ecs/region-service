use std::{sync::Arc, thread};
use legion::Entity;
use core::time::Duration;
use super::{region::{EntityUpdate, IntentError, UpdateComponent}, simulation::Simulation, system};
use crate::ecs::region::Intent;
use crate::ecs::region::RegionInstance;
use std::any::Any;
use tokio::sync::Mutex;
use tokio::sync::RwLock;
use tokio::time::Instant;
use tokio::{
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use tokio::{sync::broadcast, task};

pub struct WorldContext {
    // Unique storage / global / context ID assosiated with this context
    object_id: String,
    // Primary entity that this context drives.
    // Todo see if we can get away with not having a rwlock?
    entity_id: Entity,

    // Current attached communication channels for active region this world context exists in
    region_intent_sender: Arc<RwLock<Option<mpsc::UnboundedSender<Box<dyn EntityUpdate>>>>>,
    region_update_receiver: Arc<Mutex<Option<broadcast::Receiver<Box<dyn Intent>>>>>,
}

impl WorldContext {
    pub fn new(
        object_id: String,
        entity_id: Entity,
        intent_sender: mpsc::UnboundedSender<Box<dyn EntityUpdate>>,
        update_broadcast_receiver: broadcast::Receiver<Box<dyn Intent>>,
    ) -> Self {
        Self {
            object_id: object_id,
            entity_id: entity_id,
            region_intent_sender: Arc::new(RwLock::new(Some(intent_sender))),
            region_update_receiver: Arc::new(Mutex::new(Some(update_broadcast_receiver))),
        }
    }

    // Submits an intent to be processed by the owned region
    pub async fn submit_intent<T: Any + Sized + Send + Sync>(
        &self,
        component: T,
    ) -> Result<(), IntentError> {
        let region_intent_sender = self.region_intent_sender.read().await;
        match region_intent_sender
            .as_ref()
            .unwrap()
            .send(Box::new(UpdateComponent::new(self.entity_id, component)))
        {
            Ok(_) => Ok(()),
            Err(e) => {
                println!(
                    "cannot submit intent for world_context {} with err {}",
                    self.object_id, e
                );
                Err(IntentError)
            }
        }
    }

    // Updates the context of this world instance to have a new region intent sender and update receiver
    // This operation happens rarely and only when traveling in between regions
    pub async fn update_context(
        &self,
        intent_sender: mpsc::UnboundedSender<Box<dyn EntityUpdate>>,
        update_broadcast_receiver: broadcast::Receiver<Box<dyn Intent>>,
    ) {
        // Ensure we drop the lock in between operations
        {
            let mut region_intent_sender = self.region_intent_sender.write().await;
            region_intent_sender.replace(intent_sender);
        }

        let mut region_update_receiver = self.region_update_receiver.lock().await;
        region_update_receiver.replace(update_broadcast_receiver);
    }

    pub async fn watch_updates(&self, update_handler: fn(Box<dyn Intent>)) {
        loop {
            let mut region_update_receiver = self.region_update_receiver.lock().await;

            match region_update_receiver.as_mut().unwrap().try_recv() {
                Ok(update) => update_handler(update),
                Err(e) => {
                    //TODO: Check empty vs lagged
                    println!("error occured while proccessing broadcast channel! consider increasing channel size. {}", e)
                }
            }
        }
    }
}

pub struct WorldInstance {
    id: String,
    regions: Vec<Vec<RegionInstance>>,
    region_x_count: i32,
    region_y_count: i32,
    // Rate at which ticks run at (time between ticks)
    tick_rate_secs: f32,
}

impl WorldInstance {
    pub fn new(
        id: String,
        region_x_count: i32,
        region_y_count: i32,
        _region_width_x: i32,
        _region_width_y: i32,
        tick_rate_hz: i32,
    ) -> WorldInstance {
        let mut regions_x: Vec<Vec<RegionInstance>> = Vec::new();

        for x in 0..region_x_count {
            let mut regions = Vec::new();
            for y in 0..region_y_count {
                regions.push(RegionInstance::new(x, y));
            }
            regions_x.push(regions);
        }

        WorldInstance {
            id: id,
            regions: regions_x,
            region_x_count: region_x_count,
            region_y_count: region_y_count,
            tick_rate_secs: 1f32 / tick_rate_hz as f32,
        }
    }

    pub fn tick_rate_secs(&self) -> f32 {
        self.tick_rate_secs
    }

    // This async func can take some time!
    pub async fn create_context(
        &self,
        region_x: i32,
        region_y: i32,
        object_id: String,
    ) -> WorldContext {
        //TODO: Validate x/y and throw exception
        let region = &self.regions[region_x as usize][region_y as usize];
        //TODO: I'm pretty sure there's a better pattern than awaiting at the end of a function?
        region.create_world_context(object_id).await
    }

    // Runs new simulation for an unlimited amount of ticks
    // See run_simulation for more information
    pub fn run(&mut self) {
        self.run_simulation(u64::MAX);
    }

    // Runs a simulation with the default builder
    pub fn run_simulation(&mut self, simulate_tick_count: u64) {
        let s = self.tick_rate_secs;
        self.run_simulation_with_builder(simulate_tick_count, &|| {
            Simulation::new(s)
        });
    }
    // Runs a new simulation based on this world and its regions for the amount of ticks requested
    // All regions are dispatched into individual threads using tokio
    // Each region gets its own simulation context for processing
    pub fn run_simulation_with_builder(&mut self, simulate_tick_count: u64, simulation_builder: &dyn Fn() -> Simulation) {
        // 60 hz = one tick every 16.67 ms or .0167 second
        let world_regions = &mut self.regions;
        world_regions.iter_mut().for_each(|regions| {
            regions.iter_mut().for_each(|region| {
                let mut r = region.clone();
                let mut simulation = simulation_builder();
                //TODO: Remove to dedicated thread pool, leaving the default pool for I/O operations only.
                // Note: Tried rayon vs tokio spawn_blocking, rayon requires specific thread pool tuning and tokio is working great by default
                // Last test yielded almost 10x faster performance on tokio > rayon
                task::spawn_blocking(move || {
                    loop {
                        r.process_tick(&mut simulation);

                        // Simulation Complete
                        if simulation.tick() > simulate_tick_count {
                            break;
                        }

                        let next_tick_start_secs = simulation.next_tick_start_secs();
                        thread::sleep(Duration::from_secs_f32(next_tick_start_secs));
                    }
                });
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::protos::components::Velocity;
use crate::protos::components::MoveIntent;

    use super::*;
    use tokio_test::assert_ok;
    use rayon::prelude::*;

    #[test]
    fn new_world_test() {
        let id = "1";
        let world = WorldInstance::new(String::from(id), 1, 1, 100, 100, 60);

        assert_eq!(world.id, id);
        assert_eq!(world.regions.len(), 5);
        assert_eq!(world.regions[0].len(), 5);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);

        world.run_simulation(1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn create_context_and_intent() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);
        let object_id = String::from("1000");
        world.run_simulation(5); // Simulate 10 ticks so it may consume the context
        let context = world.create_context(0, 0, object_id.clone()).await;
        let result = context.submit_intent(MoveIntent::new()).await;
        assert_eq!(context.object_id, object_id);
        assert_ok!(result);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick_5x5_world() {
        let mut world = WorldInstance::new(String::from("1"), 5, 5, 100, 100, 60);

        world.run_simulation(1);
    }


    #[tokio::test(flavor = "multi_thread", worker_threads = 500)]
    async fn process_tick_50x50_world() {
        let mut world = WorldInstance::new(String::from("1"), 50, 50, 100, 100, 60);

        world.run_simulation(1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick_25_times() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);
        world.run_simulation(25);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick_25_times_with_1000_entities() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);
        let tick_rate_secs = world.tick_rate_secs();

        world.run_simulation_with_builder(25, &|| {
            let mut simulation = Simulation::new(tick_rate_secs);
            let world = simulation.world_mut();
            for i in 0..1000 {
                world.push((i.to_string(), MoveIntent::new(), Velocity::new()));
            }
            simulation
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick_25_times_with_10000_entities() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);
        let tick_rate_secs = world.tick_rate_secs();
        
        world.run_simulation_with_builder(25, &|| {
            let mut simulation = Simulation::new(tick_rate_secs);
            let world = simulation.world_mut();
            for i in 0..10000 {
                world.push((i.to_string(), MoveIntent::new(), Velocity::new()));
            }
            simulation
        });
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn process_tick_25_times_with_100000_entities() {
        let mut world = WorldInstance::new(String::from("1"), 1, 1, 100, 100, 60);
        let tick_rate_secs = world.tick_rate_secs();
        
        world.run_simulation_with_builder(25, &|| {
            let mut simulation = Simulation::new(tick_rate_secs);
            let world = simulation.world_mut();
            for i in 0..100000 {
                world.push((i.to_string(), MoveIntent::new(), Velocity::new()));
            }
            simulation
        });
    }
}

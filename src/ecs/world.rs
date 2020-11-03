use crate::ecs::region::Intent;
use crate::ecs::region::RegionInstance;
use legion::{
    storage::IntoComponentSource, system, systems::CommandBuffer, Entity, Resources, Schedule,
    World,
};
use rapier3d::dynamics::{IntegrationParameters, JointSet, RigidBodySet};
use rapier3d::geometry::{BroadPhase, ColliderSet, NarrowPhase};
use rapier3d::pipeline::PhysicsPipeline;
use rapier3d::{
    dynamics::BodyStatus, dynamics::RigidBodyBuilder, na::Isometry3, na::Vector3,
    pipeline::EventHandler,
};
use rayon::prelude::*;
use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::pin::Pin;
use std::thread::sleep;
use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc, time::Duration, time::SystemTime};
use tokio::runtime::Runtime;
use tokio::sync::oneshot::{self, Sender};
use tokio::sync::{broadcast, mpsc::error::SendError};
use tokio::sync::{broadcast::error::TryRecvError, Mutex};
use tokio::sync::{Notify, RwLock};
use tokio::task;
use tokio::{
    runtime::Builder,
    sync::mpsc::{self, UnboundedReceiver, UnboundedSender},
};
use rayon::prelude::*;

use super::region::{EntityUpdate, IntentError, UpdateComponent};

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
    tick_rate_hz: i32,
}

impl WorldInstance {
    pub fn new(
        id: String,
        region_x_count: i32,
        region_y_count: i32,
        region_width_x: i32,
        region_width_y: i32,
        tick_rate_hz: i32,
    ) -> WorldInstance {
        let mut regions: Vec<Vec<RegionInstance>> = Vec::new();

        for x in 0..region_x_count {
            for y in 0..region_y_count {
                regions[x as usize][y as usize] = RegionInstance::new(x, y);
            }
        }

        WorldInstance {
            id: id,
            regions: regions,
            region_x_count: region_x_count,
            region_y_count: region_y_count,
            tick_rate_hz: tick_rate_hz,
        }
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
        region.create_world_context(object_id).await
    }

    pub async fn run(&mut self) {
        self.run_simulation(u64::MAX);
    }

    // As per tokio's suggestion, using raynon for blocking CPU bound execution of each tick
    // We will use tokio for I/O and Rayon for heavy physics/update dispatching
    // https://docs.rs/tokio/0.3.2/tokio/task/fn.spawn_blocking.html
    pub async fn run_simulation(&mut self, simulate_tick_count: u64) {
        let mut tick = 0u64;
        // 60 hz = one tick every 16.67 ms or .0167 seconds
        let tick_rate_duration = Duration::from_secs_f32(1f32 / self.tick_rate_hz as f32);
        loop {
            rayon::scope(|s| {
                for regions_x in self.regions.iter_mut() {
                    for region in regions_x.iter_mut() {
                        let r = region;
                        s.spawn(move |_| { 
                            r.run_simulation(simulate_tick_count);
                        });
                    }
                }
            });

            if tick == simulate_tick_count {
                break;
            }
    
            // Increment tick counter and reset once hitting max
            if tick == u64::MAX {
                tick = 1;
            } else if simulate_tick_count > tick {
                tick += 1;
            } else if simulate_tick_count == tick {
                break;
            }

            tokio::time::sleep_until(tokio::time::Instant::now() + tick_rate_duration).await
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::protos::components::Position;

    use super::*;
    use tokio_test::assert_ok;

    #[test]
    fn new_world_test() {
        let id = "1";
        let world = WorldInstance::new(String::from(id), 5, 5, 60);

        assert_eq!(world.id, id);
        assert_eq!(world.regions.len(), 5);
        assert_eq!(world.regions[0].len(), 5);

        let region = &world.regions[1][3];

        assert_eq!(region.x, 1);
        assert_eq!(region.y, 3);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 100)]
    async fn create_entity() {
        let connection_id = "100";
        let world = WorldInstance::new(String::from("1"), 5, 5, 60);
        world.run_simulation(1); // Run only 1 ticks... otherwise the program runs forever
        let context = world.create_context(0, 0, observer).await;
        let result = context.submit_intent(Position::default()).await;
        println!(
            "created world context connection_id: {}, entity_id: {:?}",
            context.object_id, context.entity_id
        );

        assert_eq!(connection_id, context.connection_id);
        assert_ok!(result);
    }
}

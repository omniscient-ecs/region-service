use crate::{ecs::world::WorldContext};
use core::any::Any;
use std::sync::Arc;

use dyn_clone::DynClone;
use legion::Entity;
use legion::Resources;
use legion::World;
use std::fmt;

use std::time::SystemTime;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::*;

use super::simulation::Simulation;

pub struct Tick(u64);
pub struct DeltaTime(f32);

impl DeltaTime {
    pub fn time_since_last_tick_secs(&self) -> f32 {
        self.0
    }
}

pub struct IntentError;

// Implement std::fmt::Display for AppError
impl fmt::Debug for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Could not register intent")
    }
}

pub trait EntityUpdate: Send + Sync {
    fn process_update(self: Box<Self>, world: &mut World);
}

pub struct UpdateComponent<T>
where
    T: Any + Sized + Send + Sync,
{
    entity_id: Entity,
    component: T,
}

impl<T: Any + Sized + Send + Sync> EntityUpdate for UpdateComponent<T> {
    fn process_update(self: Box<Self>, world: &mut World) {
        world.push_with_id(self.entity_id, (self.component,));
    }
}

impl<T: Any + Sized + Send + Sync> UpdateComponent<T> {
    pub fn new(entity_id: Entity, component: T) -> Self {
        Self {
            entity_id: entity_id,
            component: component,
        }
    }
}

struct RegisterContext {
    on_registered_channel: oneshot::Sender<Entity>,
    connection_id: String,
}

impl RegisterContext {
    fn new(on_ready: oneshot::Sender<Entity>, connection_id: String) -> Self {
        Self {
            on_registered_channel: on_ready,
            connection_id: connection_id,
        }
    }
}

impl EntityUpdate for RegisterContext {
    fn process_update(self: Box<Self>, world: &mut World) {
        let entity_id = world.push((self.connection_id,));
        self.on_registered_channel.send(entity_id);
    }
}

// Wrapper for client side intent messages
pub trait Intent: Send + Sync + DynClone {
    fn to_bytes(self: Box<Self>) -> Vec<u8>;
    fn get_component(self: Box<Self>) -> Box<dyn Any>;
}

dyn_clone::clone_trait_object!(Intent);

#[derive(Clone)]
struct ClientIntent<T: Any + Sized + Send + Sync + protobuf::Message + Clone> {
    component: Box<T>,
}

impl<T: Any + Sized + Send + Sync + protobuf::Message + Clone> Intent for ClientIntent<T> {
    fn to_bytes(self: Box<Self>) -> Vec<u8> {
        self.component.write_to_bytes().unwrap()
    }

    fn get_component(self: Box<Self>) -> Box<dyn Any> {
        self.component
    }
}

impl<T: Any + Sized + Send + Sync + protobuf::Message + Clone> ClientIntent<T> {
    pub fn new(component: T) -> Self {
        Self {
            component: Box::new(component),
        }
    }
}

#[derive(Clone)]
pub struct RegionInstance {
    x: i32,
    y: i32,
    intent_sender: mpsc::UnboundedSender<Box<dyn EntityUpdate>>,
    intent_receiver: Arc<Mutex<UnboundedReceiver<Box<dyn EntityUpdate>>>>,
    update_broadcast_sender: broadcast::Sender<Box<dyn Intent>>,
}

impl RegionInstance {
    pub fn new(x: i32, y: i32) -> Self {
        let (intent_sender, intent_receiver) = mpsc::unbounded_channel();
        // TODO: Make broadcast receiver configurable via env var
        //TODO: Is there a nicer way to ignore receiver?
        let (update_broadcast_sender, _receiver) = broadcast::channel(1000);
        Self {
            x: x,
            y: y,
            intent_sender: intent_sender,
            intent_receiver: Arc::new(Mutex::new(intent_receiver)),
            update_broadcast_sender: update_broadcast_sender,
        }
    }

    pub fn process_updates(&mut self, world: &mut World) {
        // NOTE: This lock should be 100%. If it ever panics, something seriously is wrong
        let mut intent_receiver = self.intent_receiver.try_lock().unwrap();
        let mut update_count = 0;
        while let Ok(update) = intent_receiver.try_recv() {
            update.process_update(world);
            update_count += 1;
        }

        println!(
            "successfully processed {} updates for region x:{}, y:{}",
            update_count, self.x, self.y
        );
    }

    // Creates a new world context with the specified global unique object id and region channel assosiated with this region
    //TODO: Switch to result
    pub async fn create_world_context(&self, object_id: String) -> WorldContext {
        let (channel_sender, channel_receiver) = oneshot::channel();
        let register = RegisterContext::new(channel_sender, object_id.clone());

        self.intent_sender.send(Box::new(register));

        let entity_id = channel_receiver.await.unwrap();

        WorldContext::new(
            object_id,
            entity_id,
            self.intent_sender.clone(),
            self.update_broadcast_sender.subscribe(),
        )
    }

    // Registers world context to this specific region.
    // This makes it so the WorldContext send intents and receive updates are contextual to this region
    async fn register_context(&self, world_context: WorldContext) {
        world_context
            .update_context(
                self.intent_sender.clone(),
                self.update_broadcast_sender.subscribe(),
            )
            .await
    }

    pub fn upsert_component<T: Any + Sized + Send + Sync>(
        &self,
        entity_id: Entity,
        component: T,
    ) -> Result<(), IntentError> {
        match self
            .intent_sender
            .send(Box::new(UpdateComponent::new(entity_id, component)))
        {
            Ok(_) => Ok(()),
            Err(_e) => Err(IntentError),
        }
    }

    pub fn process_tick(&mut self, simulation: &mut Simulation) {
        let start_tick_time = SystemTime::now();
        let last_tick_time = simulation.last_tick_time();
        let tick_rate_secs = simulation.tick_rate_secs();
        let tick = simulation.tick();
        let mut world = simulation.world_mut();
        // 1. Process all intents
        self.process_updates(&mut world);

        let mut resources = Resources::default();

        // Set resources for processing ECS
        resources.insert(Tick(tick));
        resources.insert(DeltaTime(last_tick_time.elapsed().unwrap().as_secs_f32()));

        // 3. Execute tick ECS schedule
        simulation.execute_schedule(&mut resources);

        let process_tick_duration = start_tick_time.elapsed().unwrap().as_secs_f32();

        if process_tick_duration <= tick_rate_secs {
            // Successfully completed tick less than duration rate
            simulation.update_tick_data(
                tick + 1,
                start_tick_time,
                tick_rate_secs - process_tick_duration,
            );
        } else {
            // Server could not process tick fast enough {
            // Calculate next tick by skipping all ticks that were missed
            let next_tick = (process_tick_duration / tick_rate_secs).ceil() as u64;
            let next_tick_start_secs = tick_rate_secs - (process_tick_duration % tick_rate_secs);
            simulation.update_tick_data(next_tick, start_tick_time, next_tick_start_secs);
        }

        println!(
            "successfully processed tick {} in {}us for region x:{} y:{}",
            tick,
            start_tick_time.elapsed().unwrap().as_micros(),
            self.x,
            self.y,
        );
    }
}

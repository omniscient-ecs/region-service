use crate::{ecs::world::WorldContext, protos::components::Position};
use core::any::Any;
use core::time::Duration;
use dyn_clone::DynClone;
use legion::Entity;
use legion::Resources;
use legion::{Schedule, World};
use std::{sync::atomic::AtomicBool, fmt};
use std::sync::Arc;
use std::{thread::sleep, time::SystemTime};
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::*;

use super::physics::PhysicsState;

pub struct IntentError;

// Implement std::fmt::Display for AppError
impl fmt::Debug for IntentError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Could not register intent")
    }
}

struct Tick(u64);

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

pub struct RegionInstance {
    x: i32,
    y: i32,
    tick_rate_hz: i8,
    sleeping: bool,
    intent_sender: mpsc::UnboundedSender<Box<dyn EntityUpdate>>,
    intent_receiver: UnboundedReceiver<Box<dyn EntityUpdate>>,
    update_broadcast_sender: broadcast::Sender<Box<dyn Intent>>,
    world: World,
}

impl RegionInstance {
    pub fn new(x: i32, y: i32) -> Self {
        let (intent_sender, intent_receiver) = mpsc::unbounded_channel();
        // TODO: Make broadcast receiver configurable via env var
        //TODO: Is there a nicer way to ignore receiver?
        let (update_broadcast_sender, receiver) = broadcast::channel(1000);
        Self {
            x: x,
            y: y,
            tick_rate_hz: 60,
            sleeping: false,
            intent_sender: intent_sender,
            intent_receiver: intent_receiver,
            update_broadcast_sender: update_broadcast_sender,
            world: World::default(),
        }
    }

    pub fn process_updates(&mut self) {
        while let Ok(update) = self.intent_receiver.try_recv() {
            update.process_update(&mut self.world);
        }

        println!("successfully processed region x:{}, y:{}", self.x, self.y);
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
        self.update_broadcast_sender
            .send(Box::new(ClientIntent::new(Position::default())));
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
            Err(e) => Err(IntentError),
        }
    }

    pub fn run_simulation(&mut self, tick: u64) {
        let mut resources = Resources::default();
        
        let mut start_tick_time = SystemTime::now();
        let mut physics_state = PhysicsState::new(60);

        let elapsed_time_since_last_tick_secs =
            start_tick_time.elapsed().unwrap().as_secs_f32();

        // Reset time at the start of this current tick
        start_tick_time = SystemTime::now();

        // Set latest tick
        resources.insert(Tick(tick));

        // 1. Process all intents
        self.process_updates();

        // 2. Perform physics tick
        physics_state.tick();

        // Execute tick
        //schedule.execute(&mut self.world, &mut resources);

        println!(
            "Region x:{}, y:{} completed tick {} in total µs: {}",
            self.x,
            self.y,
            tick,
            start_tick_time.elapsed().unwrap().as_micros()
        );

       
    }
}

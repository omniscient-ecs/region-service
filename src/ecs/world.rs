
use std::sync::Mutex;
use tokio::{runtime::Builder, sync::mpsc::{self, UnboundedReceiver, UnboundedSender}};
use tokio::runtime::Runtime;
use tokio::{sync::oneshot::{self, Sender}};
use std::pin::Pin;
use tokio::sync::{Notify, RwLock};
use legion::{Entity, Resources, Schedule, World, storage::Component, storage::IntoComponentSource, system, systems::CommandBuffer};
use tokio::{task};
use std::{cell::Cell, collections::HashMap, rc::Rc, sync::Arc, time::Duration, time::SystemTime};
use std::cell::RefCell;
use std::any::Any;
use rapier3d::{dynamics::BodyStatus, dynamics::RigidBodyBuilder, na::Isometry3, na::Vector3, pipeline::EventHandler};
use rapier3d::dynamics::{JointSet, RigidBodySet, IntegrationParameters};
use rapier3d::geometry::{BroadPhase, NarrowPhase, ColliderSet};
use rapier3d::pipeline::PhysicsPipeline;
use std::thread::sleep;

struct Tick(u64);

struct MovementSystemConfig {
    always_send_update_on_tick: bool,
}

struct PhysicsState {
    config: MovementSystemConfig,
    pipeline: PhysicsPipeline,
    gravity: Vector3<f32>,
    integration_parameters: IntegrationParameters,
    broad_phase: BroadPhase,
    narrow_phase: NarrowPhase,
    bodies: RigidBodySet,
    colliders: ColliderSet,
    joints: JointSet,
}

impl PhysicsState {
    pub fn new(tick_rate_hz: u8) -> Self {
        let mut integration_parameters = IntegrationParameters::default();
        integration_parameters.set_dt(1f32 / tick_rate_hz as f32);

        Self {
            config: MovementSystemConfig {
                always_send_update_on_tick: true,
            },
            pipeline: PhysicsPipeline::new(),
            gravity: Vector3::new(0.0, -9.81, 0.0),
            integration_parameters: integration_parameters,
            broad_phase: BroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: JointSet::new(),
        }
    }
}


#[system]
fn region(#[state] region: &mut RegionInstance, cmd: &mut CommandBuffer) {
    region.process_updates(cmd);
}

#[system]
fn movement(#[state] physics_state: &mut PhysicsState) {
    println!("processing movement");

    physics_state.pipeline.step(
        &physics_state.gravity,
        &physics_state.integration_parameters,
        &mut physics_state.broad_phase,
        &mut physics_state.narrow_phase,
        &mut physics_state.bodies,
        &mut physics_state.colliders,
        &mut physics_state.joints,
        &(),
    );
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RigidBody {
    
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Position {
    position_x: f32,
    position_y: f32,
    position_z: f32,
    rotation_x: f32,
    rotation_y: f32,
    rotation_z: f32,
}


impl Position {
    pub fn default() -> Self {
        Self {
            position_x: 0f32,
            position_y: 0f32,
            position_z: 0f32,
            rotation_x: 0f32,
            rotation_y: 0f32,
            rotation_z: 0f32,
        }
    }
}

struct Velocity {
    linear_x: f32,
    linear_y: f32,
    linear_z: f32,
    angular_x: f32,
    angular_y: f32,
    angular_z: f32,
}


//TODO: Abstract into trait
struct NetworkedObserver {
    connection_id: String,
    //TODO: Handler
    //TODO: Socket
}

impl NetworkedObserver {
    fn new(connection_id: String) -> Self { 
        Self {
            connection_id: connection_id,
        }
    }
}


#[derive(Clone)]
struct WorldContext {
    // Connection Id that this context is referenced to
    connection_id: String,
    // Primary entity that this context drives.
    // Todo see if we can get away with not having a rwlock?
    entity_id: Entity,
    // Primary region that this context drives
    region: Arc<RwLock<RegionInstance>>,
}

impl WorldContext {
    fn new(region: RegionInstance, connection_id: String, entity_id: Entity) -> Self {
        Self {
            connection_id: connection_id,
            entity_id: entity_id,
            region: Arc::new(RwLock::new(region)),
        }
    }

    // Note - you cannot call this function until entity has been set! (May need to add thread safety and checking...)
    pub async fn submit_intent<T: Any + Sized + Send + Sync + Copy>(&self, component: T) {
        let region = self.region.read().await;
        region.upsert_component(self.entity_id, component);
    }
}

pub struct WorldInstance {
    id: String,
    regions: Vec<Vec<RegionInstance>>,
    region_width_x: i32,
    region_width_y: i32,
    
}

impl WorldInstance {
    pub fn new(id: String, width_x: i32, width_y: i32, tick_rate_hz: u8) -> WorldInstance {
        let mut regions = vec![vec![RegionInstance::new(); width_y as usize]; width_x as usize];
        
        let mut x = 0;
        for region_x_iterator in regions.iter_mut() {
            let mut y = 0;
            for region in region_x_iterator.iter_mut() {
                region.x = x as i32;
                region.y = y as i32;
                y += 1;
            }
            x += 1;
        }
        
        WorldInstance {
            id: id,
            regions: regions,
            region_width_x: width_x,
            region_width_y: width_y,
        }
    }

    // This async func can take some time!
    pub async fn create_context(&self, region_x: i32, region_y:i32, observer: NetworkedObserver) -> WorldContext {
        let region = self.regions[region_x as usize][region_y as usize].clone();
        let cloned_region = region.clone();
        let connection_id = observer.connection_id.to_owned();
        //TODO: Register surrounding regions
        let entity_id = region.register_observer(observer).await;
        WorldContext::new(cloned_region, connection_id, entity_id.unwrap())
    }

    pub fn run(&self) {
        self.run_simulation(u64::MAX);
    }

    pub fn run_simulation(&self, simulate_tick_count: u64) {
        let region_x_itr = self.regions.to_owned();
        for region_x_iterator in region_x_itr {
            for region in region_x_iterator {
                task::spawn_blocking(move || {
                    println!("preparing to run region {} {}", region.x, region.y);

                    let schedule = Rc::new(RefCell::new(Schedule::builder()
                        .add_system(region_system(region.clone()))
                        .add_system(movement_system(PhysicsState::new(60)))
                        .flush()
                        .build()));

                    region.run_simulation(schedule, simulate_tick_count);
                });
            }
        }
    }
}

//TODO: Create system that moves vec items 


trait EntityUpdate : Send + Sync {
    fn process_update(self: Box<Self>, cmd: &mut CommandBuffer);
}

struct UpdateComponent<T> where T : Any + Sized + Send + Sync + Copy   {
    entity_id: Entity,
    component: T,
}

impl<T: Component + Copy> EntityUpdate for UpdateComponent<T> {
    fn process_update(self: Box<Self>, cmd: &mut CommandBuffer) {
        cmd.add_component(self.entity_id, self.component);
    }
}

impl<T: Any + Sized + Send + Sync + Copy> UpdateComponent<T> {
    fn new(entity_id: Entity, component: T) -> Self {
        Self {
            entity_id: entity_id,
            component: component,
        }
    }
}

struct RegisterContext {
    on_registered_channel: Sender<Entity>,
    connection_id: String,
}

impl RegisterContext {
    fn new(on_ready: Sender<Entity>, connection_id: String) -> Self {
        Self {
            on_registered_channel: on_ready,
            connection_id: connection_id,
        }
    }
}

impl EntityUpdate for RegisterContext {
    fn process_update(self: Box<Self>, cmd: &mut CommandBuffer) {
        let connection_id = self.connection_id.to_owned();
        //TODO: Figure out how to get the lock?
        let entity_id = cmd.push((connection_id,));
        
        self.on_registered_channel.send(entity_id);        
    }
}


#[derive(Clone)]
struct RegionInstance {
    x: i32,
    y: i32,
    tick_rate_hz: i8,
    sleeping: bool,
    events_sender: Arc<UnboundedSender<Box<dyn EntityUpdate>>>,
    events_receiver: Arc<Mutex<UnboundedReceiver<Box<dyn EntityUpdate>>>>,
    // Observers to broadcast updates each tick
    observers: Arc<RwLock<HashMap<String, NetworkedObserver>>>,
}


impl RegionInstance {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            x: 0,
            y: 0,
            tick_rate_hz: 60,
            sleeping: false,
            events_sender: Arc::new(tx),
            events_receiver: Arc::new(Mutex::new(rx)),
            observers: Arc::new(RwLock::new(HashMap::new()))
        }
    }

    fn process_updates(&mut self, cmd: &mut CommandBuffer) {
        let mut events_receiver = self.events_receiver.lock().unwrap();
        while let Ok(update) = events_receiver.try_recv() {
            update.process_update(cmd);
        }

        println!("successfully processed region x:{}, y:{}", self.x, self.y);
    }

    async fn register_observer(self, observer: NetworkedObserver) -> Option<Entity> {
        let (channel_sender, channel_receiver) = oneshot::channel();
        let region = self.clone();
        let connection_id = observer.connection_id.to_owned();
        let register = RegisterContext::new( channel_sender, connection_id);

        self.events_sender.send(Box::new(register));

        let mut observers = region.observers.write().await;
        observers.insert(observer.connection_id.to_owned(), observer);

        match channel_receiver.await {
            Ok(e) => Some(e),
            Err(e) => {
                println!("error creating entity {}", e);
                None
            },
        }
    }

    pub async fn upsert_component<T: Any + Sized + Send + Sync + Copy>(&self, entity_id: Entity, component: T) {
        self.events_sender.send(Box::new(UpdateComponent::new(entity_id, component)));
    }

    fn run(&self, ecs_schedule: Rc<RefCell<Schedule>>) {
        self.run_simulation(ecs_schedule, u64::MAX);
    }

    fn run_simulation(&self, ecs_schedule: Rc<RefCell<Schedule>>, simulate_tick_count: u64) {
        let mut world = World::default();
        let mut resources = Resources::default();
        // 60 hz = one tick every 16.67 ms or .0167 seconds
        let tick_rate_seconds = 1f32 / self.tick_rate_hz as f32;
        let mut start_tick_time = SystemTime::now();
        let mut tick = 1u64;

        loop {
            let elapsed_time_since_last_tick_secs = start_tick_time.elapsed().unwrap().as_secs_f32();

            // Sleep until next tick
            // TODO: Check if elapsedTime > tick rate seconds, which means the system is falling behind!
            if elapsed_time_since_last_tick_secs < tick_rate_seconds {
                sleep(Duration::from_secs_f32(tick_rate_seconds - elapsed_time_since_last_tick_secs));
            } else {
                println!("tick rate is lagging and behind by {} seconds", elapsed_time_since_last_tick_secs-tick_rate_seconds)
            }
            
            // Reset time at the start of this current tick
            start_tick_time = SystemTime::now();

            // Set latest tick
            resources.insert(Tick(tick));

            // Execute tick
            ecs_schedule.borrow_mut().execute(&mut world, &mut resources);

            println!("Region x:{}, y:{} completed tick {} in total µs: {}",
                self.x,
                self.y,
                tick,
                start_tick_time.elapsed().unwrap().as_micros());


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
        }
    }
}

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

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
        let observer = NetworkedObserver::new(String::from(connection_id));
        let context = world.create_context(0, 0, observer).await;
        println!("created world context connection_id: {}, entity_id: {:?}", context.connection_id, context.entity_id);

        assert_eq!(connection_id, context.connection_id);
    }
    
    #[bench]
    fn run_world_test(b: &mut Bencher) {
       
    }
}
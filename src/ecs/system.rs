use legion::Entity;
use legion::systems::CommandBuffer;
use legion::{systems::Builder, Schedule, system};

use crate::protos::components::{MoveIntent, Velocity};

use super::{region::DeltaTime, physics::PhysicsState};



//TODO: Figure out generic way to pass intents to 3rd party systems otherwise configure them via config file.
pub fn new_schedule() -> Schedule {
    Schedule::builder()
         // Process all intents first
        .add_system(move_intent_system())
        .flush()
        // Process all ECS systems
        .add_system(physics_system(PhysicsState::new()))
        .build()
}

// Add all intent systems

fn add_intent_systems(builder: &mut Builder) -> &mut Builder{
    builder
       
}

#[system(for_each)]
fn move_intent(entity: &Entity, move_intenet: &mut MoveIntent, velocity: &mut Velocity, cmd: &mut CommandBuffer) {
    //TODO: Update velocity correcty
    if move_intenet.forward {
        velocity.set_velocity_x(1f32);
    }
    cmd.remove_component::<MoveIntent>(*entity);
}

#[system]
fn physics(#[state] physics_state: &mut PhysicsState, #[resource] dt: &DeltaTime) {
    physics_state.process_tick(dt.time_since_last_tick_secs())
}

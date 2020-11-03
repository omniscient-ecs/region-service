use legion::{system, systems::CommandBuffer, Schedule};

use crate::ecs::region::RegionInstance;

use super::physics::PhysicsState;

pub(crate) fn new_schedule() -> Schedule {
    Schedule::builder().build()
}

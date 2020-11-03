use rapier3d::dynamics::JointSet;
use rapier3d::geometry::NarrowPhase;
use rapier3d::{dynamics::IntegrationParameters, na::Vector3, pipeline::PhysicsPipeline};
use rapier3d::{
    dynamics::RigidBodySet,
    geometry::{BroadPhase, ColliderSet},
};

pub(crate) struct MovementSystemConfig {
    always_send_update_on_tick: bool,
}

pub(crate) struct PhysicsState {
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

    pub fn tick(&mut self) {
        self.pipeline.step(
            &self.gravity,
            &self.integration_parameters,
            &mut self.broad_phase,
            &mut self.narrow_phase,
            &mut self.bodies,
            &mut self.colliders,
            &mut self.joints,
            &(),
        );
    }
}

use rapier3d::{
    na::Vector3,
    pipeline::PhysicsPipeline,
    dynamics::{IntegrationParameters, RigidBodySet, JointSet},
    geometry::{BroadPhase, ColliderSet, NarrowPhase},
};

pub struct PhysicsState {
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
    pub fn new() -> Self {
        Self {
            pipeline: PhysicsPipeline::new(),
            gravity: Vector3::new(0.0, -9.81, 0.0),
            integration_parameters: IntegrationParameters::default(),
            broad_phase: BroadPhase::new(),
            narrow_phase: NarrowPhase::new(),
            bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            joints: JointSet::new(),
        }
    }

    pub fn process_tick(&mut self, time_since_last_tick_secs: f32) {
        self.integration_parameters
            .set_dt(time_since_last_tick_secs);
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

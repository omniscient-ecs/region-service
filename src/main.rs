#![feature(test)]
#![feature(unsized_locals)]
#![feature(slice_iter_mut_as_slice)]

mod ecs;
mod protos;

#[tokio::main]
async fn main() {
    let mut world = ecs::world::WorldInstance::new(String::from("1"), 1, 5, 100, 100, 60);
    world.run();
}

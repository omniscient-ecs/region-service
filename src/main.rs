#![feature(test)]
#![feature(unsized_locals)]

mod ecs;
mod protos;

#[tokio::main]
async fn main() {
    let mut world = ecs::world::WorldInstance::new(String::from("1"), 1, 5, 100, 100, 60);
    world.run();
}

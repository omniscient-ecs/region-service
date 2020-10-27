
#![feature(test)]
#![feature(unsized_locals)]

mod ecs;

#[tokio::main]
async fn main() {
    let mut world = ecs::world::WorldInstance::new(String::from("1"), 1, 5, 60);
    world.run();

    loop { 
    }
}
#[path = "camera_follow/game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("Move with WASD or arrow keys. The camera follows smoothly.");
    application.run(initial_world)?;
    Ok(())
}

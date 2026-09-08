#[path = "game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("Use WASD or the arrow keys to accelerate the blue body.");
    application.run(initial_world)?;
    Ok(())
}

#[path = "persistent_score/game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("Press Space to add points, then Enter to replace the World.");
    application.run(initial_world)?;
    Ok(())
}

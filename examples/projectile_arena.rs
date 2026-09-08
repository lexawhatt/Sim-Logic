#[path = "projectile_arena/game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) =
        game::build_application(sim_logic::prelude::TimeConfig::default())?;
    println!("Move with WASD or arrow keys. Press Space to fire at the red target.");
    application.run(initial_world)?;
    Ok(())
}

#[path = "coin_pickup/game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("Move with WASD or arrow keys and collect the yellow coins.");
    application.run(initial_world)?;
    Ok(())
}

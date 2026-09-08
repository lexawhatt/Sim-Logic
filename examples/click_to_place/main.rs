#[path = "game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application(Default::default())?;
    println!("Left-click to place a circle; right-click to clear. Escape exits.");
    println!("The static camera maps each click using that click's logical viewport.");
    println!(
        "At most {} circles can be placed at once.",
        game::MAX_MARKERS
    );
    application.run(initial_world)?;
    Ok(())
}

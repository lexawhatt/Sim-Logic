#[path = "game.rs"]
mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("The camera follows the blue body; the status panel stays on screen.");
    println!("Space pauses/resumes motion. Green means running; amber means paused.");
    println!(
        "The blue bar tracks simulation progress; the small light keeps pulsing while paused."
    );
    println!("Resize to reposition the panel. Escape exits.");
    application.run(initial_world)?;
    Ok(())
}

mod game;

fn main() -> sim_logic::LogicResult {
    let (application, initial_world) = game::build_application()?;
    println!("Move with WASD or arrow keys; the room walls block the player.");
    let report = application.run(initial_world)?;
    println!("closed after {} logical frames", report.logic_frames());
    Ok(())
}

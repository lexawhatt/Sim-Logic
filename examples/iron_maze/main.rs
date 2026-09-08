#[path = "mod.rs"]
mod iron_maze;

use sim_logic::prelude::*;

fn main() -> LogicResult {
    println!("IRON MAZE - a Sim;Logic raycasting game");
    println!("WASD: move | Left/Right: turn | Space/LMB: fire | Enter: restart | Escape: exit");
    println!("Eliminate six sentries, collect supplies, and reach the exit beacon.");
    let (application, world) = iron_maze::build_application()?;
    application.run_desktop(
        world,
        DesktopConfig::new("IRON MAZE | Sim;Logic", 1280.0, 720.0)?,
    )?;
    Ok(())
}

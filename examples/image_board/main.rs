#[path = "game.rs"]
mod game;

use sim_logic::prelude::*;

fn main() -> LogicResult {
    println!("Image board: one original 8x8 RGBA asset shared by five screen images.");
    println!(
        "Top row, left to right: nearest sampling, linear sampling, cropped upper half, blue tint."
    );
    println!("The pale stripe covers the first image and passes behind the second.");
    println!("WASD/arrows move the bottom image; Space toggles its upper-half crop.");
    println!("Enter opens the alternate World with the same asset; Escape exits.");
    let (application, initial) = game::build_application()?;
    application.run_desktop(
        initial,
        DesktopConfig::new(
            "Image board | WASD/arrows move | Space crop | Enter alternate World | Esc exit",
            1280.0,
            720.0,
        )?,
    )?;
    Ok(())
}

#[path = "mod.rs"]
mod territory_wars;

use sim_logic::prelude::*;

fn main() -> LogicResult {
    let mut seed = territory_wars::DEFAULT_SEED;
    let mut demo = false;
    let mut debug = false;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--demo" => demo = true,
            "--debug" => {
                demo = true;
                debug = true;
            }
            "--seed" => {
                seed = arguments
                    .next()
                    .ok_or("--seed needs an unsigned integer")?
                    .parse()?
            }
            "--help" | "-h" => {
                println!(
                    "Options: --demo starts from a prepared battle; --debug also opens the inspector."
                );
                println!("MMB drag pans the map; wheel zooms at the cursor; V resets the view.");
                println!(
                    "Frontier: offline territory conquest on Sim;Logic.\nUsage: territory_wars [--seed INTEGER] [--demo | --debug] [--help]\nClick land to start; click neutral or a bordering rival to send troops.\nSpace expands; slider/arrows/1-5 choose percentage; P pauses; R restarts; N changes map.\nF3 mechanics inspector. F4 arms cheats: F5 troops, F6 AI orders, F8 win.\nF9 steps one tick while paused. Escape closes the window."
                );
                return Ok(());
            }
            _ => {
                return Err(
                    "usage: territory_wars [--seed INTEGER] [--demo | --debug] [--help]".into(),
                );
            }
        }
    }
    let (app, world) = if demo {
        territory_wars::app::build_demo(seed, debug)?
    } else {
        territory_wars::build_application(seed)?
    };
    println!(
        "FRONTIER | seed {seed} | Click land to start. F3: inspector. F4: cheats. Escape: exit."
    );
    println!("MMB drag: pan. Wheel: zoom at cursor. V: reset view. HUD stays fixed.");
    app.run_desktop(
        world,
        DesktopConfig::new("FRONTIER | Sim;Logic", 1440.0, 900.0)?,
    )?;
    Ok(())
}

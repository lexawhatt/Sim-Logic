mod scene;

fn main() -> sim_logic::LogicResult {
    let (app, world) = scene::build_application()?;
    println!("Text labels: Space pauses the counter; Enter changes World; Escape exits.");
    println!(
        "The example embeds DejaVu Sans; redistribution terms: tests/assets/text/LICENSE-DejaVu.txt"
    );
    app.run_desktop(
        world,
        sim_logic::desktop::DesktopConfig::new("Sim;Logic | Real text", 1280.0, 720.0)?,
    )?;
    Ok(())
}

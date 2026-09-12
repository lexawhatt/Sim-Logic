mod app;
mod scene;
mod view;

fn main() -> sim_logic::LogicResult {
    let (application, initial) = app::build_application(Default::default())?;
    println!("Click and release over a button. Drag out or lose focus to cancel.");
    println!("D toggles COUNT, including while held; P pauses; R clears dots; Escape exits.");
    println!("Labels use the existing DejaVu font; license: tests/assets/text/LICENSE-DejaVu.txt");
    application.run_desktop(
        initial,
        sim_logic::desktop::DesktopConfig::new("Sim;Logic | Pointer buttons", 1000.0, 700.0)?,
    )?;
    Ok(())
}

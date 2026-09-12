mod app;
mod model;
mod presentation;
mod projection;
mod scene;
mod view;

fn main() -> sim_logic::LogicResult {
    let mut args = std::env::args_os().skip(1);
    let save_path = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "voxel-sandbox.save".into());
    if args.next().is_some() {
        return Err("usage: voxel_sandbox [save-file]".into());
    }
    let (application, initial) = app::build_application(Default::default(), save_path)?;
    println!("Twin Fields: WASD move; arrows look; Space jumps; middle-drag also looks.");
    println!("Aim at the crosshair. Left click breaks; right click places; 1-5 select blocks.");
    println!("N travels; P pauses; F5 saves; F9 loads; Escape exits. UI buttons also work.");
    println!("Finite original sandbox, two persistent regions. Saving occurs only on request.");
    application.run_desktop(
        initial,
        sim_logic::desktop::DesktopConfig::new("Twin Fields | Sim;Logic", 1100.0, 720.0)?,
    )?;
    Ok(())
}

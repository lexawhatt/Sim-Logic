mod acceptance;
mod app;
mod materials;
mod model;
mod presentation;
mod projection;
mod scene;
mod showcase;
mod view;

fn main() -> sim_logic::LogicResult {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next();
    let test_drive = argument.as_deref() == Some(std::ffi::OsStr::new("--test-drive"));
    let save_path = if test_drive {
        "voxel-sandbox.save".into()
    } else {
        argument
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "voxel-sandbox.save".into())
    };
    if args.next().is_some() {
        return Err("usage: voxel_sandbox [save-file | --test-drive]".into());
    }
    let (mut application, initial) = app::build_application(Default::default(), save_path)?;
    println!("Twin Fields: WASD move; arrows look; Space jumps; middle-drag also looks.");
    println!("Aim at the crosshair. Left click breaks; right click places; 1-5 select blocks.");
    println!("N travels; P pauses; F5 saves; F9 loads; Escape exits. UI buttons also work.");
    println!("L lighting; F fog; M mipmaps; T patches the gray study board; V projection.");
    println!("Study panels are visual only, without collisions or saved block identities.");
    println!("Finite original sandbox, two persistent regions. Saving occurs only on request.");
    let mut config =
        sim_logic::desktop::DesktopConfig::new("Twin Fields | Sim;Logic", 1100.0, 720.0)?;
    if test_drive {
        acceptance::configure(&mut application)?;
        config.set_gpu_timing(true);
    }
    let report = application.run_desktop(initial, config)?;
    if test_drive {
        acceptance::verify(&report)?;
    }
    Ok(())
}

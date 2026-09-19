//! Small stock-host proving UI for Sim;X's integration requests.
mod scene;
use sim_logic::prelude::*;

fn main() -> LogicResult {
    let mut frames = None;
    let mut exercise_window = false;
    let mut fullscreen = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => {
                let count: u64 = args
                    .next()
                    .ok_or("--frames requires a count >= 120")?
                    .parse()?;
                if count < 120 {
                    return Err("--frames requires a count >= 120".into());
                }
                frames = Some(count);
            }
            "--exercise-window" => exercise_window = true,
            "--fullscreen" => fullscreen = true,
            _ => return Err(format!("unknown argument: {arg}").into()),
        }
    }
    let (app, initial) = scene::build(frames, exercise_window, fullscreen)?;
    let mut desktop = DesktopConfig::new("Sim;Logic | UI integration", 1000.0, 700.0)?;
    if fullscreen {
        desktop.set_window_mode(WindowMode::BorderlessFullscreen(
            FullscreenMonitor::Automatic,
        ));
    }
    println!(
        "Tab / Shift-Tab: focus; Enter / click: activate; P: pause; F11: fullscreen; Escape: exit."
    );
    println!(
        "Animation stays in FrameUpdate while fixed simulation is paused. Hover changes the native cursor."
    );
    let report = app.run_desktop(initial, desktop)?;
    println!(
        "UI integration: logic={} presented={} skipped={}",
        report.logic_frames(),
        report.drawn_frames(),
        report.skipped_frames()
    );
    if let Some(frames) = frames
        && (report.exit_reason() != DesktopExitReason::ApplicationRequested
            || report.drawn_frames() < frames - 1
            || report.skipped_frames() != 0)
    {
        return Err("bounded UI run did not confirm every expected presentation".into());
    }
    Ok(())
}

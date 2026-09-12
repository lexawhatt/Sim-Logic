//! Bounded in-game Engine 0.4 smoke drive. No user saves or synthetic OS input.

use super::{
    app::{Action, Routes, Session},
    materials::Settings,
    model::{Player, SaveGame},
    scene::{Local, Phase},
};
use sim_logic::prelude::*;

struct Drive {
    frame: usize,
    ready: usize,
    visits: usize,
}

pub fn configure(app: &mut Application<Action>) -> LogicResult {
    app.register_app_resource(Drive {
        frame: 0,
        ready: 0,
        visits: 0,
    })?;
    // Registered last: poses override gameplay presentation only for this drive.
    app.add_fallible_frame_system(step);
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "bounded example acceptance schedule"
)]
fn step(
    mut drive: AppResMut<Drive>,
    mut session: AppResMut<Session>,
    routes: AppRes<Routes>,
    local: Res<Local>,
    mut settings: ResMut<Settings>,
    mut view: ResMut<View3d>,
    mut commands: Commands,
) -> LogicResult {
    drive.frame += 1;
    if drive.frame == 1 {
        commands.set_paused(true)?;
        settings.studies = true;
    }
    if drive.frame > 400 {
        return Err("Engine 0.4 drive exceeded its bounded loading allowance".into());
    }
    if local.phase != Phase::Ready {
        return Ok(());
    }
    if drive.ready == 128 {
        if session.game.active_region().pending_chunks() != 0 {
            return Err("streamed destination did not finish generating during acceptance".into());
        }
        if drive.visits == 0 {
            let token = commands.new_transition_intent()?;
            commands.replace_world(token, routes.0[local.region.other().index()])?;
            drive.visits = 1;
            drive.ready = 0;
        } else {
            // Verify the canonical two-region save independently of GPU resources.
            let bytes = session.game.encode()?;
            let loaded = SaveGame::decode(&bytes)?;
            if loaded.encode()? != bytes {
                return Err("save round-trip changed canonical state".into());
            }
            println!(
                "voxel drive: 256 camera poses, two streamed regions, {} edits, in-memory save round-trip",
                session.edits
            );
            commands.request_exit()?;
        }
        return Ok(());
    }
    let pose = drive.ready;
    if pose == 0 {
        // Settings are world-local; enable the render probes after each entry.
        settings.studies = true;
    }
    if pose == 96 {
        // Exercise actual cache eviction and source retirement, not just a
        // camera moving independently of the canonical streaming center.
        session.game.player = Player::at(if drive.visits == 0 {
            [-28.5, 12.0, -28.5]
        } else {
            [80.5, 12.0, 80.5]
        });
        session.game.player.pitch = -0.65;
    }
    // Includes near-plane crossings, grazing axes and viewpoints inside terrain.
    // This is a real Native surface traversal, not CPU-side clipping emulation.
    let angle = pose as f32 * std::f32::consts::TAU / 24.0;
    let eye = Vec3::new(
        12.0 + angle.sin() * 9.0,
        2.0 + (pose % 9) as f32,
        12.0 + angle.cos() * 9.0,
    )?;
    view.set_pose(eye, Vec3::new(12.125, 4.125, 12.125)?)?;
    if pose >= 96 {
        let player = session.game.player;
        let [x, y, z] = player.eye();
        view.set_pose(Vec3::new(x, y, z)?, Vec3::new(x + 3.0, y - 6.0, z - 8.0)?)?;
    }
    if pose == 14 {
        // Exact camera from the original Engine 0.3 projected-orientation crash.
        let yaw = 14_f32 * 0.13;
        view.set_pose(
            Vec3::new(11.75, 7.58, 11.0)?,
            Vec3::new(11.75 + yaw.sin(), 7.18, 11.0 - yaw.cos())?,
        )?;
    }
    if pose == 8 || pose == 12 {
        break_one(&mut session)?;
    }
    if matches!(pose, 20 | 24 | 28) {
        settings.patch_requests = settings.patch_requests.saturating_add(1);
    }
    if matches!(pose, 32 | 40) {
        settings.lighting = !settings.lighting;
    }
    if matches!(pose, 44 | 52) {
        settings.fog = !settings.fog;
    }
    if matches!(pose, 56 | 64) {
        settings.mipmaps = !settings.mipmaps;
    }
    if matches!(pose, 68 | 80) {
        settings.orthographic = !settings.orthographic;
    }
    drive.ready += 1;
    Ok(())
}

fn break_one(session: &mut Session) -> LogicResult {
    let original = session.game.player;
    // Fixed finite search for a legal downward hit, excluding immutable bedrock.
    let mut edited = false;
    'search: for x in [10.5, 12.5, 14.5] {
        for y in (3..12).rev() {
            session.game.player = Player::at([x, y as f32, 10.5]);
            session.game.player.pitch = -1.45;
            if session.game.break_target().is_ok() {
                edited = true;
                break 'search;
            }
        }
    }
    session.game.player = original;
    if !edited {
        return Err("Engine 0.4 terrain edit probe found no legal block".into());
    }
    session.edits = session.edits.saturating_add(1);
    Ok(())
}

pub fn verify(report: &DesktopRunReport) -> LogicResult {
    if report.exit_reason() != DesktopExitReason::ApplicationRequested
        || report.committed_transitions() != 1
        || report.drawn_frames() < 258
        || report.skipped_frames() != 0
    {
        return Err(format!(
            "Engine 0.4 drive incomplete: {} presents, {} skips, {} transitions, {:?}",
            report.drawn_frames(),
            report.skipped_frames(),
            report.committed_transitions(),
            report.exit_reason()
        )
        .into());
    }
    let updates = report.three_d_updates();
    if updates.mesh_updates == 0 || updates.texture_updates < 6 {
        return Err(format!("Engine 0.4 update paths not exercised: {updates:?}").into());
    }
    println!(
        "Engine 0.4 drive PASS: {} presents, updates={updates:?}",
        report.drawn_frames()
    );
    let timings = report.gpu_timings();
    println!("GPU status/counters: {:?}", timings.statistics());
    // Print only initialized completed samples, never fixed-array placeholders.
    for sample in timings.samples() {
        println!("GPU completed pass: {sample:?}");
    }
    println!(
        "GPU correlation: unmatched={}, evicted={}",
        timings.unmatched_samples(),
        timings.evicted_correlations()
    );
    Ok(())
}

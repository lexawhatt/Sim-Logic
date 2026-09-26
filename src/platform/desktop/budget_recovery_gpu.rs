//! Real stock-host admission/recovery, not manual feedback injection.
use crate::{desktop::*, prelude::*};
use std::time::{Duration, Instant};
use winit::platform::wayland::EventLoopBuilderExtWayland;

#[derive(Resource)]
struct Progress {
    phase: u8,
    started: Instant,
}

fn exercise(
    mut progress: ResMut<Progress>,
    feedback: AppRes<PresentationFeedback>,
    rectangles: Query<(LogicEntityRef, &ScreenRectangleVisual)>,
    mut images: Query<&mut ScreenImageVisual>,
    mut commands: Commands,
) -> LogicResult {
    if progress.started.elapsed() > Duration::from_secs(15) {
        return Err("native budget acceptance timed out".into());
    }
    match progress.phase {
        0 | 1 => (),
        2 => {
            commands.spawn(rectangle(20.0)?)?;
        }
        3 => {
            let rejected = feedback.last_rejection();
            assert_eq!(
                rejected.unwrap().stage,
                PresentationBudgetStage::ScreenScene
            );
            for (entity, visual) in &rectangles {
                if visual.position().to_vec2().x() == 20.0 {
                    commands.despawn(entity.handle())?;
                }
            }
        }
        4 => {
            if feedback.last_rejection().is_some() {
                return Ok(()); // Surface skips do not count as successful recovery.
            }
            for mut image in &mut images {
                image.set_clip(ScreenClip::Unclipped);
            }
        }
        5 => {
            let rejected = feedback.last_rejection().unwrap();
            assert_eq!(rejected.stage, PresentationBudgetStage::Composition);
            assert_eq!(rejected.resource, PresentationBudgetResource::TextureBytes);
            for mut image in &mut images {
                image.set_clip(ScreenClip::Empty);
            }
        }
        _ => {
            if feedback.last_rejection().is_none() {
                assert_eq!(feedback.rejected_frames(), 2);
                commands.request_exit()?;
            }
        }
    }
    progress.phase = progress.phase.saturating_add(1);
    Ok(())
}

fn rectangle(x: f32) -> LogicResult<ScreenRectangleVisual> {
    Ok(ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, 2.0),
        LogicalScreenVector::new(8.0, 8.0),
        Color::WHITE,
    )?)
}

fn run(opt_in: bool) -> LogicResult<DesktopHost<u8>> {
    let mut config = AppConfig::default();
    let frame = FrameLimits::default();
    let limits = RenderLimits::default()
        .with_max_screen_images(1)
        .with_screen_scene_budget(SceneBudget::new(
            1, 0, 1000, 100_000, 100_000, 100_000, 1000,
        ))
        .with_frame_limits(FrameLimits::new(
            16,
            frame.max_commands(),
            frame.max_vertices(),
            frame.max_upload_bytes(),
            0, // An otherwise valid image must be rejected by the compositor.
            frame.max_draw_calls(),
        ));
    config.set_render_limits(limits);
    let mut app = Application::<u8>::new(config)?;
    if opt_in {
        app.register_app_resource(PresentationFeedback::default())?;
        app.add_fallible_frame_system(exercise);
    } else {
        app.add_fallible_frame_system(
            |mut progress: ResMut<Progress>, mut commands: Commands| -> LogicResult {
                if progress.phase == 2 {
                    commands.spawn(rectangle(20.0)?)?;
                }
                progress.phase += 1;
                Ok(())
            },
        );
    }
    let camera = ActiveCamera2d::centered(1.0)?;
    let rectangle = rectangle(2.0)?;
    let asset = app.register_image_rgba8(1, 1, &[255; 4])?;
    let mut image = ScreenImageVisual::new(
        asset,
        LogicalScreenPosition::new(30.0, 2.0),
        LogicalScreenVector::new(8.0, 8.0),
    )?;
    image.set_clip(ScreenClip::Empty);
    let initial = app.register_world("native budget acceptance", move |world| {
        world.spawn(camera)?;
        world.spawn(rectangle)?;
        world.spawn(image)?;
        world.insert_resource(Progress {
            phase: 0,
            started: Instant::now(),
        })?;
        Ok(())
    })?;
    let mut host = DesktopHost::new(
        app.build_headless(initial)?,
        DesktopConfig::new("Sim Logic budget acceptance", 64.0, 64.0)?,
        64,
        frame_budget(limits.frame_limits()),
        limits.three_d(),
    );
    let event_loop = EventLoop::builder().with_any_thread(true).build()?;
    event_loop.run_app(&mut host)?;
    println!(
        "budget acceptance opt_in={opt_in} drawn={} rejected={} logic={} adapter={:?}",
        host.report.drawn_frames(),
        host.report.budget_rejections(),
        host.report.logic_frames(),
        host.renderer.as_ref().map(|r| r.diagnostics())
    );
    Ok(host)
}

#[test]
#[ignore = "requires an isolated Linux display and Vulkan; run alone"]
fn native_budget_recovery_keeps_running_and_presents_again() -> LogicResult {
    let host = run(true)?;
    assert!(host.fatal.is_none(), "{:?}", host.fatal);
    assert_eq!(host.report.budget_rejections(), 2);
    assert!(host.report.drawn_frames() >= 4);
    assert!(host.report.last_screen_extraction().is_some());
    assert!(
        host.runner
            .app_resource::<PresentationFeedback>()
            .unwrap()
            .last_rejection()
            .is_none()
    );
    Ok(())
}

#[test]
#[ignore = "requires an isolated Linux display and Vulkan; run alone"]
fn native_budget_failure_remains_fatal_without_opt_in() -> LogicResult {
    let host = run(false)?;
    assert!(matches!(
        host.fatal,
        Some(DesktopRunError::LogicFrame { .. })
    ));
    assert_eq!(host.report.budget_rejections(), 0);
    assert_eq!(host.report.logic_frames(), 3);
    assert!(host.report.drawn_frames() > 0);
    Ok(())
}

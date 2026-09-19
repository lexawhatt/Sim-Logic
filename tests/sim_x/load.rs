//! Match the reported decorative source counts, without a timing/FPS assertion.
use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

#[test]
fn reported_picker_scale_runs_in_frame_update_with_fixed_time_paused() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_lines(963)
            .with_max_screen_circles(57)
            .with_screen_scene_budget(sim_engine::SceneBudget::new(
                1020,
                0,
                500_000,
                4 * 1024 * 1024,
                8 * 1024 * 1024,
                64 * 1024 * 1024,
                1020,
            )),
    );
    let mut app = Application::<Action>::new(config)?;
    app.add_fallible_frame_system(
        |viewport: FrameViewport, mut lines: Query<&mut ScreenLineVisual>| -> LogicResult {
            for mut line in &mut lines {
                line.set_endpoints(
                    LogicalScreenPosition::new(10.0, 10.0),
                    LogicalScreenPosition::new(viewport.logical().width() - 10.0, 20.0),
                )?;
            }
            Ok(())
        },
    );
    let line = ScreenLineVisual::new(
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenPosition::new(80.0, 20.0),
        1.0,
        Color::WHITE,
    )?;
    let circle =
        ScreenCircleVisual::new(LogicalScreenPosition::new(20.0, 30.0), 5.0, Color::WHITE)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("picker-scale", move |world| {
        world.spawn(camera)?;
        for _ in 0..963 {
            world.spawn(line)?;
        }
        for _ in 0..57 {
            world.spawn(circle)?;
        }
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    runner.set_paused(true);
    for width in [1280.0, 800.0, 1920.0] {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(100),
            &[],
            LogicalViewport::new(width, 800.0)?,
        )) else {
            return Err("rejected frame".into());
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
        assert_eq!(report.fixed_ticks_attempted(), 0);
        let snapshot = runner.extracted_frame().ok_or("snapshot")?;
        assert_eq!(
            snapshot.screen_draws(),
            &[ScreenDraw::Primitives { run: 0 }]
        );
        let mut counts = [0, 0];
        for primitive in snapshot.screen_primitive_run_records(0).ok_or("run")? {
            match primitive {
                ResolvedScreenPrimitive::Line { visual, .. } => {
                    counts[0] += 1;
                    assert_eq!(visual.end().to_vec2().x(), width - 10.0);
                }
                ResolvedScreenPrimitive::Circle { .. } => counts[1] += 1,
                _ => return Err("unexpected primitive".into()),
            }
        }
        assert_eq!(counts, [963, 57]);
    }
    Ok(())
}

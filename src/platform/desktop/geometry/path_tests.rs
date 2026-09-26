//! Exercise paths through the stock compositor, not just resource construction.
use super::*;
use crate::desktop::images::{self, DesktopImages};
use sim_engine::{FrameBudget, RenderStatus};

pub(super) fn probe(renderer: &mut WgpuRenderer) -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_fallible_frame_system(
        |mut step: ResMut<Step>, mut paths: Query<&mut ScreenPolylineVisual>| -> LogicResult {
            for mut path in &mut paths {
                match step.0 {
                    0 => {
                        let style = path.style().with_cap(StrokeCap2d::Square);
                        path.set_style(style)?;
                    }
                    1 => path.set_points(&[
                        LogicalScreenPosition::new(4.25, 5.5),
                        LogicalScreenPosition::new(30.25, 10.5),
                        LogicalScreenPosition::new(45.25, 40.5),
                    ])?,
                    2 => path.set_clip(ScreenClip::Empty),
                    3 => path.set_clip(ScreenClip::new(
                        LogicalScreenPosition::new(8.25, 2.5),
                        LogicalScreenVector::new(28.5, 28.25),
                    )?),
                    _ => (),
                }
            }
            step.0 += 1;
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let path = ScreenPolylineVisual::new(
        &[
            LogicalScreenPosition::new(4.25, 5.5),
            LogicalScreenPosition::new(30.25, 10.5),
            LogicalScreenPosition::new(40.25, 35.5),
        ],
        StrokeStyle2d::new(2.0, Color::WHITE).with_join(StrokeJoin2d::Round),
    )?;
    let circle = ScreenCircleVisual::outlined(
        LogicalScreenPosition::new(32.25, 32.5),
        8.0,
        2.0,
        Color::WHITE,
    )?;
    let initial = app.register_world("paths", move |world| {
        world.spawn(camera)?;
        world.spawn(path.clone())?;
        world.spawn(circle)?;
        world.insert_resource(Step::default())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let mut images = DesktopImages::new();
    images.screen_mode = DesktopScreenMode::Compact;
    let present = |renderer: &mut WgpuRenderer,
                   images: &mut DesktopImages,
                   runner: &HeadlessRunner<Action>|
     -> LogicResult {
        let frame = runner.extracted_frame().ok_or("snapshot")?;
        let report = images::present(
            renderer,
            frame,
            runner.image_assets(),
            images,
            FrameBudget::default(),
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(report.status(), RenderStatus::Drawn);
        // Paths are one Scene command, not N independent line commands.
        assert!(frame.screen_scene().statistics().accepted_commands() <= 2);
        assert!(images.geometry.scratch.is_empty());
        Ok(())
    };
    present(renderer, &mut images, &runner)?;
    assert_eq!(
        runner
            .extracted_frame()
            .unwrap()
            .screen_scene()
            .statistics()
            .accepted_commands(),
        2
    );
    assert_eq!(images.geometry.updates.prepared_runs, 1);
    assert_eq!(images.geometry.updates.compact_runs, 0);
    present(renderer, &mut images, &runner)?;
    assert_eq!(images.geometry.updates.reused_runs, 1);
    assert_eq!(images.geometry.updates.uploaded_bytes, 0);
    for _ in 0..4 {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::ZERO,
            &[],
            LogicalViewport::new(64.0, 64.0)?,
        )) else {
            return Err("path frame rejected".into());
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
        present(renderer, &mut images, &runner)?;
        assert_eq!(images.geometry.updates.prepared_runs, 1);
    }
    for scale in [1.25, 1.0] {
        renderer.set_scale_factor(scale)?;
        present(renderer, &mut images, &runner)?;
        assert_eq!(images.geometry.updates.reused_runs, 1);
        assert_eq!(images.geometry.updates.uploaded_bytes, 0);
    }
    pollster::block_on(renderer.recover_device_and_surface())?;
    images.clear();
    present(renderer, &mut images, &runner)?;
    assert_eq!(images.geometry.updates.prepared_runs, 1);
    images.screen_mode = DesktopScreenMode::Streaming;
    present(renderer, &mut images, &runner)?;
    assert!(!images.geometry.has_runs());
    println!(
        "screen paths: confirmed presents, style/point edits, hide/clip, 1.25 DPI, idle reuse, device recovery and streaming passed; no pixel oracle"
    );
    Ok(())
}

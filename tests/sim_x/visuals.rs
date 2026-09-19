use sim_engine::{Layer, Stroke};
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

fn pointer(x: f32, y: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(800.0, 600.0)?,
    )?)
}

#[test]
fn rounded_fill_and_nested_clips_agree_with_hits_and_rejections_are_atomic() -> LogicResult {
    let mut rect = ScreenRectangleVisual::rounded(
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenVector::new(100.0, 60.0),
        Color::WHITE,
        20.0,
    )
    .map_err(|error| WorldBuildError::user(error.to_string()))?;
    assert!(!rect.contains_pointer(pointer(10.0, 10.0)?));
    assert!(rect.contains_pointer(pointer(30.0, 10.0)?));
    assert!(rect.contains_pointer(pointer(60.0, 40.0)?));
    assert!(!rect.contains_pointer(pointer(110.0, 40.0)?));
    let parent = ScreenClip::new(
        LogicalScreenPosition::new(20.0, 0.0),
        LogicalScreenVector::new(200.0, 100.0),
    )?;
    let child = ScreenClip::new(
        LogicalScreenPosition::new(0.0, 20.0),
        LogicalScreenVector::new(40.0, 50.0),
    )?;
    rect.set_clip(parent.intersection(child));
    assert!(rect.contains_pointer(pointer(30.0, 30.0)?));
    assert!(!rect.contains_pointer(pointer(60.0, 40.0)?));
    rect.set_stroke(Some(Stroke::new(2.0, Color::BLACK)))?;
    let before = rect;
    for radius in [f32::NAN, f32::INFINITY, -1.0] {
        assert!(rect.set_corner_radius(radius).is_err());
        assert_eq!(rect, before);
    }
    assert!(
        rect.set_stroke(Some(Stroke::new(0.0, Color::WHITE)))
            .is_err()
    );
    assert_eq!(rect, before);
    let distant = ScreenClip::new(
        LogicalScreenPosition::new(500.0, 500.0),
        LogicalScreenVector::new(20.0, 20.0),
    )?;
    assert_eq!(parent.intersection(distant), ScreenClip::Empty);
    rect.set_clip(parent.intersection(distant));
    assert!(!rect.contains_pointer(pointer(30.0, 30.0)?));
    Ok(())
}

#[test]
fn presentation_updates_same_frame_while_paused_and_batches_vectors() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_fallible_frame_system(
        |viewport: FrameViewport,
         mut lines: Query<&mut ScreenLineVisual>,
         mut circles: Query<&mut ScreenCircleVisual>|
         -> LogicResult {
            for mut line in &mut lines {
                line.set_endpoints(
                    LogicalScreenPosition::new(20.0, 20.0),
                    LogicalScreenPosition::new(viewport.logical().width() - 20.0, 40.0),
                )?;
            }
            for mut circle in &mut circles {
                circle.set_center(LogicalScreenPosition::new(
                    viewport.logical().width() * 0.5,
                    70.0,
                ))?;
            }
            Ok(())
        },
    );
    let initial = app.register_world("ui", |world| {
        world.spawn(
            ActiveCamera2d::centered(40.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        world.spawn(
            ScreenRectangleVisual::rounded(
                LogicalScreenPosition::new(10.0, 10.0),
                LogicalScreenVector::new(300.0, 100.0),
                Color::WHITE,
                12.0,
            )
            .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        let line = ScreenLineVisual::new(
            LogicalScreenPosition::new(20.0, 20.0),
            LogicalScreenPosition::new(60.0, 20.0),
            2.0,
            Color::BLACK,
        )
        .map_err(|error| WorldBuildError::user(error.to_string()))?;
        world.spawn(line)?;
        world.spawn((Disabled, line))?;
        world.spawn(
            ScreenCircleVisual::new(LogicalScreenPosition::new(40.0, 40.0), 10.0, Color::BLACK)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    runner.set_paused(true);
    for width in [800.0, 1000.0, 400.0] {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(150),
            &[],
            LogicalViewport::new(width, 600.0)?,
        )) else {
            return Err("frame rejected".into());
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
        assert_eq!(report.fixed_ticks_attempted(), 0);
        assert_eq!(runner.components::<Transform2d>().count(), 0);
        let frame = runner.extracted_frame().ok_or("snapshot")?;
        assert_eq!(frame.screen_draws(), &[ScreenDraw::Primitives { run: 0 }]);
        assert_eq!(frame.screen_primitives().count(), 3);
        for primitive in frame.screen_primitives() {
            match primitive {
                ResolvedScreenPrimitive::Line { visual, .. } => {
                    assert_eq!(visual.end().to_vec2().x(), width - 20.0)
                }
                ResolvedScreenPrimitive::Circle { visual, .. } => {
                    assert_eq!(visual.center().to_vec2().x(), width * 0.5)
                }
                _ => {}
            }
        }
    }
    Ok(())
}

#[test]
fn vectors_interleave_with_images_and_empty_clip_does_not_bypass_source_limits() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_screen_images(1));
    let mut app = Application::<Action>::new(config)?;
    let image = app.register_image_rgba8(1, 1, &[255; 4])?;
    let initial = app.register_world("mixed", move |world| {
        world.spawn(
            ActiveCamera2d::centered(1.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        let mut line = ScreenLineVisual::new(
            LogicalScreenPosition::new(10.0, 10.0),
            LogicalScreenPosition::new(100.0, 10.0),
            2.0,
            Color::WHITE,
        )
        .map_err(|error| WorldBuildError::user(error.to_string()))?;
        line.set_layer(Layer::new(-1));
        world.spawn(line)?;
        world.spawn(
            ScreenImageVisual::new(
                image,
                LogicalScreenPosition::new(10.0, 10.0),
                LogicalScreenVector::new(20.0, 20.0),
            )
            .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        let mut circle =
            ScreenCircleVisual::new(LogicalScreenPosition::new(50.0, 50.0), 10.0, Color::WHITE)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
        circle.set_layer(Layer::new(1));
        world.spawn(circle)?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    assert_eq!(
        runner.extracted_frame().ok_or("snapshot")?.screen_draws(),
        &[
            ScreenDraw::Primitives { run: 0 },
            ScreenDraw::Image { index: 0 },
            ScreenDraw::Primitives { run: 1 }
        ]
    );
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_screen_lines(0));
    let mut app = Application::<Action>::new(config)?;
    let initial = app.register_world("disabled-budget", |world| {
        world.spawn(
            ActiveCamera2d::centered(1.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        let mut line = ScreenLineVisual::new(
            LogicalScreenPosition::new(10.0, 10.0),
            LogicalScreenPosition::new(30.0, 10.0),
            2.0,
            Color::WHITE,
        )
        .map_err(|error| WorldBuildError::user(error.to_string()))?;
        line.set_clip(ScreenClip::Empty);
        world.spawn(line)?;
        Ok(())
    })?;
    assert!(app.build_headless(initial).is_err());
    Ok(())
}

#[test]
fn vector_mutations_and_rotated_image_hits_are_atomic() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let image = app.register_image_rgba8(1, 1, &[255; 4])?;
    let mut visual = ScreenImageVisual::new(
        image,
        LogicalScreenPosition::new(100.0, 100.0),
        LogicalScreenVector::new(100.0, 20.0),
    )
    .map_err(|error| WorldBuildError::user(error.to_string()))?;
    assert!(visual.contains_pointer(pointer(190.0, 110.0)?));
    visual.set_rotation(std::f32::consts::FRAC_PI_2)?;
    assert!(!visual.contains_pointer(pointer(190.0, 110.0)?));
    assert!(visual.contains_pointer(pointer(150.0, 150.0)?));
    let old = visual;
    assert!(visual.set_rotation(f32::NAN).is_err());
    assert_eq!(visual, old);
    visual.set_clip(ScreenClip::new(
        LogicalScreenPosition::new(100.0, 100.0),
        LogicalScreenVector::new(100.0, 20.0),
    )?);
    assert!(!visual.contains_pointer(pointer(150.0, 150.0)?));
    let mut line = ScreenLineVisual::new(
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenPosition::new(30.0, 10.0),
        4.0,
        Color::WHITE,
    )
    .map_err(|error| WorldBuildError::user(error.to_string()))?;
    assert!(line.contains_pointer(pointer(20.0, 11.0)?));
    assert!(!line.contains_pointer(pointer(20.0, 13.0)?));
    let old = line;
    assert!(line.set_endpoints(old.start(), old.start()).is_err());
    assert_eq!(line, old);
    let mut circle =
        ScreenCircleVisual::new(LogicalScreenPosition::new(30.0, 30.0), 10.0, Color::WHITE)
            .map_err(|error| WorldBuildError::user(error.to_string()))?;
    let old = circle;
    assert!(circle.set_radius(-1.0).is_err());
    assert_eq!(circle, old);
    assert!(circle.contains_pointer(pointer(40.0, 30.0)?));
    assert!(!circle.contains_pointer(pointer(41.0, 30.0)?));
    Ok(())
}

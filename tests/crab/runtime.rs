use std::{error::Error, time::Duration};

use sim_engine::Layer;
use sim_logic::prelude::*;

const WIDTH: u32 = 460;
const HEIGHT: u32 = 307;
const PIXEL_BYTES: usize = WIDTH as usize * HEIGHT as usize * 4;
const STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Replace,
}

fn application(assets: ImageAssetLimits, image_count: usize) -> LogicResult<Application<Action>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 4)?);
    config.set_image_asset_limits(assets);
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(image_count)
            .with_frame_limits(FrameLimits::new(
                8,
                100,
                1000,
                1024 * 1024,
                PIXEL_BYTES,
                100,
            )),
    );
    Ok(Application::new(config)?)
}

fn exact_limits() -> ImageAssetLimits {
    ImageAssetLimits::new(1, WIDTH, HEIGHT, PIXEL_BYTES)
}

fn position(x: f32, y: f32) -> LogicalScreenPosition {
    LogicalScreenPosition::new(x, y)
}

fn world<const N: usize>(
    app: &mut Application<Action>,
    visuals: [ScreenImageVisual; N],
) -> LogicResult<WorldFactoryId> {
    let camera = ActiveCamera2d::centered(1.0)?;
    Ok(app.register_world("crab", move |world| {
        world.spawn(camera)?;
        for visual in visuals {
            world.spawn(visual)?;
        }
        Ok(())
    })?)
}

fn advance(
    runner: &mut HeadlessRunner<Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        elapsed,
        events,
        LogicalViewport::new(1280.0, 720.0)?,
    )) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

#[test]
fn prepares_the_original_transparent_image_with_normal_screen_defaults() -> LogicResult {
    let mut app = application(exact_limits(), 1)?;
    let crab = sim_logic::draw_crab(&mut app, position(-40.0, 24.0), 230.0)?;
    assert_eq!(crab.image().width(), WIDTH);
    assert_eq!(crab.image().height(), HEIGHT);
    assert_eq!(crab.position(), position(-40.0, 24.0));
    assert_eq!(crab.size(), LogicalScreenVector::new(230.0, 153.5));
    assert_eq!(crab.tint(), Color::WHITE);
    assert_eq!(crab.filter(), ImageFilter::Linear);
    assert_eq!(crab.source_region(), None);
    assert_eq!(crab.layer(), Layer::DEFAULT);
    assert_eq!(crab.draw_order_depth(), 0.0);
    let initial = world(&mut app, [crab])?;
    let runner = app.build_headless(initial)?;
    assert_eq!(runner.image_asset_count(), 1);
    assert_eq!(runner.image_pixel_bytes(), PIXEL_BYTES);
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    let asset = runner.image_asset(crab.image()).ok_or("crab pixels")?;
    assert_eq!((asset.width(), asset.height()), (WIDTH, HEIGHT));
    assert_eq!(asset.pixels().len(), PIXEL_BYTES);
    let mut transparent = 0;
    let mut opaque = 0;
    let mut partial = 0;
    for pixel in asset.pixels().chunks_exact(4) {
        match pixel[3] {
            0 => transparent += 1,
            255 => opaque += 1,
            _ => partial += 1,
        }
    }
    assert!(
        transparent > 0,
        "the original background must stay transparent"
    );
    assert!(
        opaque > 0,
        "the artwork must not decode as empty transparency"
    );
    assert!(
        partial > 0,
        "the original antialiased alpha must be retained"
    );
    let frame = runner.extracted_frame().ok_or("snapshot")?;
    assert_eq!(frame.resolved_screen_images().len(), 1);
    assert_eq!(
        frame.resolved_screen_images()[0].position(),
        crab.position()
    );
    Ok(())
}

#[test]
fn calls_and_visual_copies_reuse_one_handle_at_exact_asset_capacity() -> LogicResult {
    let mut app = application(exact_limits(), 3)?;
    let original = draw_crab(&mut app, position(0.0, 0.0), 460.0)?;
    assert!(matches!(
        app.register_image_rgba8(1, 1, &[255; 4]),
        Err(ImageAssetError::ImageLimitExceeded { limit: 1 })
    ));
    let second = sim_logic::easter_eggs::draw_crab(&mut app, position(600.0, 50.0), 115.0)?;
    let mut copied = original;
    copied.set_position(position(100.0, 400.0))?;
    copied.set_tint(Color::rgba(0.5, 1.0, 0.5, 0.75))?;
    copied.set_draw_order_depth(2.0)?;
    assert_eq!(original.image(), second.image());
    assert_eq!(original.image(), copied.image());
    assert_eq!(original.position(), position(0.0, 0.0));
    assert_eq!(original.tint(), Color::WHITE);
    assert_eq!(second.size(), LogicalScreenVector::new(115.0, 76.75));
    let initial = world(&mut app, [original, second, copied])?;
    let runner = app.build_headless(initial)?;
    assert_eq!(runner.image_asset_count(), 1);
    assert_eq!(runner.image_pixel_bytes(), PIXEL_BYTES);
    let images = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_screen_images();
    assert_eq!(images.len(), 3);
    assert!(images.iter().all(|image| image.image() == original.image()));
    Ok(())
}

#[test]
fn invalid_geometry_does_not_register_or_cache_an_image() -> LogicResult {
    for (point, width) in [
        (position(0.0, 0.0), 0.0),
        (position(0.0, 0.0), -1.0),
        (position(0.0, 0.0), f32::NAN),
        (position(0.0, 0.0), f32::INFINITY),
        (position(f32::NAN, 0.0), 230.0),
        (position(0.0, f32::NEG_INFINITY), 230.0),
        (position(f32::MAX, 0.0), 230.0),
        (position(0.0, f32::MAX), 230.0),
        (position(f32::MAX * 0.75, 0.0), f32::MAX * 0.5),
    ] {
        let mut app = application(exact_limits(), 0)?;
        let error = draw_crab(&mut app, point, width).unwrap_err();
        assert!(matches!(error, CrabDrawError::Visual(_)));
        assert!(error.source().is_some());
        // A successful unrelated registration proves that even the count-one
        // registry was untouched by the rejected crab geometry.
        let sentinel = app.register_image_rgba8(1, 1, &[7, 8, 9, 255])?;
        let initial = world(&mut app, [])?;
        let runner = app.build_headless(initial)?;
        assert_eq!(runner.image_asset_count(), 1);
        assert_eq!(runner.image_pixel_bytes(), 4);
        assert_eq!(
            runner.image_asset(sentinel).ok_or("sentinel")?.pixels(),
            &[7, 8, 9, 255]
        );
    }
    Ok(())
}

#[test]
fn geometry_errors_precede_asset_errors_and_do_not_poison_an_existing_cache() -> LogicResult {
    let mut disabled = application(ImageAssetLimits::new(0, 0, 0, 0), 0)?;
    assert!(matches!(
        draw_crab(&mut disabled, position(0.0, 0.0), -1.0),
        Err(CrabDrawError::Visual(_))
    ));
    let mut app = application(exact_limits(), 0)?;
    let first = draw_crab(&mut app, position(0.0, 0.0), 230.0)?;
    assert!(matches!(
        draw_crab(&mut app, position(f32::MAX, 0.0), 230.0),
        Err(CrabDrawError::Visual(_))
    ));
    let again = draw_crab(&mut app, position(-1000.0, -1000.0), 46.0)?;
    assert_eq!(again.image(), first.image());
    let initial = world(&mut app, [])?;
    let runner = app.build_headless(initial)?;
    assert_eq!(runner.image_asset_count(), 1);
    assert_eq!(runner.image_pixel_bytes(), PIXEL_BYTES);
    Ok(())
}

#[test]
fn large_finite_width_does_not_overflow_an_unnecessary_aspect_intermediate() -> LogicResult {
    let mut app = application(exact_limits(), 0)?;
    let crab = draw_crab(&mut app, position(0.0, 0.0), f32::MAX)?;
    assert_eq!(crab.size().to_vec2().x(), f32::MAX);
    assert!(crab.size().to_vec2().y().is_finite());
    assert!(crab.size().to_vec2().y() > 0.0);
    // This is a geometry-constructor check, not a claim of GPU portability.
    Ok(())
}

#[test]
fn original_asset_count_dimension_and_pixel_limits_are_respected() -> LogicResult {
    for (limits, expected) in [
        (
            ImageAssetLimits::new(1, WIDTH, HEIGHT, PIXEL_BYTES + 4),
            "count",
        ),
        (
            ImageAssetLimits::new(2, WIDTH - 1, HEIGHT, PIXEL_BYTES + 4),
            "width",
        ),
        (
            ImageAssetLimits::new(2, WIDTH, HEIGHT - 1, PIXEL_BYTES + 4),
            "height",
        ),
        (
            ImageAssetLimits::new(2, WIDTH, HEIGHT, PIXEL_BYTES + 3),
            "bytes",
        ),
    ] {
        let mut app = application(limits, 0)?;
        let sentinel = app.register_image_rgba8(1, 1, &[1, 2, 3, 255])?;
        for _ in 0..2 {
            let error = draw_crab(&mut app, position(0.0, 0.0), 230.0).unwrap_err();
            assert!(error.source().is_some());
            match (expected, error) {
                (
                    "count",
                    CrabDrawError::Image(ImageAssetError::ImageLimitExceeded { limit: 1 }),
                ) => {}
                (
                    "width",
                    CrabDrawError::Image(ImageAssetError::DimensionsExceeded {
                        width: WIDTH,
                        max_width,
                        ..
                    }),
                ) => assert_eq!(max_width, WIDTH - 1),
                (
                    "height",
                    CrabDrawError::Image(ImageAssetError::DimensionsExceeded {
                        height: HEIGHT,
                        max_height,
                        ..
                    }),
                ) => assert_eq!(max_height, HEIGHT - 1),
                (
                    "bytes",
                    CrabDrawError::Image(ImageAssetError::PixelByteLimitExceeded {
                        retained: 4,
                        incoming: PIXEL_BYTES,
                        ..
                    }),
                ) => {}
                (_, error) => panic!("wrong {expected} failure: {error:?}"),
            }
        }
        let initial = world(&mut app, [])?;
        let runner = app.build_headless(initial)?;
        assert_eq!(runner.image_asset_count(), 1);
        assert_eq!(runner.image_pixel_bytes(), 4);
        assert_eq!(
            runner.image_asset(sentinel).ok_or("sentinel")?.pixels(),
            &[1, 2, 3, 255]
        );
    }
    Ok(())
}

#[test]
fn helper_neither_spawns_entities_nor_enables_screen_image_extraction() -> LogicResult {
    let mut app = application(exact_limits(), 0)?;
    let _unspawned = draw_crab(&mut app, position(0.0, 0.0), 230.0)?;
    let initial = world(&mut app, [])?;
    let runner = app.build_headless(initial)?;
    assert_eq!(runner.components::<ScreenImageVisual>().count(), 0);
    assert!(
        runner
            .extracted_frame()
            .ok_or("snapshot")?
            .resolved_screen_images()
            .is_empty()
    );
    assert_eq!(runner.image_asset_count(), 1);

    let mut app = application(exact_limits(), 0)?;
    let crab = draw_crab(&mut app, position(0.0, 0.0), 230.0)?;
    let initial = world(&mut app, [crab])?;
    assert!(matches!(
        app.build_headless(initial),
        Err(RunnerBuildError::InitialWorld(
            CandidateFailure::Extraction(ExtractionError::ScreenImageLimitExceeded { limit: 0 })
        ))
    ));
    Ok(())
}

#[test]
fn repeated_calls_do_not_raise_image_placement_or_entity_limits() -> LogicResult {
    let mut app = application(exact_limits(), 1)?;
    let first = draw_crab(&mut app, position(0.0, 0.0), 230.0)?;
    let second = draw_crab(&mut app, position(400.0, 0.0), 230.0)?;
    let initial = world(&mut app, [first, second])?;
    assert!(matches!(
        app.build_headless(initial),
        Err(RunnerBuildError::InitialWorld(
            CandidateFailure::Extraction(ExtractionError::ScreenImageLimitExceeded { limit: 1 })
        ))
    ));

    let mut config = AppConfig::default();
    config.set_entity_limit(1)?;
    config.set_render_limits(RenderLimits::default().with_max_screen_images(1));
    let mut app = Application::<Action>::new(config)?;
    let crab = draw_crab(&mut app, position(0.0, 0.0), 230.0)?;
    let initial = world(&mut app, [crab])?;
    assert!(matches!(
        app.build_headless(initial),
        Err(RunnerBuildError::InitialWorld(CandidateFailure::Factory(
            WorldBuildError::EntityLimitExceeded { limit: 1 }
        )))
    ));
    Ok(())
}

#[test]
fn caches_are_application_scoped_and_foreign_visuals_remain_invalid() -> LogicResult {
    let mut first_app = application(exact_limits(), 1)?;
    let first = draw_crab(&mut first_app, position(0.0, 0.0), 230.0)?;
    let mut second_app = application(exact_limits(), 1)?;
    let second = draw_crab(&mut second_app, position(0.0, 0.0), 230.0)?;
    assert_ne!(first.image(), second.image());
    assert_eq!(
        draw_crab(&mut first_app, position(10.0, 10.0), 115.0)?.image(),
        first.image()
    );
    assert_eq!(
        draw_crab(&mut second_app, position(10.0, 10.0), 115.0)?.image(),
        second.image()
    );
    let initial = world(&mut first_app, [first])?;
    let runner = first_app.build_headless(initial)?;
    assert!(runner.image_asset(second.image()).is_none());
    let foreign = world(&mut second_app, [first])?;
    assert!(matches!(
        second_app.build_headless(foreign),
        Err(RunnerBuildError::InitialWorld(CandidateFailure::Extraction(
            ExtractionError::UnregisteredImageAsset { image, .. }
        ))) if image == first.image()
    ));
    Ok(())
}

#[test]
fn one_cached_asset_survives_successful_and_failed_world_replacement() -> LogicResult {
    for valid_target in [false, true] {
        let mut app = application(exact_limits(), 1)?;
        app.bind_key(PhysicalKeyCode::Enter, Action::Replace)?;
        app.add_world_replacement_on_press_system();
        let first = draw_crab(&mut app, position(24.0, 24.0), 230.0)?;
        let second = draw_crab(&mut app, position(640.0, 360.0), 115.0)?;
        assert_eq!(first.image(), second.image());
        let camera = ActiveCamera2d::centered(1.0)?;
        let target = app.register_world("target", move |world| {
            world.spawn(camera)?;
            world.spawn(second)?;
            if !valid_target {
                world.spawn(first)?;
            }
            Ok(())
        })?;
        let initial = app.register_world("initial", move |world| {
            world.spawn(camera)?;
            world.spawn(first)?;
            world.insert_resource(WorldReplacementOnPress::new(Action::Replace, target))?;
            Ok(())
        })?;
        let mut runner = app.build_headless(initial)?;
        let generation = runner.world_generation();
        let pixel_pointer = runner
            .image_asset(first.image())
            .ok_or("pixels")?
            .pixels()
            .as_ptr();
        advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::key(
                PhysicalKeyCode::Enter,
                ButtonState::Pressed,
            )],
        )?;
        let report = advance(&mut runner, STEP * 3, &[])?;
        assert!(report.failure().is_none());
        assert_eq!(runner.image_asset_count(), 1);
        assert_eq!(runner.image_pixel_bytes(), PIXEL_BYTES);
        assert_eq!(
            runner
                .image_asset(first.image())
                .ok_or("pixels")?
                .pixels()
                .as_ptr(),
            pixel_pointer
        );
        let images = runner
            .extracted_frame()
            .ok_or("snapshot")?
            .resolved_screen_images();
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].image(), first.image());
        if valid_target {
            assert!(
                matches!(report.transition(), FrameTransition::Committed { target: actual, .. } if *actual == target)
            );
            assert_ne!(runner.world_generation(), generation);
            assert_eq!(images[0].position(), second.position());
        } else {
            assert!(matches!(
                report.transition(),
                FrameTransition::PreparationFailed { .. }
            ));
            assert_eq!(runner.world_generation(), generation);
            assert_eq!(images[0].position(), first.position());
        }
    }
    Ok(())
}

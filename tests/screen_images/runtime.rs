use super::support::*;
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};
use std::time::Duration;

#[test]
fn registration_copies_pixels_and_handles_are_application_scoped() -> LogicResult {
    let mut app = application(4)?;
    let mut source = PIXELS;
    let first = app.register_image_rgba8(2, 2, &source)?;
    source.fill(0);
    let second = app.register_image_rgba8(2, 2, &PIXELS)?;
    let foreign = application(4)?.register_image_rgba8(2, 2, &PIXELS)?;
    assert_ne!(first, second);
    assert_ne!(first, foreign);
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = image(first, -40.0, 0.0)?;
    let world = app.register_world("images", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        Ok(())
    })?;
    let runner = app.build_headless(world)?;
    assert_eq!(runner.image_asset_count(), 2);
    assert_eq!(runner.image_pixel_bytes(), 32);
    assert_eq!(runner.image_asset(first).ok_or("image")?.pixels(), PIXELS);
    assert!(runner.image_asset(foreign).is_none());
    assert_eq!(
        snapshot(&runner)?.resolved_screen_images()[0].image(),
        first
    );
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    Ok(())
}

#[test]
fn foreign_image_rejects_candidate_before_it_becomes_active() -> LogicResult {
    let foreign = application(2)?.register_image_rgba8(2, 2, &PIXELS)?;
    for disabled in [false, true] {
        let mut app = application(2)?;
        app.register_image_rgba8(2, 2, &PIXELS)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let visual = image(foreign, 0.0, 0.0)?;
        let world = app.register_world("foreign", move |world| {
            world.spawn(camera)?;
            if disabled {
                world.spawn((visual, Disabled))?;
            } else {
                world.spawn(visual)?;
            }
            Ok(())
        })?;
        let result = app.build_headless(world);
        if disabled {
            assert!(snapshot(&result?)?.resolved_screen_images().is_empty());
        } else {
            assert!(
                matches!(result, Err(RunnerBuildError::InitialWorld(CandidateFailure::Extraction(ExtractionError::UnregisteredImageAsset{image,..}))) if image==foreign)
            );
        }
    }
    Ok(())
}

#[test]
fn immutable_pixels_survive_successful_and_failed_world_replacement() -> LogicResult {
    for valid in [false, true] {
        let mut app = application(2)?;
        let id = app.register_image_rgba8(2, 2, &PIXELS)?;
        let foreign = application(2)?.register_image_rgba8(2, 2, &PIXELS)?;
        app.bind_key(PhysicalKeyCode::Enter, TestAction::Replace)?;
        app.add_world_replacement_on_press_system();
        let camera = ActiveCamera2d::centered(10.0)?;
        let target_visual = image(if valid { id } else { foreign }, 200.0, 0.0)?;
        let target = app.register_world("target", move |world| {
            world.spawn(camera)?;
            world.spawn(target_visual)?;
            Ok(())
        })?;
        let initial_visual = image(id, 10.0, 0.0)?;
        let initial = app.register_world("initial", move |world| {
            world.spawn(camera)?;
            world.spawn(initial_visual)?;
            world.insert_resource(WorldReplacementOnPress::new(TestAction::Replace, target))?;
            Ok(())
        })?;
        let mut runner = app.build_headless(initial)?;
        let generation = runner.world_generation();
        let pixels = runner.image_asset(id).ok_or("pixels")?.pixels().as_ptr();
        let pending = advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::key(
                PhysicalKeyCode::Enter,
                ButtonState::Pressed,
            )],
        )?;
        assert!(matches!(pending.transition(), FrameTransition::None));
        let report = advance(&mut runner, STEP * 3, &[])?;
        assert!(report.failure().is_none());
        assert_eq!(
            runner.image_asset(id).ok_or("pixels")?.pixels().as_ptr(),
            pixels
        );
        assert_eq!(runner.image_asset_count(), 1);
        let row = snapshot(&runner)?.resolved_screen_images()[0];
        assert_eq!(row.image(), id);
        if valid {
            assert!(
                matches!(report.transition(),FrameTransition::Committed{target:actual,..} if *actual==target)
            );
            assert_ne!(runner.world_generation(), generation);
            assert_eq!(row.position(), target_visual.position());
        } else {
            assert!(matches!(
                report.transition(),
                FrameTransition::PreparationFailed { .. }
            ));
            assert_eq!(runner.world_generation(), generation);
            assert_eq!(row.position(), initial_visual.position());
        }
    }
    Ok(())
}

#[test]
fn bad_live_reference_keeps_last_good_snapshot_and_can_be_repaired() -> LogicResult {
    let mut app = application(2)?;
    let id = app.register_image_rgba8(2, 2, &PIXELS)?;
    let foreign = application(2)?.register_image_rgba8(2, 2, &PIXELS)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Modify)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>,
              mut images: Query<&mut ScreenImageVisual>|
              -> LogicResult {
            for mut image in &mut images {
                if input.has_press_occurrence(TestAction::Modify) {
                    image.set_image(foreign)?;
                }
                if input.has_release_occurrence(TestAction::Modify) {
                    image.set_image(id)?;
                    image.set_position(LogicalScreenPosition::new(200.0, 30.0))?;
                }
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = image(id, 0.0, 0.0)?;
    let initial = app.register_world("repairable", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let before = snapshot(&runner)?.resolved_screen_images().to_vec();
    let failed = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(
        matches!(failed.failure(),Some(FrameFailure::Extraction(ExtractionError::UnregisteredImageAsset{image,..})) if *image==foreign)
    );
    assert_eq!(snapshot(&runner)?.resolved_screen_images(), before);
    let repaired = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    assert!(repaired.failure().is_none());
    assert_eq!(snapshot(&runner)?.resolved_screen_images()[0].image(), id);
    assert_eq!(
        snapshot(&runner)?.resolved_screen_images()[0].position(),
        LogicalScreenPosition::new(200.0, 30.0)
    );
    Ok(())
}

#[test]
fn image_extraction_is_opt_in_and_disabled_sources_do_not_consume_the_cap() -> LogicResult {
    assert_eq!(RenderLimits::default().max_screen_images(), 0);
    assert_eq!(
        RenderLimits::default().frame_limits().max_texture_bytes(),
        0
    );
    for disabled in [false, true] {
        let mut app = application(0)?;
        let id = app.register_image_rgba8(2, 2, &PIXELS)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let visual = image(id, 0.0, 0.0)?;
        let initial = app.register_world("zero-cap", move |world| {
            world.spawn(camera)?;
            if disabled {
                world.spawn((visual, Disabled))?;
            } else {
                world.spawn(visual)?;
            }
            Ok(())
        })?;
        let result = app.build_headless(initial);
        if disabled {
            assert!(snapshot(&result?)?.resolved_screen_images().is_empty());
        } else {
            assert!(matches!(
                result,
                Err(RunnerBuildError::InitialWorld(
                    CandidateFailure::Extraction(ExtractionError::ScreenImageLimitExceeded {
                        limit: 0
                    })
                ))
            ));
        }
    }
    Ok(())
}

#[derive(Resource)]
struct Target(LogicEntity);

#[test]
fn commands_enable_disable_and_replace_image_at_the_normal_barrier() -> LogicResult {
    let mut app = application(2)?;
    let first = app.register_image_rgba8(2, 2, &PIXELS)?;
    let second = app.register_image_rgba8(2, 2, &PIXELS)?;
    let replacement = image(second, 80.0, 0.0)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Modify)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>,
              target: Res<Target>,
              mut commands: Commands|
              -> LogicResult {
            if input.has_press_occurrence(TestAction::Modify) {
                commands.disable(target.0)?;
            }
            if input.has_release_occurrence(TestAction::Modify) {
                commands.enable(target.0)?;
                commands.insert(target.0, replacement)?;
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let original = image(first, 0.0, 0.0)?;
    let initial = app.register_world("commands", move |world| {
        world.spawn(camera)?;
        let target = world.spawn(original)?;
        world.insert_resource(Target(target))?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let source = snapshot(&runner)?.resolved_screen_images()[0].source();
    let disabled = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(disabled.failure().is_none());
    assert!(snapshot(&runner)?.resolved_screen_images().is_empty());
    let enabled = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    assert!(enabled.failure().is_none());
    let row = snapshot(&runner)?.resolved_screen_images()[0];
    assert_eq!(row.source(), source);
    assert_eq!(row.image(), second);
    assert_eq!(row.position(), replacement.position());
    Ok(())
}

#[test]
fn paused_frame_layout_uses_current_viewport_without_interpolating_images() -> LogicResult {
    let mut app = application(2)?;
    let id = app.register_image_rgba8(2, 2, &PIXELS)?;
    app.add_fallible_frame_system(
        |viewport: FrameViewport, mut images: Query<&mut ScreenImageVisual>| -> LogicResult {
            for mut image in &mut images {
                image.set_geometry(
                    LogicalScreenPosition::new(-10.0, -20.0),
                    LogicalScreenVector::new(
                        viewport.logical().width(),
                        viewport.logical().height(),
                    ),
                )?;
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(64.0)?;
    let visual = image(id, 0.0, 0.0)?;
    let initial = app.register_world("resize", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    runner.set_paused(true);
    for (width, height) in [(320.0, 900.0), (1920.0, 1080.0), (1.0, 1.0)] {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            STEP * 4,
            &[],
            LogicalViewport::new(width, height)?,
        )) else {
            return Err("frame rejected".into());
        };
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 0);
        assert_eq!(
            snapshot(&runner)?.resolved_screen_images()[0].size(),
            LogicalScreenVector::new(width, height)
        );
    }
    Ok(())
}

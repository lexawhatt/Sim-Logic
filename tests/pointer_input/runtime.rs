use std::{collections::HashSet, time::Duration};

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn click_samples_retain_event_geometry_and_deliver_once_to_shared_fixed_snapshot() -> LogicResult {
    let mut application = application()?;
    application.add_fixed_system(observe_fixed);
    let initial = register_world(&mut application, "retained-click")?;
    let mut runner = application.build_headless(initial)?;
    let clicked = sample(200.0, 150.0, 800.0)?;
    let released = sample(300.0, 250.0, 900.0)?;
    let latest = sample(400.0, 350.0, 1_100.0)?;
    let following = sample(500.0, 450.0, 1_200.0)?;

    let first = advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(clicked),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(released),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::pointer_moved(latest),
        ],
    )?;
    assert_eq!(first.fixed_ticks_attempted(), 0);
    let frame = probe(&runner)?.frames[0].clone();
    assert_eq!(frame.pointer, Some(latest));
    assert_eq!(frame.presses.len(), 1);
    assert_eq!(frame.releases.len(), 1);
    assert_eq!(frame.presses[0].pointer(), Some(clicked));
    assert_eq!(frame.releases[0].pointer(), Some(released));
    assert_ne!(frame.presses[0].intent(), frame.releases[0].intent());

    advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(probe(&runner)?.frames[1].pointer, Some(latest));
    assert!(probe(&runner)?.frames[1].presses.is_empty());
    let catch_up = advance(
        &mut runner,
        FIXED_STEP * 3,
        &[InputEvent::pointer_moved(following)],
    )?;
    assert_eq!(catch_up.fixed_ticks_attempted(), 3);
    let observations = probe(&runner)?;
    assert_eq!(observations.fixed.len(), 6);
    for tick in observations.fixed.chunks_exact(2) {
        assert_eq!(
            tick[0], tick[1],
            "systems share the same immutable snapshot"
        );
        assert_eq!(tick[0].pointer, Some(following));
    }
    assert_eq!(observations.fixed[0].presses, frame.presses);
    assert_eq!(observations.fixed[0].releases, frame.releases);
    for snapshot in &observations.fixed[2..] {
        assert!(snapshot.presses.is_empty());
        assert!(snapshot.releases.is_empty());
    }
    Ok(())
}

#[test]
fn keyboard_and_mouse_share_held_state_but_preserve_independent_edges() -> LogicResult {
    let mut runner = runner()?;
    let pointer = sample(123.0, 234.0, 800.0)?;
    advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::pointer_moved(pointer),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    let first = &probe(&runner)?.fixed[0];
    assert!(first.held);
    assert_eq!(first.presses.len(), 2);
    assert_eq!(first.presses[0].pointer(), None);
    assert_eq!(first.presses[1].pointer(), Some(pointer));
    assert_ne!(first.presses[0].intent(), first.presses[1].intent());

    advance(&mut runner, FIXED_STEP, &[InputEvent::PointerLeft])?;
    let leaving = &probe(&runner)?.fixed[1];
    assert_eq!(leaving.pointer, None);
    assert!(leaving.held, "the keyboard still holds the shared action");
    assert_eq!(leaving.releases.len(), 1);
    assert_eq!(leaving.releases[0].pointer(), None);
    advance(&mut runner, FIXED_STEP, &[InputEvent::PointerLeft])?;
    assert!(probe(&runner)?.fixed[2].releases.is_empty());

    advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    let released = &probe(&runner)?.fixed[3];
    assert!(!released.held);
    assert_eq!(released.releases.len(), 1);
    assert_eq!(released.releases[0].pointer(), None);
    Ok(())
}

#[test]
fn button_without_known_position_is_valid_and_duplicate_binding_preserves_original() -> LogicResult
{
    let mut application = application()?;
    let duplicate = application
        .bind_mouse_button(MouseButton::Left, TestAction::Pause)
        .err()
        .ok_or("duplicate mouse binding should fail")?;
    assert_eq!(duplicate.button(), MouseButton::Left);
    let initial = register_world(&mut application, "unknown-position")?;
    let mut runner = application.build_headless(initial)?;
    advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Pressed,
        )],
    )?;
    let snapshot = &probe(&runner)?.fixed[0];
    assert_eq!(snapshot.pointer, None);
    assert_eq!(snapshot.presses.len(), 1);
    assert_eq!(snapshot.presses[0].pointer(), None);
    assert!(!runner.is_paused());
    Ok(())
}

#[test]
fn pause_keeps_latest_position_and_held_button_without_replaying_discarded_clicks() -> LogicResult {
    let mut runner = runner()?;
    let before_pause = sample(100.0, 200.0, 800.0)?;
    let during_pause = sample(300.0, 400.0, 900.0)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(before_pause),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
        ],
    )?;
    assert!(runner.is_paused());
    let paused = advance(
        &mut runner,
        FIXED_STEP * 3,
        &[
            InputEvent::pointer_moved(during_pause),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    assert_eq!(paused.fixed_ticks_attempted(), 0);
    assert_eq!(probe(&runner)?.frames[1].pointer, Some(during_pause));
    assert_eq!(probe(&runner)?.frames[1].presses.len(), 1);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
        ],
    )?;
    assert!(!runner.is_paused());
    advance(&mut runner, FIXED_STEP, &[])?;
    let resumed = &probe(&runner)?.fixed[0];
    assert_eq!(resumed.pointer, Some(during_pause));
    assert!(resumed.held);
    assert!(resumed.presses.is_empty());
    assert!(resumed.releases.is_empty());
    Ok(())
}

#[test]
fn replacement_preserves_pointer_and_held_state_but_retires_click_edges() -> LogicResult {
    let mut application = application()?;
    let target = register_world(&mut application, "target")?;
    application.add_fallible_frame_system(
        move |input: FrameInput<TestAction>, mut commands: Commands| -> LogicResult {
            if let Some(edge) = input.pressed(TestAction::Replace).next() {
                commands.replace_world(edge.intent(), target)?;
            }
            Ok(())
        },
    );
    let initial = register_world(&mut application, "source")?;
    let mut runner = application.build_headless(initial)?;
    let clicked = sample(110.0, 220.0, 800.0)?;
    let pointer = sample(330.0, 440.0, 900.0)?;
    let replacement = advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(clicked),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(pointer),
            InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        ],
    )?;
    assert!(matches!(
        replacement.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_eq!(runner.active_world_name(), "target");
    assert_eq!(
        probe(&runner)?.frames[0].presses[0].pointer(),
        Some(clicked)
    );
    advance(&mut runner, FIXED_STEP, &[])?;
    let target_tick = &probe(&runner)?.fixed[0];
    assert_eq!(target_tick.pointer, Some(pointer));
    assert!(target_tick.held);
    assert!(target_tick.presses.is_empty());
    assert!(target_tick.releases.is_empty());
    assert_eq!(probe(&runner)?.frames[1].pointer, Some(pointer));
    Ok(())
}

#[test]
fn finite_pointer_samples_preserve_outside_coordinates_and_lawful_event_hashing() -> LogicResult {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(PointerSample::new(LogicalScreenPosition::new(invalid, 0.0), viewport).is_err());
        assert!(PointerSample::new(LogicalScreenPosition::new(0.0, invalid), viewport).is_err());
    }
    let outside = sample(-100.0, 700.0, 800.0)?;
    assert_eq!(
        outside.position(),
        LogicalScreenPosition::new(-100.0, 700.0)
    );
    assert_eq!(outside.viewport(), viewport);
    let positive_zero = InputEvent::pointer_moved(sample(0.0, 0.0, 800.0)?);
    let negative_zero = InputEvent::pointer_moved(sample(-0.0, -0.0, 800.0)?);
    assert_eq!(positive_zero, negative_zero);
    let unique = HashSet::from([positive_zero, negative_zero]);
    assert_eq!(unique.len(), 1);
    Ok(())
}

#[test]
fn world_conversion_uses_selected_camera_and_propagates_engine_projection_errors() -> LogicResult {
    let pointer = sample(600.0, 450.0, 800.0)?;
    let mut camera = Camera2d::new(Vec2::new(2.0, 3.0), 20.0)?;
    assert_eq!(pointer.world_position(camera)?, Vec2::new(12.0, -4.5));
    camera.set_rotation(0.4)?;
    camera.set_projection(sim_engine::Projection2d::new(0.6, 1.25)?);
    let world = Vec2::new(-4.0, 7.0);
    let projected = camera.projected_world_to_screen(world, 0.0, pointer.viewport())?;
    let rotated = PointerSample::new(projected, pointer.viewport())?.world_position(camera)?;
    assert!((rotated.x() - world.x()).abs() < 0.0001);
    assert!((rotated.y() - world.y()).abs() < 0.0001);

    camera.set_projection(sim_engine::Projection2d::new(
        std::f32::consts::FRAC_PI_2,
        1.0,
    )?);
    let error = pointer.world_position(camera);
    assert!(matches!(
        error,
        Err(Camera2dError::SingularProjection { .. })
    ));
    assert_eq!(
        error,
        camera.screen_to_world(pointer.position(), pointer.viewport())
    );
    Ok(())
}

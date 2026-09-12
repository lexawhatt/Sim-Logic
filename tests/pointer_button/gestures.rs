use std::time::Duration;

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn complete_same_frame_gestures_use_each_edges_target_pointer_and_token() -> LogicResult {
    let mut runner = runner(false)?;
    let pressed = sample(20.0, 30.0, 800.0, 600.0)?;
    let released = sample(50.0, 60.0, 900.0, 650.0)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(pressed),
            press(),
            InputEvent::pointer_moved(released),
            release(),
            motion(180.0, 40.0)?,
            press(),
            release(),
            motion(700.0, 500.0)?,
        ],
    )?;
    let observations = frame(&runner)?;
    assert_eq!(observations.len(), 4);
    assert_eq!(
        events(&runner)?,
        [
            (EventKind::Pressed, Target::First),
            (EventKind::Clicked, Target::First),
            (EventKind::Pressed, Target::Second),
            (EventKind::Clicked, Target::Second),
        ]
    );
    for entry in observations {
        let event = entry.event.ok_or("button event missing")?;
        assert!(entry.claimed);
        assert_eq!(event.pointer, entry.edge.pointer());
        assert_eq!(event.intent, entry.edge.intent());
    }
    assert_eq!(
        observations[0].event.ok_or("press missing")?.pointer,
        Some(pressed)
    );
    assert_eq!(
        observations[1].event.ok_or("click missing")?.pointer,
        Some(released)
    );
    assert_ne!(observations[0].edge.intent(), observations[1].edge.intent());
    assert_eq!(observations[0].captured, Some(Target::First));
    assert_eq!(observations[1].captured, None);
    assert_eq!(captured(&runner)?, None);
    assert_eq!(probe(&runner)?.background_presses, 0);
    assert_eq!(probe(&runner)?.background_releases, 0);
    Ok(())
}

#[test]
fn outside_press_remains_unclaimed_when_released_over_a_button() -> LogicResult {
    let mut runner = runner(false)?;
    advance(&mut runner, STEP, &[motion(500.0, 400.0)?, press()])?;
    assert_eq!(captured(&runner)?, None);
    advance(&mut runner, STEP, &[motion(20.0, 30.0)?, release()])?;
    assert!(events(&runner)?.is_empty());
    assert!(
        probe(&runner)?
            .frames
            .iter()
            .flatten()
            .all(|entry| !entry.claimed)
    );
    assert_eq!(probe(&runner)?.background_presses, 1);
    assert_eq!(probe(&runner)?.background_releases, 1);
    advance(&mut runner, STEP, &[press(), release()])?;
    assert_eq!(
        events(&runner)?,
        [
            (EventKind::Pressed, Target::First),
            (EventKind::Clicked, Target::First),
        ]
    );
    Ok(())
}

#[test]
fn captured_target_survives_motion_but_release_over_another_target_cancels() -> LogicResult {
    let mut runner = runner(false)?;
    advance(&mut runner, STEP, &[motion(20.0, 30.0)?, press()])?;
    advance(&mut runner, STEP, &[motion(180.0, 30.0)?])?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    advance(&mut runner, STEP, &[release()])?;
    let outside = frame(&runner)?[0];
    assert!(outside.claimed);
    assert_eq!(
        outside.event.ok_or("cancel missing")?.kind,
        EventKind::ReleasedOutside
    );
    assert_eq!(outside.event.ok_or("cancel missing")?.target, Target::First);
    assert_eq!(
        outside.event.ok_or("cancel missing")?.intent,
        outside.edge.intent()
    );
    assert_eq!(captured(&runner)?, None);

    advance(
        &mut runner,
        STEP,
        &[motion(20.0, 30.0)?, press(), motion(500.0, 400.0)?],
    )?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    advance(&mut runner, STEP, &[motion(50.0, 60.0)?, release()])?;
    assert_eq!(
        frame(&runner)?[0].event.ok_or("click missing")?.kind,
        EventKind::Clicked
    );
    assert_eq!(probe(&runner)?.background_releases, 0);
    Ok(())
}

#[test]
fn keyboard_alias_and_other_mouse_buttons_do_not_change_capture() -> LogicResult {
    let mut runner = runner(false)?;
    advance(
        &mut runner,
        STEP,
        &[
            motion(20.0, 30.0)?,
            press(),
            motion(180.0, 30.0)?,
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Right, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Middle, ButtonState::Pressed),
            key(PhysicalKeyCode::Space, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Right, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Middle, ButtonState::Released),
        ],
    )?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    for entry in &frame(&runner)?[1..] {
        assert!(!entry.claimed);
        assert_eq!(entry.event, None);
        assert_eq!(entry.captured, Some(Target::First));
    }
    advance(&mut runner, STEP, &[motion(20.0, 30.0)?, release()])?;
    assert_eq!(
        frame(&runner)?[0].event.ok_or("click missing")?.kind,
        EventKind::Clicked
    );
    assert_eq!(
        runner
            .resource::<Interaction>()
            .ok_or("interaction missing")?
            .button
            .button(),
        MouseButton::Left
    );
    Ok(())
}

#[test]
fn cancellation_is_claimed_and_never_confirms_then_a_new_gesture_can_click() -> LogicResult {
    for (event, reason) in [
        (
            InputEvent::PointerLeft,
            InputCancellationReason::PointerLeft,
        ),
        (InputEvent::FocusLost, InputCancellationReason::FocusLost),
    ] {
        let mut runner = runner(false)?;
        advance(
            &mut runner,
            STEP,
            &[
                motion(20.0, 30.0)?,
                key(PhysicalKeyCode::Space, ButtonState::Pressed),
                press(),
                event,
                motion(180.0, 30.0)?,
                press(),
                release(),
            ],
        )?;
        assert_eq!(
            events(&runner)?,
            [
                (EventKind::Pressed, Target::First),
                (EventKind::InputCancelled(reason), Target::First),
                (EventKind::Pressed, Target::Second),
                (EventKind::Clicked, Target::Second),
            ]
        );
        let cancellation = frame(&runner)?
            .iter()
            .find(|entry| {
                entry
                    .event
                    .is_some_and(|event| event.kind == EventKind::InputCancelled(reason))
            })
            .ok_or("cancellation missing")?;
        assert!(cancellation.claimed);
        assert_eq!(cancellation.captured, None);
        assert_eq!(
            cancellation.event.ok_or("cancellation missing")?.intent,
            cancellation.edge.intent()
        );
        assert_eq!(cancellation.edge.pointer(), None);
        assert_eq!(probe(&runner)?.background_releases, 0);
    }
    Ok(())
}

#[test]
fn invalidation_suppresses_release_even_after_the_same_target_is_enabled_again() -> LogicResult {
    let mut runner = runner(false)?;
    advance(&mut runner, STEP, &[motion(20.0, 30.0)?, press()])?;
    advance(
        &mut runner,
        STEP,
        &[
            key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            key(PhysicalKeyCode::KeyD, ButtonState::Released),
            key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            key(PhysicalKeyCode::KeyN, ButtonState::Pressed),
            press(),
        ],
    )?;
    assert_eq!(captured(&runner)?, None);
    assert_eq!(
        probe(&runner)?.explicit_cancellations,
        [Some(Target::First), None]
    );
    advance(&mut runner, STEP, &[release()])?;
    assert!(frame(&runner)?[0].claimed);
    assert_eq!(frame(&runner)?[0].event, None);
    assert_eq!(probe(&runner)?.background_releases, 0);
    advance(&mut runner, STEP, &[press(), release()])?;
    assert_eq!(
        events(&runner)?,
        [
            (EventKind::Pressed, Target::First),
            (EventKind::Pressed, Target::First),
            (EventKind::Clicked, Target::First),
        ]
    );
    Ok(())
}

#[test]
fn cancelling_idle_or_outside_state_does_not_claim_background_gestures() -> LogicResult {
    let mut runner = runner(false)?;
    advance(
        &mut runner,
        STEP,
        &[
            key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            key(PhysicalKeyCode::KeyD, ButtonState::Released),
            motion(500.0, 400.0)?,
            press(),
            key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            release(),
        ],
    )?;
    assert_eq!(probe(&runner)?.explicit_cancellations, [None, None]);
    assert!(events(&runner)?.is_empty());
    assert_eq!(probe(&runner)?.background_presses, 1);
    assert_eq!(probe(&runner)?.background_releases, 1);
    Ok(())
}

#[test]
fn missing_or_outside_press_positions_cannot_be_overridden_by_a_caller_target() -> LogicResult {
    let mut runner = runner(true)?;
    advance(
        &mut runner,
        STEP,
        &[press(), motion(20.0, 30.0)?, release()],
    )?;
    assert!(events(&runner)?.is_empty());
    for (x, y) in [(-1.0, 30.0), (800.0, 30.0), (20.0, -1.0), (20.0, 600.0)] {
        advance(
            &mut runner,
            STEP,
            &[motion(x, y)?, press(), motion(20.0, 30.0)?, release()],
        )?;
        assert!(
            frame(&runner)?
                .iter()
                .all(|entry| !entry.claimed && entry.event.is_none())
        );
    }
    assert!(events(&runner)?.is_empty());
    advance(&mut runner, STEP, &[press(), release()])?;
    assert_eq!(
        events(&runner)?,
        [
            (EventKind::Pressed, Target::First),
            (EventKind::Clicked, Target::First),
        ]
    );
    Ok(())
}

#[test]
fn outside_release_is_cancelled_even_when_the_caller_returns_the_captured_target() -> LogicResult {
    let mut runner = runner(true)?;
    for (x, y) in [(-1.0, 30.0), (800.0, 30.0), (20.0, -1.0), (20.0, 600.0)] {
        advance(
            &mut runner,
            STEP,
            &[motion(20.0, 30.0)?, press(), motion(x, y)?, release()],
        )?;
        let release = frame(&runner)?[1];
        assert!(release.claimed);
        assert_eq!(
            release.event.ok_or("cancellation missing")?.kind,
            EventKind::ReleasedOutside
        );
        assert_eq!(release.captured, None);
    }
    Ok(())
}

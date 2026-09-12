use std::time::Duration;

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn zero_tick_gesture_clicks_once_and_fixed_retention_remains_independently_visible() -> LogicResult
{
    let mut runner = runner(false)?;
    advance(&mut runner, Duration::ZERO, &[motion(20.0, 30.0)?, press()])?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    advance(&mut runner, Duration::ZERO, &[release()])?;
    assert!(probe(&runner)?.fixed.is_empty());
    let expected = events(&runner)?;
    assert_eq!(
        expected,
        [
            (EventKind::Pressed, Target::First),
            (EventKind::Clicked, Target::First),
        ]
    );
    let press = probe(&runner)?.frames[0][0].edge;
    let release = probe(&runner)?.frames[2][0].edge;
    let report = advance(&mut runner, STEP * 3, &[])?;
    assert_eq!(report.fixed_ticks_attempted(), 3);
    assert_eq!(probe(&runner)?.fixed[0], [press, release]);
    assert!(probe(&runner)?.fixed[1..].iter().all(Vec::is_empty));
    assert_eq!(events(&runner)?, expected);
    assert!(frame(&runner)?.is_empty());
    Ok(())
}

#[test]
fn frame_claims_do_not_consume_fixed_edges_that_ran_before_the_ui() -> LogicResult {
    let mut runner = runner(false)?;
    advance(
        &mut runner,
        STEP,
        &[motion(20.0, 30.0)?, press(), release()],
    )?;
    assert!(frame(&runner)?.iter().all(|entry| entry.claimed));
    assert_eq!(probe(&runner)?.fixed[0].len(), 2);
    assert_eq!(probe(&runner)?.fixed[0][0], frame(&runner)?[0].edge);
    assert_eq!(probe(&runner)?.fixed[0][1], frame(&runner)?[1].edge);
    assert_eq!(probe(&runner)?.background_presses, 0);
    assert_eq!(probe(&runner)?.background_releases, 0);
    Ok(())
}

#[test]
fn pause_preserves_frame_capture_and_cancellation_without_replaying_fixed_input() -> LogicResult {
    let mut runner = runner(false)?;
    advance(&mut runner, Duration::ZERO, &[motion(20.0, 30.0)?, press()])?;
    runner.set_paused(true);
    advance(&mut runner, STEP * 3, &[release()])?;
    assert_eq!(
        frame(&runner)?[0].event.ok_or("paused click missing")?.kind,
        EventKind::Clicked
    );
    advance(&mut runner, STEP, &[press(), InputEvent::FocusLost])?;
    assert_eq!(
        frame(&runner)?[1]
            .event
            .ok_or("paused cancellation missing")?
            .kind,
        EventKind::InputCancelled(InputCancellationReason::FocusLost)
    );
    assert_eq!(captured(&runner)?, None);
    assert!(probe(&runner)?.fixed.is_empty());
    let expected = events(&runner)?;
    runner.set_paused(false);
    advance(&mut runner, STEP, &[])?;
    assert_eq!(probe(&runner)?.fixed.len(), 1);
    assert!(probe(&runner)?.fixed[0].is_empty());
    assert_eq!(events(&runner)?, expected);
    Ok(())
}

#[test]
fn world_local_capture_is_recreated_and_an_old_held_release_cannot_click_a_reused_target()
-> LogicResult {
    let mut app = application()?;
    let target = register_world(&mut app, "after-replacement", false)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>, mut commands: Commands| -> LogicResult {
            if let Some(edge) = input.pressed(TestAction::Replace).next() {
                commands.replace_world(edge.intent(), target)?;
            }
            Ok(())
        },
    );
    let initial = register_world(&mut app, "before-replacement", false)?;
    let mut runner = app.build_headless(initial)?;
    let generation = runner.world_generation();
    advance(&mut runner, Duration::ZERO, &[motion(20.0, 30.0)?, press()])?;
    assert_eq!(captured(&runner)?, Some(Target::First));
    let replacement = advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Escape, ButtonState::Pressed)],
    )?;
    assert!(
        matches!(replacement.transition(), FrameTransition::Committed { target: committed, .. } if *committed == target)
    );
    assert_ne!(runner.world_generation(), generation);
    assert_eq!(captured(&runner)?, None);
    assert_eq!(events(&runner)?, [(EventKind::Pressed, Target::First)]);

    advance(&mut runner, STEP, &[release()])?;
    let unmatched = frame(&runner)?[0];
    assert!(!unmatched.claimed);
    assert_eq!(unmatched.event, None);
    assert_eq!(captured(&runner)?, None);
    assert_eq!(probe(&runner)?.fixed[0], [unmatched.edge]);
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

struct PersistentInteraction {
    button: PointerButton<Target>,
    cancelled_at_boundary: Option<Target>,
    releases: Vec<(bool, bool)>,
}

#[test]
fn application_owned_capture_can_suppress_the_old_release_across_world_replacement() -> LogicResult
{
    let mut app = application()?;
    app.register_app_resource(PersistentInteraction {
        button: PointerButton::new(MouseButton::Left),
        cancelled_at_boundary: None,
        releases: Vec::new(),
    })?;
    let target = register_world(&mut app, "persistent-after", false)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<TestAction>,
              mut persistent: AppResMut<PersistentInteraction>,
              mut commands: Commands|
              -> LogicResult {
            for edge in input.edges() {
                if edge.action() == TestAction::Replace && edge.state() == ButtonState::Pressed {
                    persistent.cancelled_at_boundary = persistent.button.cancel();
                    commands.replace_world(edge.intent(), target)?;
                }
                let outcome = persistent.button.process(edge, Some(Target::First));
                if edge.control() == InputControl::MouseButton(MouseButton::Left)
                    && edge.state() == ButtonState::Released
                {
                    persistent.releases.push((
                        outcome.claimed(),
                        matches!(outcome.event(), Some(PointerButtonEvent::Clicked { .. })),
                    ));
                }
            }
            Ok(())
        },
    );
    let initial = register_world(&mut app, "persistent-before", false)?;
    let mut runner = app.build_headless(initial)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            motion(20.0, 30.0)?,
            press(),
            key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        ],
    )?;
    assert_eq!(runner.active_world_name(), "persistent-after");
    let persistent = runner
        .app_resource::<PersistentInteraction>()
        .ok_or("persistent interaction missing")?;
    assert_eq!(persistent.cancelled_at_boundary, Some(Target::First));
    assert_eq!(persistent.button.captured(), None);
    advance(&mut runner, STEP, &[release(), press(), release()])?;
    let persistent = runner
        .app_resource::<PersistentInteraction>()
        .ok_or("persistent interaction missing")?;
    assert_eq!(persistent.releases, [(true, false), (true, true)]);
    Ok(())
}

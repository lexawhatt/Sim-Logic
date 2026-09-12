use std::time::Duration;

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn zero_tick_cancellation_reaches_fixed_once_with_the_same_metadata_and_token() -> LogicResult {
    let mut runner = runner(16, false)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    let press = last_frame(&runner)?.presses[0];
    advance(&mut runner, Duration::ZERO, &[InputEvent::FocusLost])?;
    let cancellation = last_frame(&runner)?.releases[0];
    assert_ne!(press.intent(), cancellation.intent());
    assert_eq!(
        cancellation.cancellation_reason(),
        Some(InputCancellationReason::FocusLost)
    );
    assert!(probe(&runner)?.fixed.is_empty());

    let report = advance(&mut runner, STEP * 3, &[])?;
    assert_eq!(report.fixed_ticks_attempted(), 3);
    let fixed = &probe(&runner)?.fixed;
    assert_eq!(fixed.len(), 3);
    assert_eq!(fixed[0].presses, [press]);
    assert_eq!(fixed[0].releases, [cancellation]);
    assert!(
        fixed
            .iter()
            .all(|snapshot| !snapshot.held && snapshot.pointer.is_none())
    );
    assert!(
        fixed[1..]
            .iter()
            .all(|snapshot| snapshot.presses.is_empty() && snapshot.releases.is_empty())
    );
    assert!(last_frame(&runner)?.presses.is_empty());
    assert!(last_frame(&runner)?.releases.is_empty());
    Ok(())
}

#[test]
fn paused_cancellation_is_frame_visible_but_not_replayed_when_fixed_resumes() -> LogicResult {
    let mut runner = runner(16, false)?;
    advance(
        &mut runner,
        STEP,
        &[
            InputEvent::pointer_moved(sample(60.0)?),
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::KeyW, ButtonState::Pressed)],
    )?;
    let fixed_count = probe(&runner)?.fixed.len();
    runner.set_paused(true);
    advance(&mut runner, Duration::ZERO, &[InputEvent::FocusLost])?;
    let frame = last_frame(&runner)?;
    assert_eq!(frame.releases.len(), 3);
    assert!(
        frame
            .releases
            .iter()
            .all(|edge| edge.cancellation_reason() == Some(InputCancellationReason::FocusLost))
    );
    assert!(!frame.held);
    assert_eq!(frame.pointer, None);
    assert_eq!(probe(&runner)?.fixed.len(), fixed_count);
    let paused = advance(&mut runner, STEP * 3, &[])?;
    assert_eq!(paused.fixed_ticks_attempted(), 0);
    assert!(last_frame(&runner)?.releases.is_empty());

    runner.set_paused(false);
    advance(&mut runner, STEP, &[])?;
    assert_eq!(probe(&runner)?.fixed.len(), fixed_count + 1);
    let fixed = probe(&runner)?
        .fixed
        .last()
        .ok_or("resumed fixed observation")?;
    assert!(fixed.presses.is_empty());
    assert!(fixed.releases.is_empty());
    assert!(!fixed.held);
    assert_eq!(fixed.pointer, None);
    Ok(())
}

#[derive(Default)]
struct SavedIntent(Option<TransitionIntentToken>);

#[derive(Resource)]
struct Route {
    target: WorldFactoryId,
    replay: bool,
}

fn replace_on_cancellation_or_replay(
    input: FrameInput<TestAction>,
    route: Option<Res<Route>>,
    mut saved: AppResMut<SavedIntent>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    let Some(route) = route else {
        return Ok(());
    };
    if route.replay {
        if let Some(intent) = saved.0.take() {
            commands.replace_world(intent, route.target)?;
        }
    } else if let Some(edge) = input
        .released(TestAction::Shared)
        .find(|edge| edge.cancellation_reason() == Some(InputCancellationReason::FocusLost))
    {
        saved.0 = Some(edge.intent());
        commands.replace_world(edge.intent(), route.target)?;
    }
    Ok(())
}

#[test]
fn cancellation_intent_can_replace_world_but_cannot_be_revived_after_replacement() -> LogicResult {
    let mut application = application(16, false)?;
    application.register_app_resource(SavedIntent::default())?;
    application.add_fallible_frame_system(replace_on_cancellation_or_replay);
    let third = register_world(&mut application, "must-not-reach")?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let second = application.register_world("after-cancellation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Route {
            target: third,
            replay: true,
        })?;
        Ok(())
    })?;
    let first = application.register_world("before-cancellation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Route {
            target: second,
            replay: false,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(first)?;
    let old_generation = runner.world_generation();
    advance(
        &mut runner,
        STEP,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    let replacement = advance(&mut runner, Duration::ZERO, &[InputEvent::FocusLost])?;
    assert!(
        matches!(replacement.transition(), FrameTransition::Committed { target, .. } if *target == second)
    );
    let cancellation = last_frame(&runner)?.releases[0];
    assert_eq!(
        cancellation.control(),
        InputControl::Key(PhysicalKeyCode::Space)
    );
    assert_eq!(
        cancellation.cancellation_reason(),
        Some(InputCancellationReason::FocusLost)
    );
    let new_generation = runner.world_generation();
    assert_ne!(old_generation, new_generation);

    let replay = advance(&mut runner, Duration::ZERO, &[])?;
    assert!(matches!(
        replay.transition(),
        FrameTransition::Invalid(TransitionRequestFailure::InvalidIntent)
    ));
    assert_eq!(runner.world_generation(), new_generation);
    assert_eq!(runner.active_world_name(), "after-cancellation");
    assert!(last_frame(&runner)?.presses.is_empty());
    assert!(last_frame(&runner)?.releases.is_empty());
    advance(&mut runner, STEP, &[])?;
    let fixed = probe(&runner)?
        .fixed
        .last()
        .ok_or("new world fixed observation")?;
    assert!(fixed.presses.is_empty());
    assert!(
        fixed.releases.is_empty(),
        "old generation cancellation must not be delivered in the new world"
    );

    advance(
        &mut runner,
        STEP,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    let fresh = last_frame(&runner)?.presses[0];
    assert_ne!(fresh.intent(), cancellation.intent());
    assert!(!fresh.is_cancelled());
    assert_eq!(runner.world_generation(), new_generation);
    Ok(())
}

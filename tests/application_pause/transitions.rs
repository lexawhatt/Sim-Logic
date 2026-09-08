use std::{error::Error, time::Duration};

use sim_logic::{prelude::*, transition::TransitionRejection};

use super::support::*;

#[derive(Debug, Clone, Copy)]
enum TransitionCase {
    Commit,
    FactoryFailure,
    StartupFailure,
    Invalid,
    Conflict,
}

#[derive(Resource)]
struct PauseDuringStartup;

fn targets(
    application: &mut Application<TestAction>,
    case: TransitionCase,
) -> Result<(WorldFactoryId, Option<WorldFactoryId>), Box<dyn Error>> {
    let target = match case {
        TransitionCase::Commit | TransitionCase::Conflict => {
            register_world(application, "target", 100.0)?
        }
        TransitionCase::FactoryFailure => application.register_world("factory-fails", |_| {
            Err(WorldBuildError::user("deliberate candidate failure"))
        })?,
        TransitionCase::StartupFailure => {
            application.add_fallible_system(
                Stage::Startup,
                |pause: Option<Res<PauseDuringStartup>>, mut commands: Commands| {
                    if pause.is_some() {
                        commands.set_paused(false)?;
                    }
                    Ok::<_, CommandEnqueueError>(())
                },
            );
            let camera = ActiveCamera2d::centered(20.0)?;
            application.register_world("startup-fails", move |world| {
                world.spawn(camera)?;
                world.insert_resource(PauseDuringStartup)?;
                Ok(())
            })?
        }
        TransitionCase::Invalid => {
            let mut foreign = Application::<TestAction>::new(AppConfig::default())?;
            register_world(&mut foreign, "foreign-target", 100.0)?
        }
    };
    let alternate = if matches!(case, TransitionCase::Conflict) {
        Some(register_world(application, "conflicting-target", 200.0)?)
    } else {
        None
    };
    Ok((target, alternate))
}

fn queue_replacement(
    commands: &mut Commands,
    intent: TransitionIntentToken,
    target: WorldFactoryId,
    alternate: Option<WorldFactoryId>,
) -> Result<(), CommandEnqueueError> {
    commands.replace_world(intent, target)?;
    if let Some(alternate) = alternate {
        let independent = commands.new_transition_intent()?;
        commands.replace_world(independent, alternate)?;
    }
    Ok(())
}

fn assert_transition(report: &LogicFrameReport, case: TransitionCase, target: WorldFactoryId) {
    assert!(
        report.failure().is_none(),
        "{case:?}: {:?}",
        report.failure()
    );
    match (case, report.transition()) {
        (TransitionCase::Commit, FrameTransition::Committed { target: actual, .. }) => {
            assert_eq!(*actual, target);
        }
        (
            TransitionCase::FactoryFailure,
            FrameTransition::PreparationFailed {
                target: actual,
                error: CandidateFailure::Factory(_),
            },
        ) => assert_eq!(*actual, target),
        (
            TransitionCase::StartupFailure,
            FrameTransition::PreparationFailed {
                target: actual,
                error: CandidateFailure::StartupSystem(error),
            },
        ) => {
            assert_eq!(*actual, target);
            assert!(
                error
                    .reason()
                    .contains(&CommandEnqueueError::PauseUnavailable.to_string())
            );
        }
        (
            TransitionCase::Invalid,
            FrameTransition::Invalid(TransitionRequestFailure::InvalidFactory),
        )
        | (TransitionCase::Conflict, FrameTransition::Rejected(TransitionRejection::Conflict)) => {}
        (_, transition) => panic!("unexpected {case:?} transition: {transition:?}"),
    }
}

#[test]
fn pause_commits_after_transition_validation_and_survives_every_transition_outcome()
-> Result<(), Box<dyn Error>> {
    for stage in [Stage::FixedUpdate, Stage::FrameUpdate] {
        for case in [
            TransitionCase::Commit,
            TransitionCase::FactoryFailure,
            TransitionCase::StartupFailure,
            TransitionCase::Invalid,
            TransitionCase::Conflict,
        ] {
            let mut application = application()?;
            let (target, alternate) = targets(&mut application, case)?;
            if stage == Stage::FixedUpdate {
                application.add_fallible_fixed_system(
                    move |input: FixedInput<TestAction>, mut commands: Commands| {
                        if let Some(edge) = input.pressed(TestAction::Replace).next() {
                            commands.set_paused(true)?;
                            queue_replacement(&mut commands, edge.intent(), target, alternate)?;
                        }
                        Ok::<_, CommandEnqueueError>(())
                    },
                );
            } else {
                application.add_fallible_frame_system(
                    move |input: FrameInput<TestAction>, mut commands: Commands| {
                        if let Some(edge) = input.pressed(TestAction::Replace).next() {
                            commands.set_paused(true)?;
                            queue_replacement(&mut commands, edge.intent(), target, alternate)?;
                        }
                        Ok::<_, CommandEnqueueError>(())
                    },
                );
            }
            application.add_fixed_system(observe_fixed);
            application.add_frame_system(observe_frame);
            application.add_fallible_frame_system(
                |input: FrameInput<TestAction>, time: FrameTime, mut commands: Commands| {
                    if input.has_press_occurrence(TestAction::Pause) {
                        commands.set_paused(!time.is_paused())?;
                    }
                    Ok::<_, CommandEnqueueError>(())
                },
            );
            let initial = register_world(&mut application, "source", 0.0)?;
            let mut runner = application.build_headless(initial)?;
            let source_generation = runner.world_generation();

            let report = advance(
                &mut runner,
                FIXED_STEP * 4,
                &[
                    InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
                    InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
                ],
            )?;
            assert_transition(&report, case, target);
            assert!(runner.is_paused());
            assert_eq!(
                report.fixed_ticks_attempted(),
                if stage == Stage::FixedUpdate { 1 } else { 4 }
            );
            assert_eq!(
                probe(&runner)?.frame_pause_states.len(),
                usize::from(stage == Stage::FrameUpdate)
            );
            if matches!(case, TransitionCase::Commit) {
                assert_ne!(runner.world_generation(), source_generation);
                assert_eq!(
                    report.extracted_generation(),
                    Some(runner.world_generation())
                );
                assert_eq!(extracted_x(&runner)?, 100.0);
            } else {
                assert_eq!(runner.world_generation(), source_generation);
            }
            if stage == Stage::FixedUpdate
                && matches!(case, TransitionCase::Invalid | TransitionCase::Conflict)
            {
                assert_eq!(
                    report
                        .dropped_for_transition()
                        .map(|dropped| dropped.ticks()),
                    Some(3)
                );
            }

            runner.set_paused(false);
            let continued = advance(&mut runner, Duration::ZERO, &[])?;
            assert!(continued.failure().is_none());
            assert_eq!(continued.fixed_ticks_attempted(), 0);
            assert!(
                !runner.is_paused(),
                "the skipped frame pause press must not replay"
            );
            assert!(matches!(continued.transition(), FrameTransition::None));
        }
    }
    Ok(())
}

#[test]
fn resume_with_retained_ticks_extracts_after_invalid_or_conflicting_frame_replacement()
-> Result<(), Box<dyn Error>> {
    for case in [
        TransitionCase::Commit,
        TransitionCase::FactoryFailure,
        TransitionCase::Invalid,
        TransitionCase::Conflict,
    ] {
        let mut application = application()?;
        let (target, alternate) = targets(&mut application, case)?;
        application.add_fallible_fixed_system(|time: FixedTime, mut commands: Commands| {
            if time.tick_index() == 0 {
                commands.set_paused(true)?;
            }
            Ok::<_, CommandEnqueueError>(())
        });
        application.add_fallible_fixed_system(move_visuals);
        application.add_fixed_system(observe_fixed);
        application.add_frame_system(observe_frame);
        application.add_fallible_frame_system(
            move |input: FrameInput<TestAction>, mut commands: Commands| {
                if let Some(edge) = input.pressed(TestAction::Pause).next() {
                    commands.set_paused(false)?;
                    queue_replacement(&mut commands, edge.intent(), target, alternate)?;
                }
                Ok::<_, CommandEnqueueError>(())
            },
        );
        let initial = register_world(&mut application, "frozen-source", 0.0)?;
        let mut runner = application.build_headless(initial)?;
        let paused = advance(&mut runner, FIXED_STEP * 4, &[])?;
        assert!(paused.failure().is_none());
        assert!(runner.is_paused());

        let resumed = advance(
            &mut runner,
            Duration::from_secs(1),
            &[InputEvent::key(
                PhysicalKeyCode::Space,
                ButtonState::Pressed,
            )],
        )?;
        assert_transition(&resumed, case, target);
        assert!(!runner.is_paused());
        assert_eq!(resumed.fixed_ticks_attempted(), 0);
        assert_eq!(
            resumed.extracted_generation(),
            Some(runner.world_generation())
        );
        assert_eq!(
            extracted_x(&runner)?,
            if matches!(case, TransitionCase::Commit) {
                100.0
            } else {
                1.0
            }
        );

        let next = advance(&mut runner, Duration::ZERO, &[])?;
        assert!(next.failure().is_none());
        assert_eq!(
            next.fixed_ticks_attempted(),
            if matches!(case, TransitionCase::Invalid | TransitionCase::Conflict) {
                3
            } else {
                0
            }
        );
        assert_eq!(probe(&runner)?.fixed_pause_presses, 0);
    }
    Ok(())
}

#[test]
fn exit_suppresses_pause_and_replacement_while_preserving_the_pre_batch_clock_state()
-> Result<(), Box<dyn Error>> {
    for (stage, initially_paused) in [
        (Stage::FixedUpdate, false),
        (Stage::FrameUpdate, false),
        (Stage::FrameUpdate, true),
    ] {
        let mut application = application()?;
        let target = register_world(&mut application, "exit-target", 100.0)?;
        let request = move |commands: &mut Commands| -> Result<(), CommandEnqueueError> {
            commands.set_paused(!initially_paused)?;
            let intent = commands.new_transition_intent()?;
            commands.replace_world(intent, target)?;
            commands.request_exit()
        };
        if stage == Stage::FixedUpdate {
            application.add_fallible_fixed_system(
                move |input: FixedInput<TestAction>, mut commands: Commands| {
                    if input.has_press_occurrence(TestAction::Replace) {
                        request(&mut commands)?;
                    }
                    Ok::<_, CommandEnqueueError>(())
                },
            );
        } else {
            application.add_fallible_frame_system(
                move |input: FrameInput<TestAction>, mut commands: Commands| {
                    if input.has_press_occurrence(TestAction::Replace) {
                        request(&mut commands)?;
                    }
                    Ok::<_, CommandEnqueueError>(())
                },
            );
        }
        application.add_fixed_system(observe_fixed);
        let initial = register_world(&mut application, "exit-source", 0.0)?;
        let mut runner = application.build_headless(initial)?;
        runner.set_paused(initially_paused);
        let source_generation = runner.world_generation();

        let exited = advance(
            &mut runner,
            FIXED_STEP * 4,
            &[
                InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
                InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            ],
        )?;
        assert!(exited.failure().is_none());
        assert!(exited.exit_requested());
        assert_eq!(exited.transitions_suppressed_by_exit(), 1);
        assert!(matches!(exited.transition(), FrameTransition::None));
        assert_eq!(runner.world_generation(), source_generation);
        assert_eq!(runner.is_paused(), initially_paused);

        let continued = advance(&mut runner, Duration::ZERO, &[])?;
        assert!(continued.failure().is_none());
        assert!(!continued.exit_requested());
        assert_eq!(continued.fixed_ticks_attempted(), 0);
        assert_eq!(runner.is_paused(), initially_paused);
        let previous_presses = probe(&runner)?.fixed_pause_presses;
        runner.set_paused(false);
        let next_tick = advance(&mut runner, FIXED_STEP, &[])?;
        assert!(next_tick.failure().is_none());
        assert_eq!(next_tick.fixed_ticks_attempted(), 1);
        assert_eq!(probe(&runner)?.fixed_pause_presses, previous_presses);
    }
    Ok(())
}

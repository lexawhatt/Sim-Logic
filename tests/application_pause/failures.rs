use std::{any::type_name, error::Error, time::Duration};

use sim_logic::{commands::CommandBatchError, prelude::*};

use super::support::*;

#[derive(Debug, Clone, Copy)]
enum Rejection {
    System,
    Events,
    CommandLimit,
    Structure,
}

#[derive(Resource)]
struct BatchState {
    target: LogicEntity,
    attempted: bool,
    later_runs: usize,
}

#[derive(Clone, Copy)]
struct TestEvent;

#[test]
fn failed_stages_discard_pause_and_structural_prefixes_from_either_clock_state()
-> Result<(), Box<dyn Error>> {
    for stage in [Stage::FixedUpdate, Stage::FrameUpdate] {
        for initially_paused in [false, true] {
            if stage == Stage::FixedUpdate && initially_paused {
                continue;
            }
            for rejection in [
                Rejection::System,
                Rejection::Events,
                Rejection::CommandLimit,
                Rejection::Structure,
            ] {
                let mut config = AppConfig::default();
                config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
                config.set_event_limit(1)?;
                if matches!(rejection, Rejection::CommandLimit) {
                    config.set_command_limit(2)?;
                }
                let mut application = Application::<TestAction>::new(config)?;
                application.approve_component::<Marker>()?;
                application.add_fallible_system(
                    stage,
                    move |mut state: ResMut<BatchState>,
                          mut events: EventWriter<TestEvent>,
                          mut commands: Commands|
                          -> LogicResult {
                        if state.attempted {
                            return Ok(());
                        }
                        state.attempted = true;
                        commands.set_paused(!initially_paused)?;
                        commands.spawn(Marker)?;
                        match rejection {
                            Rejection::System => return Err("deliberate pause failure".into()),
                            Rejection::Events => {
                                events.send(TestEvent)?;
                                assert!(matches!(
                                    events.send(TestEvent),
                                    Err(EventSendError::LimitExceeded { limit: 1, .. })
                                ));
                            }
                            Rejection::CommandLimit => {
                                assert_eq!(
                                    commands.set_paused(!initially_paused),
                                    Err(CommandEnqueueError::LimitExceeded { limit: 2 })
                                );
                            }
                            Rejection::Structure => {
                                commands.despawn(state.target)?;
                                commands.despawn(state.target)?;
                            }
                        }
                        Ok(())
                    },
                );
                application.add_system(stage, |mut state: ResMut<BatchState>| {
                    state.later_runs += 1;
                });
                let camera = ActiveCamera2d::centered(20.0)?;
                let initial = application.register_world("rejected-pause", move |world| {
                    world.spawn(camera)?;
                    let target = world.spawn(Marker)?;
                    world.insert_resource(BatchState {
                        target,
                        attempted: false,
                        later_runs: 0,
                    })?;
                    Ok(())
                })?;
                let mut runner = application.build_headless(initial)?;
                runner.set_paused(initially_paused);
                let target = runner.resource::<BatchState>().ok_or("batch state")?.target;

                let failed = advance(&mut runner, FIXED_STEP, &[])?;
                match (rejection, failed.failure()) {
                    (
                        Rejection::System,
                        Some(FrameFailure::System {
                            stage: actual,
                            error,
                        }),
                    ) => {
                        assert_eq!(*actual, stage);
                        assert_eq!(
                            error.reason(),
                            "returned an error: deliberate pause failure"
                        );
                    }
                    (
                        Rejection::Events,
                        Some(FrameFailure::System {
                            stage: actual,
                            error,
                        }),
                    ) => {
                        assert_eq!(*actual, stage);
                        assert!(error.reason().contains(type_name::<TestEvent>()));
                        assert!(error.reason().contains("exceeded its limit of 1"));
                    }
                    (
                        Rejection::CommandLimit,
                        Some(FrameFailure::Commands {
                            stage: actual,
                            error: CommandBatchError::LimitExceeded { limit: 2 },
                        }),
                    ) => assert_eq!(*actual, stage),
                    (
                        Rejection::Structure,
                        Some(FrameFailure::Commands {
                            stage: actual,
                            error: CommandBatchError::DuplicateDespawn { entity },
                        }),
                    ) => {
                        assert_eq!(*actual, stage);
                        assert_eq!(*entity, target);
                    }
                    (_, failure) => {
                        panic!("unexpected {stage:?} {rejection:?} result: {failure:?}")
                    }
                }
                assert_eq!(runner.is_paused(), initially_paused);
                assert_eq!((failed.spawned(), failed.despawned()), (0, 0));
                assert!(runner.component::<Marker>(target).is_ok());
                assert_eq!(runner.components::<Marker>().count(), 1);
                let later_runs = runner
                    .resource::<BatchState>()
                    .ok_or("batch state")?
                    .later_runs;
                assert_eq!(
                    later_runs,
                    usize::from(matches!(
                        rejection,
                        Rejection::CommandLimit | Rejection::Structure
                    ))
                );

                let recovered = advance(&mut runner, FIXED_STEP, &[])?;
                assert!(recovered.failure().is_none());
                assert_eq!(runner.is_paused(), initially_paused);
                assert_eq!(
                    runner
                        .resource::<BatchState>()
                        .ok_or("batch state")?
                        .later_runs,
                    later_runs + 1
                );
            }
        }
    }
    Ok(())
}

#[test]
fn startup_pause_is_unavailable_and_propagation_rejects_the_candidate() -> Result<(), Box<dyn Error>>
{
    for propagate in [false, true] {
        let mut config = AppConfig::default();
        config.set_command_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Marker>()?;
        application.add_fallible_system(
            Stage::Startup,
            move |mut commands: Commands| -> Result<(), CommandEnqueueError> {
                let error = commands
                    .set_paused(true)
                    .expect_err("isolated Startup cannot change the application clock");
                assert_eq!(error, CommandEnqueueError::PauseUnavailable);
                if propagate {
                    return Err(error);
                }
                commands.spawn(Marker)
            },
        );
        let initial = register_world(&mut application, "startup-pause", 0.0)?;
        match application.build_headless(initial) {
            Ok(mut runner) => {
                assert!(!propagate);
                assert!(!runner.is_paused());
                assert_eq!(runner.components::<Marker>().count(), 1);
                let report = advance(&mut runner, Duration::ZERO, &[])?;
                assert!(report.failure().is_none());
            }
            Err(RunnerBuildError::InitialWorld(CandidateFailure::StartupSystem(error))) => {
                assert!(propagate);
                assert_eq!(
                    error.reason(),
                    format!(
                        "returned an error: {}",
                        CommandEnqueueError::PauseUnavailable
                    )
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

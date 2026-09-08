//! Runtime regressions for events.

use super::*;

#[test]
fn typed_events_are_ordered_repeatable_and_scoped_to_one_stage_invocation()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    let mut application = Application::<TestAction>::new(config)?;
    let camera = camera()?;
    let world = application.register_world("event-world", move |world| {
        world.spawn(camera)?;
        world.insert_resource(EventObservation::default())?;
        Ok(())
    })?;

    application.add_system(Stage::Startup, send_startup_events);
    application.add_system(Stage::Startup, read_startup_events_first);
    application.add_system(Stage::Startup, read_startup_events_second);
    application.add_system(Stage::FixedUpdate, read_fixed_before_writer);
    application.add_system(Stage::FixedUpdate, send_fixed_event);
    application.add_system(Stage::FixedUpdate, read_fixed_after_writer);
    application.add_system(Stage::FrameUpdate, read_frame_events);

    let mut runner = application.build_headless(world)?;
    let startup = runner
        .resource::<EventObservation>()
        .ok_or("event observation should exist")?;
    assert_eq!(startup.startup_first, [3, 4]);
    assert_eq!(startup.startup_second, [3, 4]);
    assert_eq!(startup.secondary, [9]);

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(20),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 2);

    let observation = runner
        .resource::<EventObservation>()
        .ok_or("event observation should remain available")?;
    assert_eq!(observation.fixed_before_writer, [0, 0]);
    assert_eq!(observation.fixed_after_writer, [vec![0], vec![1]]);
    assert_eq!(observation.frame_counts, [0]);
    Ok(())
}

#[test]
fn ignored_event_limit_error_stops_stage_and_discards_commands() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_event_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;
    let camera = camera()?;
    let world = application.register_world("event-limit", move |world| {
        world.spawn(camera)?;
        world.insert_resource(EventFailureObservation::default())?;
        Ok(())
    })?;

    application.add_system(
        Stage::FixedUpdate,
        |mut observation: ResMut<EventFailureObservation>,
         mut events: EventWriter<PrimaryEvent>,
         mut secondary: EventWriter<SecondaryEvent>,
         mut commands: LogicCommands| {
            if observation.attempts > 0 {
                return;
            }
            observation.attempts += 1;
            assert!(commands.request_exit().is_ok());
            assert!(commands.spawn(Ephemeral).is_ok());
            assert!(events.send(PrimaryEvent(1)).is_ok());
            let first = events
                .send(PrimaryEvent(2))
                .expect_err("second primary event should exceed the limit");
            let repeated = secondary
                .send(SecondaryEvent(8))
                .expect_err("poisoned stage should reject another event type");
            assert!(matches!(
                first,
                EventSendError::LimitExceeded { event, limit: 1 }
                    if event == type_name::<PrimaryEvent>()
            ));
            assert_eq!(repeated, EventSendError::StageAlreadyFailed);
        },
    );
    application.add_system(
        Stage::FixedUpdate,
        |mut observation: ResMut<EventFailureObservation>| {
            observation.later_runs += 1;
        },
    );

    let mut runner = application.build_headless(world)?;
    let failed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(failed_report) = failed else {
        panic!("bounded frame should be accepted");
    };
    let Some(FrameFailure::System { stage, error }) = failed_report.failure() else {
        panic!("ignored event overflow should poison the fixed stage");
    };
    assert_eq!(*stage, Stage::FixedUpdate);
    assert!(error.reason().contains("exceeded its limit of 1"));
    assert!(error.reason().contains(type_name::<PrimaryEvent>()));
    assert!(!failed_report.exit_requested());
    assert_eq!(failed_report.spawned(), 0);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);
    let observation = runner
        .resource::<EventFailureObservation>()
        .ok_or("failure observation should exist")?;
    assert_eq!(observation.attempts, 1);
    assert_eq!(observation.later_runs, 0);

    let recovered = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(recovered_report) = recovered else {
        panic!("bounded recovery frame should be accepted");
    };
    assert!(recovered_report.failure().is_none());
    assert_eq!(
        runner
            .resource::<EventFailureObservation>()
            .ok_or("failure observation should remain available")?
            .later_runs,
        1
    );
    Ok(())
}

#[test]
fn startup_event_failure_discards_the_isolated_candidate() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_event_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    let camera = camera()?;
    let world = application.register_world("startup-event-limit", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, |mut events: EventWriter<PrimaryEvent>| {
        assert!(events.send(PrimaryEvent(1)).is_ok());
        let _ = events.send(PrimaryEvent(2));
    });

    let error = match application.build_headless(world) {
        Ok(_) => panic!("poisoned Startup should reject the candidate"),
        Err(error) => error,
    };
    let RunnerBuildError::InitialWorld(CandidateFailure::StartupSystem(error)) = error else {
        panic!("event failure should be reported as a Startup System failure");
    };
    assert!(error.reason().contains("exceeded its limit of 1"));
    Ok(())
}

#[test]
fn explicit_system_error_takes_priority_over_event_poison() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_event_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    let camera = camera()?;
    let world = application.register_world("event-error-priority", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_fallible_system(
        Stage::FixedUpdate,
        |mut events: EventWriter<PrimaryEvent>| -> Result<(), &'static str> {
            events
                .send(PrimaryEvent(1))
                .expect("first event should fit");
            let _ = events.send(PrimaryEvent(2));
            Err("explicit event system failure")
        },
    );

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    let Some(FrameFailure::System { error, .. }) = report.failure() else {
        panic!("fallible System should stop the fixed stage");
    };
    assert_eq!(
        error.reason(),
        "returned an error: explicit event system failure"
    );
    Ok(())
}

#[test]
fn frame_event_poison_stops_later_systems_and_discards_commands() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_event_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;
    let camera = camera()?;
    let world = application.register_world("frame-event-limit", move |world| {
        world.spawn(camera)?;
        world.insert_resource(EventFailureObservation::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |mut observation: ResMut<EventFailureObservation>,
         mut events: EventWriter<PrimaryEvent>,
         mut commands: LogicCommands| {
            if observation.attempts > 0 {
                return;
            }
            observation.attempts += 1;
            assert!(commands.spawn(Ephemeral).is_ok());
            assert!(events.send(PrimaryEvent(1)).is_ok());
            let _ = events.send(PrimaryEvent(2));
        },
    );
    application.add_system(
        Stage::FrameUpdate,
        |mut observation: ResMut<EventFailureObservation>| {
            observation.later_runs += 1;
        },
    );

    let mut runner = application.build_headless(world)?;
    let failed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(failed_report) = failed else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        failed_report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FrameUpdate,
            ..
        })
    ));
    assert!(!failed_report.exit_requested());
    assert_eq!(failed_report.spawned(), 0);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);
    assert_eq!(
        runner
            .resource::<EventFailureObservation>()
            .ok_or("frame failure observation should exist")?
            .later_runs,
        0
    );

    let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(recovered_report) = recovered else {
        panic!("bounded recovery frame should be accepted");
    };
    assert!(recovered_report.failure().is_none());
    assert_eq!(
        runner
            .resource::<EventFailureObservation>()
            .ok_or("frame failure observation should remain available")?
            .later_runs,
        1
    );
    Ok(())
}

#[test]
fn event_poison_discards_transition_and_consumes_its_input_intent() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_event_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("event-poison-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("event-poison-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FixedUpdate,
        |input: FixedInput<TestAction>,
         next: Option<Res<NextWorld>>,
         mut events: EventWriter<PrimaryEvent>,
         mut commands: LogicCommands| {
            let Some(next) = next else {
                return;
            };
            for edge in input.pressed(TestAction::Enter) {
                assert!(commands.replace_world(edge.intent(), next.0).is_ok());
                assert!(events.send(PrimaryEvent(1)).is_ok());
                let _ = events.send(PrimaryEvent(2));
            }
        },
    );

    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();
    let pressed = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let failed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &pressed,
        viewport()?,
    ));
    let FrameOutcome::Advanced(failed_report) = failed else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        failed_report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    assert!(matches!(failed_report.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);

    let recovered = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(recovered_report) = recovered else {
        panic!("bounded recovery frame should be accepted");
    };
    assert!(recovered_report.failure().is_none());
    assert!(matches!(
        recovered_report.transition(),
        FrameTransition::None
    ));
    assert_eq!(runner.world_generation(), source_generation);
    Ok(())
}

#[test]
fn committed_world_starts_with_fresh_event_channels() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("fresh-event-target", move |world| {
        world.spawn(target_camera)?;
        world.insert_resource(StartupEventCount::default())?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("fresh-event-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::Startup,
        |events: EventReader<PrimaryEvent>, count: Option<ResMut<StartupEventCount>>| {
            if let Some(mut count) = count {
                count.0 = events.len();
            }
        },
    );
    application.add_system(
        Stage::FixedUpdate,
        |input: FixedInput<TestAction>,
         next: Option<Res<NextWorld>>,
         mut events: EventWriter<PrimaryEvent>,
         mut commands: LogicCommands| {
            let Some(next) = next else {
                return;
            };
            for edge in input.pressed(TestAction::Enter) {
                assert!(events.send(PrimaryEvent(7)).is_ok());
                assert!(commands.replace_world(edge.intent(), next.0).is_ok());
            }
        },
    );

    let mut runner = application.build_headless(source)?;
    let pressed = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &pressed,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(report.failure().is_none());
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_eq!(runner.active_world_name(), "fresh-event-target");
    assert_eq!(
        runner
            .resource::<StartupEventCount>()
            .ok_or("target startup observation should exist")?
            .0,
        0
    );
    Ok(())
}

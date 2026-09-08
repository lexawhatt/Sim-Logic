//! Runtime regressions for stage contracts.

use super::*;

#[test]
fn managed_time_parameters_expose_direct_f32_duration_conversion() -> Result<(), Box<dyn Error>> {
    let duration = Duration::new(0, 16_874_317);
    assert_ne!(
        duration.as_secs_f32().to_bits(),
        (duration.as_secs_f64() as f32).to_bits()
    );
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(duration, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    let camera = camera()?;
    let world = application.register_world("f32-time", move |world| {
        world.spawn(camera)?;
        world.insert_resource(FloatTimeObservation::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FixedUpdate,
        |time: FixedTime, mut observation: ResMut<FloatTimeObservation>| {
            observation.fixed_bits = Some(time.seconds_f32().to_bits());
        },
    );
    application.add_system(
        Stage::FrameUpdate,
        |time: FrameTime, mut observation: ResMut<FloatTimeObservation>| {
            observation.frame_bits = Some(time.seconds_f32().to_bits());
        },
    );

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(duration, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded f32-time frame should be accepted");
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    let observation = runner
        .resource::<FloatTimeObservation>()
        .ok_or("f32 time observation should remain available")?;
    let expected = duration.as_secs_f32().to_bits();
    assert_eq!(observation.fixed_bits, Some(expected));
    assert_eq!(observation.frame_bits, Some(expected));
    Ok(())
}

#[test]
fn startup_cannot_issue_application_control_commands_or_a_direct_intent()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.insert_resource(StartupIntentObservation {
            rejected_as_unavailable: false,
        })?;
        world.insert_resource(StartupExitObservation {
            rejected_as_unavailable: false,
        })?;
        world.insert_resource(StartupPauseObservation {
            rejected_as_unavailable: false,
        })?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, observe_startup_intent);
    application.add_system(Stage::Startup, observe_startup_exit);
    application.add_system(Stage::Startup, observe_startup_pause);

    let runner = application.build_headless(world)?;
    let observation = runner
        .resource::<StartupIntentObservation>()
        .ok_or("Startup observation should remain in the active World")?;
    assert!(observation.rejected_as_unavailable);
    let exit_observation = runner
        .resource::<StartupExitObservation>()
        .ok_or("Startup exit observation should remain in the active World")?;
    assert!(exit_observation.rejected_as_unavailable);
    let pause_observation = runner
        .resource::<StartupPauseObservation>()
        .ok_or("Startup pause observation should remain in the active World")?;
    assert!(pause_observation.rejected_as_unavailable);
    Ok(())
}

#[test]
fn startup_cannot_read_frame_input() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, startup_reads_frame_input);

    let result = application.build_headless(world);
    assert!(matches!(
        result,
        Err(super::RunnerBuildError::InitialWorld(
            super::CandidateFailure::StartupSystem(_)
        ))
    ));
    Ok(())
}

#[test]
fn startup_cannot_read_fixed_time() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, startup_reads_fixed_time);

    let result = application.build_headless(world);
    assert!(matches!(
        result,
        Err(super::RunnerBuildError::InitialWorld(
            super::CandidateFailure::StartupSystem(_)
        ))
    ));
    Ok(())
}

#[test]
fn update_stages_only_receive_their_own_input_snapshot() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.insert_resource(StageInputObservation::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, observe_fixed_stage_input);
    application.add_system(Stage::FrameUpdate, observe_frame_stage_input);

    let mut runner = application.build_headless(world)?;
    let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(report.failure().is_none());

    let observation = runner
        .resource::<StageInputObservation>()
        .ok_or("stage input observation should remain available")?;
    assert_eq!(observation.fixed_actual_edges, 1);
    assert_eq!(observation.fixed_foreign_edges, 0);
    assert!(!observation.fixed_foreign_held);
    assert_eq!(observation.frame_actual_edges, 1);
    assert_eq!(observation.frame_foreign_edges, 0);
    assert!(!observation.frame_foreign_held);
    Ok(())
}

#[test]
fn missing_frame_snapshot_fails_closed_without_replacement() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    assert!(
        runner
            .active
            .world
            .remove_resource::<FrameInputState<TestAction>>()
            .is_some()
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted before invariant validation");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    assert_eq!(report.fixed_ticks_attempted(), 0);
    assert!(
        !runner
            .active
            .world
            .contains_resource::<FrameInputState<TestAction>>()
    );
    Ok(())
}

#[test]
fn missing_direct_intent_issuer_fails_closed_before_update_and_extraction()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let generation = runner.active.generation;
    assert!(
        runner
            .active
            .world
            .remove_resource::<DirectIntentIssuer>()
            .is_some()
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted before invariant validation");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    assert_eq!(report.fixed_ticks_attempted(), 0);
    assert_eq!(report.extracted_generation(), None);
    assert_eq!(runner.active.generation, generation);
    Ok(())
}

#[test]
fn missing_command_queue_overrides_fixed_system_failure_as_runtime_invariant()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    let mut application = Application::<TestAction>::new(config)?;
    let camera = camera()?;
    let world = application.register_world("missing-fixed-commands", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FixedUpdate, || -> Result<(), &'static str> {
        Err("deliberate fixed failure")
    });
    let mut runner = application.build_headless(world)?;
    assert!(
        runner
            .active
            .world
            .remove_resource::<CommandQueue>()
            .is_some()
    );

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    assert_eq!(report.fixed_ticks_attempted(), 1);
    Ok(())
}

#[test]
fn missing_command_queue_overrides_frame_system_failure_as_runtime_invariant()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("missing-frame-commands", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FrameUpdate, || -> Result<(), &'static str> {
        Err("deliberate frame failure")
    });
    let mut runner = application.build_headless(world)?;
    assert!(
        runner
            .active
            .world
            .remove_resource::<CommandQueue>()
            .is_some()
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    assert_eq!(report.fixed_ticks_attempted(), 0);
    Ok(())
}

#[test]
fn missing_fixed_snapshot_preflights_frame_publication() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    assert!(
        runner
            .active
            .world
            .remove_resource::<FixedInputState<TestAction>>()
            .is_some()
    );
    let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted before invariant validation");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    let snapshot = runner
        .active
        .world
        .get_resource::<FrameInputState<TestAction>>()
        .ok_or("frame snapshot should still exist")?;
    assert!(!snapshot.held(TestAction::MoveRight));
    assert_eq!(snapshot.pressed(TestAction::MoveRight).count(), 0);
    Ok(())
}

#[test]
fn missing_fixed_snapshot_restores_interpolation_capture() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let transform = Transform2d::new(Vec2::new(3.0, 0.0))?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, transform, circle))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let entity = runner
        .components::<Ball>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("ball should be inspectable")?;
    runner
        .active
        .world
        .get_mut::<Transform2d>(entity.entity())
        .ok_or("ball transform should exist")?
        .set_translation(Vec2::new(7.0, 0.0))?;
    assert!(
        runner
            .active
            .world
            .remove_resource::<FixedInputState<TestAction>>()
            .is_some()
    );

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted before invariant validation");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::RuntimeInvariant)
    ));
    assert_eq!(report.fixed_ticks_attempted(), 1);
    let transform = runner
        .active
        .world
        .get::<Transform2d>(entity.entity())
        .ok_or("ball transform should survive failed delivery")?;
    assert_eq!(transform.previous_translation(), Vec2::new(3.0, 0.0));
    assert_eq!(transform.translation(), Vec2::new(7.0, 0.0));
    Ok(())
}

#[test]
fn fixed_update_cannot_read_frame_time() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, fixed_reads_frame_time);

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    Ok(())
}

#[test]
fn fixed_update_cannot_read_frame_viewport() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, fixed_reads_frame_viewport);

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    Ok(())
}

#[test]
fn frame_update_cannot_read_fixed_time() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FrameUpdate, frame_reads_fixed_time);

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FrameUpdate,
            ..
        })
    ));
    Ok(())
}

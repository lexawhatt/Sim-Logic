//! Runtime regressions for failures.

use super::*;

#[test]
fn fallible_fixed_system_stops_stage_without_rolling_back_direct_writes()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_command_limit(2)?;

    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ball>()?;
    application.approve_component::<Ephemeral>()?;

    let target_camera = camera()?;
    let target = application.register_world("fallible-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("fallible-source", move |world| {
        world.spawn(source_camera)?;
        world.spawn((Ball, Transform2d::default()))?;
        world.insert_resource(NextWorld(target))?;
        world.insert_resource(FallibleObservation::default())?;
        Ok(())
    })?;

    application.add_system(Stage::FixedUpdate, |mut commands: LogicCommands| {
        commands
            .spawn(Ephemeral)
            .expect("first System should queue within the stage limit");
    });

    application.add_fallible_system(
        Stage::FixedUpdate,
        |mut observation: ResMut<FallibleObservation>,
         next: Res<NextWorld>,
         mut balls: Query<&mut Transform2d, With<Ball>>,
         mut commands: LogicCommands|
         -> Result<(), CommandEnqueueError> {
            if observation.attempts > 0 {
                return Ok(());
            }
            observation.attempts += 1;
            for mut transform in &mut balls {
                transform
                    .translate_by(Vec2::new(2.0, 0.0))
                    .expect("finite test translation should remain valid");
            }

            let intent = commands.new_transition_intent()?;
            commands.replace_world(intent, next.0)?;
            commands.spawn(Ephemeral)?;
            Ok(())
        },
    );
    application.add_system(
        Stage::FixedUpdate,
        |mut observation: ResMut<FallibleObservation>| {
            observation.later_runs += 1;
        },
    );
    application.add_system(
        Stage::FrameUpdate,
        |mut observation: ResMut<FallibleObservation>| {
            observation.frame_runs += 1;
        },
    );

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let failed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(failed_report) = failed else {
        panic!("bounded frame should be accepted");
    };

    assert_eq!(failed_report.fixed_ticks_attempted(), 1);
    let Some(FrameFailure::System { stage, error }) = failed_report.failure() else {
        panic!("fallible fixed System should report a System failure");
    };
    assert_eq!(*stage, Stage::FixedUpdate);
    assert!(error.reason().starts_with("returned an error: "));
    assert!(error.reason().contains("command limit of 2"));
    assert_eq!(runner.world_generation(), old_generation);
    assert!(matches!(failed_report.transition(), FrameTransition::None));
    assert_eq!(failed_report.spawned(), 0);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);

    let observation = runner
        .resource::<FallibleObservation>()
        .ok_or("fallible observation should remain available")?;
    assert_eq!(observation.attempts, 1);
    assert_eq!(observation.later_runs, 0);
    assert_eq!(observation.frame_runs, 0);
    let (_, transform) = runner
        .components::<Transform2d>()
        .find(|(_, transform)| transform.translation() == Vec2::new(2.0, 0.0))
        .ok_or("direct Transform write should not be rolled back")?;
    assert_eq!(transform.previous_translation(), Vec2::ZERO);

    let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(recovered_report) = recovered else {
        panic!("bounded recovery frame should be accepted");
    };
    assert!(recovered_report.failure().is_none());
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(recovered_report.spawned(), 1);
    assert_eq!(runner.components::<Ephemeral>().count(), 1);
    let observation = runner
        .resource::<FallibleObservation>()
        .ok_or("fallible observation should remain available")?;
    assert!(observation.later_runs > 0);
    assert_eq!(observation.frame_runs, 1);
    Ok(())
}

#[test]
fn fallible_frame_system_discards_commands_and_can_run_again() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_command_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;

    let camera = camera()?;
    let world = application.register_world("fallible-frame", move |world| {
        world.spawn(camera)?;
        world.insert_resource(FallibleObservation::default())?;
        Ok(())
    })?;
    application.add_fallible_system(
        Stage::FrameUpdate,
        |mut observation: ResMut<FallibleObservation>,
         mut commands: LogicCommands|
         -> Result<(), CommandEnqueueError> {
            if observation.attempts > 0 {
                return Ok(());
            }
            observation.attempts += 1;
            commands.spawn(Ephemeral)?;
            commands.spawn(Ephemeral)?;
            Ok(())
        },
    );
    application.add_system(
        Stage::FrameUpdate,
        |mut observation: ResMut<FallibleObservation>| {
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
    assert_eq!(failed_report.extracted_generation(), None);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);
    let observation = runner
        .resource::<FallibleObservation>()
        .ok_or("fallible observation should remain available")?;
    assert_eq!(observation.attempts, 1);
    assert_eq!(observation.later_runs, 0);

    let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(recovered_report) = recovered else {
        panic!("bounded recovery frame should be accepted");
    };
    assert!(recovered_report.failure().is_none());
    assert_eq!(runner.components::<Ephemeral>().count(), 0);
    assert_eq!(
        runner
            .resource::<FallibleObservation>()
            .ok_or("fallible observation should remain available")?
            .later_runs,
        1
    );
    Ok(())
}

#[test]
fn fallible_startup_error_rejects_initial_world() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("rejected-initial", move |world| {
        world.spawn(camera)?;
        world.insert_resource(RejectStartup)?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::Startup, reject_marked_startup);

    let error = match application.build_headless(world) {
        Ok(_) => panic!("fallible Startup should reject the initial candidate"),
        Err(error) => error,
    };
    let super::RunnerBuildError::InitialWorld(super::CandidateFailure::StartupSystem(error)) =
        error
    else {
        panic!("initial build should retain the typed Startup failure path");
    };
    assert!(error.system().ends_with("reject_marked_startup"));
    assert_eq!(
        error.reason(),
        "returned an error: candidate startup rejected"
    );
    Ok(())
}

#[test]
fn fallible_startup_error_discards_replacement_candidate() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("rejected-target", move |world| {
        world.spawn(target_camera)?;
        world.insert_resource(RejectStartup)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("retained-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::Startup, reject_marked_startup);
    application.add_system(Stage::FixedUpdate, request_next_world);

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let events = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded transition frame should be accepted");
    };

    let FrameTransition::PreparationFailed {
        target: failed,
        error,
    } = report.transition()
    else {
        panic!("fallible Startup should reject only the isolated candidate");
    };
    assert_eq!(*failed, target);
    let super::CandidateFailure::StartupSystem(error) = error else {
        panic!("candidate should retain the typed Startup failure path");
    };
    assert!(
        error.system().ends_with("reject_marked_startup"),
        "unexpected System name: {}",
        error.system()
    );
    assert_eq!(
        error.reason(),
        "returned an error: candidate startup rejected"
    );
    assert!(report.failure().is_none());
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(runner.active_world_name(), "retained-source");
    Ok(())
}

#[test]
fn fixed_failure_restores_previous_endpoint_but_keeps_current_mutation()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let initial_transform = Transform2d::new(Vec2::new(1.0, 0.0))?;
    let world_a = application.register_world("world-a", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, initial_transform, ball))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);
    application.add_fallible_system(Stage::FixedUpdate, follow_ball);
    application.add_system(Stage::FixedUpdate, require_missing_fixed_resource);

    let mut runner = application.build_headless(world_a)?;

    let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));

    let Some((_, transform)) = runner.components::<Transform2d>().next() else {
        panic!("partially updated ball should remain alive");
    };
    assert_eq!(transform.previous_translation(), Vec2::new(1.0, 0.0));
    assert!(transform.translation().x() > 1.9);
    assert_eq!(transform.translation().y(), 0.0);
    let Some((_, camera)) = runner.components::<ActiveCamera2d>().next() else {
        panic!("partially updated camera should remain alive");
    };
    assert_eq!(camera.previous_center(), Vec2::ZERO);
    assert!(camera.center().x() > 1.9);
    assert_eq!(camera.center().y(), 0.0);
    Ok(())
}

#[test]
fn failed_fixed_tick_can_pause_extract_and_resume_retained_work() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_command_limit(1)?;

    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ball>()?;
    application.approve_component::<Ephemeral>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Transform2d::default(), ball))?;
        world.insert_resource(RejectSecondFixedBatch::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);
    application.add_fallible_system(Stage::FixedUpdate, follow_ball);
    application.add_system(Stage::FixedUpdate, reject_second_fixed_batch);

    let mut runner = application.build_headless(world)?;
    let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let first = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &movement,
        viewport()?,
    ));
    let FrameOutcome::Advanced(first_report) = first else {
        panic!("first bounded frame should be accepted");
    };
    assert_eq!(first_report.fixed_ticks_attempted(), 1);
    assert!(first_report.failure().is_none());

    let failed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(failed_report) = failed else {
        panic!("bounded frame should be accepted");
    };
    assert_eq!(failed_report.fixed_ticks_attempted(), 1);
    assert!(matches!(
        failed_report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    let (_, failed_camera) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("failed World should retain its camera")?;
    assert!((failed_camera.previous_center().x() - 0.0).abs() < 0.001);
    assert!((failed_camera.center().x() - 1.2).abs() < 0.001);

    runner.set_paused(true);
    let paused = runner.advance_frame(FrameRequest::new(Duration::from_secs(1), &[], viewport()?));
    let FrameOutcome::Advanced(paused_report) = paused else {
        panic!("paused frame should be accepted");
    };
    assert_eq!(paused_report.fixed_ticks_attempted(), 0);
    assert!(paused_report.failure().is_none());
    let paused_frame = runner
        .extracted_frame()
        .ok_or("paused frame should publish an extraction")?;
    let [paused_circle] = paused_frame.resolved_circles() else {
        panic!("paused World should extract exactly one circle");
    };
    assert!((paused_circle.position().x() - 1.2).abs() < 0.001);
    assert!((paused_frame.camera().center().x() - 1.2).abs() < 0.001);

    runner.set_paused(false);
    let resumed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(resumed_report) = resumed else {
        panic!("resumed frame should be accepted");
    };
    assert_eq!(resumed_report.fixed_ticks_attempted(), 1);
    assert!(resumed_report.failure().is_none());
    let resumed_frame = runner
        .extracted_frame()
        .ok_or("resumed frame should publish an extraction")?;
    let [resumed_circle] = resumed_frame.resolved_circles() else {
        panic!("resumed World should extract exactly one circle");
    };
    assert!((resumed_circle.position().x() - 1.5).abs() < 0.001);
    assert!((resumed_frame.camera().center().x() - 1.5).abs() < 0.001);

    let (_, transform) = runner
        .components::<Transform2d>()
        .next()
        .ok_or("resumed World should retain its transform")?;
    assert!((transform.previous_translation().x() - 1.2).abs() < 0.001);
    assert!((transform.translation().x() - 1.8).abs() < 0.001);
    Ok(())
}

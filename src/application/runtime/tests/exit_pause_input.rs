//! Runtime regressions for exit pause input.

use super::*;

#[test]
fn entering_pause_revokes_an_undelivered_fixed_transition_edge() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let camera_b = camera()?;
    let world_b = application.register_world("world-b", move |world| {
        world.spawn(camera_b)?;
        Ok(())
    })?;
    let camera_a = camera()?;
    let world_a = application.register_world("world-a", move |world| {
        world.spawn(camera_a)?;
        world.insert_resource(NextWorld(world_b))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, request_next_world);

    let mut runner = application.build_headless(world_a)?;
    let original_generation = runner.world_generation();
    let press = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &press, viewport()?));
    let FrameOutcome::Advanced(first) = first else {
        panic!("zero-tick press frame should be accepted");
    };
    assert_eq!(first.fixed_ticks_attempted(), 0);
    assert!(matches!(first.transition(), FrameTransition::None));

    runner.set_paused(true);
    let release = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Released,
    )];
    let paused = runner.advance_frame(FrameRequest::new(
        Duration::from_secs(1),
        &release,
        viewport()?,
    ));
    let FrameOutcome::Advanced(paused) = paused else {
        panic!("paused release frame should be accepted");
    };
    assert_eq!(paused.fixed_ticks_attempted(), 0);

    runner.set_paused(false);
    let resumed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(resumed) = resumed else {
        panic!("resumed frame should be accepted");
    };

    assert_eq!(resumed.fixed_ticks_attempted(), 1);
    assert!(matches!(resumed.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), original_generation);
    assert_eq!(runner.active_world_name(), "world-a");
    Ok(())
}

#[test]
fn fixed_exit_stops_catch_up_frame_update_and_extraction() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world = application.register_world("exit-world", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, ball))?;
        Ok(())
    })?;
    let fixed_runs = Arc::new(AtomicUsize::new(0));
    let observed_fixed_runs = Arc::clone(&fixed_runs);
    application.add_fallible_system(
        Stage::FixedUpdate,
        move |mut balls: Query<&mut Transform2d, With<Ball>>,
              mut commands: LogicCommands|
              -> Result<(), Box<dyn Error>> {
            observed_fixed_runs.fetch_add(1, Ordering::SeqCst);
            for mut transform in &mut balls {
                transform.translate_by(Vec2::new(2.0, 0.0))?;
            }
            commands.request_exit()?;
            Ok(())
        },
    );
    let later_fixed_runs = Arc::new(AtomicUsize::new(0));
    let observed_later_fixed_runs = Arc::clone(&later_fixed_runs);
    application.add_system(Stage::FixedUpdate, move || {
        observed_later_fixed_runs.fetch_add(1, Ordering::SeqCst);
    });
    let frame_runs = Arc::new(AtomicUsize::new(0));
    let observed_frame_runs = Arc::clone(&frame_runs);
    application.add_system(Stage::FrameUpdate, move || {
        observed_frame_runs.fetch_add(1, Ordering::SeqCst);
    });

    let mut runner = application.build_headless(world)?;
    let published_before = runner
        .extracted_frame()
        .ok_or("initial extraction should be published")?
        .world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(100),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded exit frame should be accepted");
    };

    assert!(report.exit_requested());
    assert_eq!(report.transitions_suppressed_by_exit(), 0);
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(fixed_runs.load(Ordering::SeqCst), 1);
    assert_eq!(later_fixed_runs.load(Ordering::SeqCst), 1);
    assert_eq!(frame_runs.load(Ordering::SeqCst), 0);
    assert_eq!(report.extracted_generation(), None);
    let canonical = runner
        .components::<Transform2d>()
        .find_map(|(_, transform)| (transform.translation().x() == 2.0).then_some(transform))
        .ok_or("exit tick should keep the moved canonical Transform")?;
    assert_eq!(canonical.previous_translation(), canonical.translation());
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("old extraction should remain diagnostic history")?
            .world_generation(),
        published_before
    );

    let ignored = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(ignored_report) = ignored else {
        panic!("a headless host may deliberately keep driving the runner");
    };
    assert!(!ignored_report.exit_requested());
    assert_eq!(ignored_report.fixed_ticks_attempted(), 0);
    assert_eq!(frame_runs.load(Ordering::SeqCst), 1);
    assert!(ignored_report.extracted_generation().is_some());
    let [circle] = runner
        .extracted_frame()
        .ok_or("continued headless frame should publish")?
        .resolved_circles()
    else {
        panic!("continued headless frame should retain one circle");
    };
    assert_eq!(circle.position(), Vec2::new(2.0, 0.0));
    Ok(())
}

#[test]
fn exit_suppresses_replacement_and_consumes_its_input_edge() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;
    let target_builds = Arc::new(AtomicUsize::new(0));
    let observed_target_builds = Arc::clone(&target_builds);
    let target_camera = camera()?;
    let target = application.register_world("suppressed-target", move |world| {
        observed_target_builds.fetch_add(1, Ordering::SeqCst);
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("exit-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, request_next_world);
    let first_tick = Arc::new(AtomicUsize::new(0));
    let observed_first_tick = Arc::clone(&first_tick);
    application.add_fallible_system(
        Stage::FixedUpdate,
        move |mut commands: LogicCommands| -> Result<(), CommandEnqueueError> {
            if observed_first_tick.fetch_add(1, Ordering::SeqCst) == 0 {
                commands.request_exit()?;
            }
            Ok(())
        },
    );

    let mut runner = application.build_headless(source)?;
    let generation = runner.world_generation();
    let lifecycle_len = runner.lifecycle().len();
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
        panic!("bounded exit frame should be accepted");
    };

    assert!(report.exit_requested());
    assert_eq!(report.transitions_suppressed_by_exit(), 1);
    assert!(matches!(report.transition(), FrameTransition::None));
    assert_eq!(target_builds.load(Ordering::SeqCst), 0);
    assert_eq!(runner.world_generation(), generation);
    assert_eq!(runner.lifecycle().len(), lifecycle_len);

    let next = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(next_report) = next else {
        panic!("headless host should be able to inspect a later frame");
    };
    assert!(!next_report.exit_requested());
    assert!(matches!(next_report.transition(), FrameTransition::None));
    assert_eq!(target_builds.load(Ordering::SeqCst), 0);
    assert_eq!(runner.world_generation(), generation);
    Ok(())
}

#[test]
fn paused_frame_exit_commits_structure_but_poison_discards_later_exit() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_command_limit(2)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;
    let camera = camera()?;
    let world = application.register_world("frame-exit", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let phase = Arc::new(AtomicUsize::new(0));
    let observed_phase = Arc::clone(&phase);
    application.add_system(Stage::FrameUpdate, move |mut commands: LogicCommands| {
        assert!(commands.request_exit().is_ok());
        assert!(commands.spawn(Ephemeral).is_ok());
        if observed_phase.fetch_add(1, Ordering::SeqCst) != 0 {
            assert!(matches!(
                commands.spawn(Ephemeral),
                Err(CommandEnqueueError::LimitExceeded { limit: 2 })
            ));
        }
    });

    let mut runner = application.build_headless(world)?;
    runner.set_paused(true);
    let first = runner.advance_frame(FrameRequest::new(Duration::from_secs(1), &[], viewport()?));
    let FrameOutcome::Advanced(first_report) = first else {
        panic!("paused exit frame should be accepted");
    };
    assert!(first_report.exit_requested());
    assert_eq!(first_report.fixed_ticks_attempted(), 0);
    assert_eq!(first_report.spawned(), 1);
    assert_eq!(first_report.extracted_generation(), None);
    assert_eq!(runner.components::<Ephemeral>().count(), 1);

    let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(second_report) = second else {
        panic!("poisoned headless frame should still complete diagnostically");
    };
    assert!(!second_report.exit_requested());
    assert_eq!(second_report.spawned(), 0);
    assert!(matches!(
        second_report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: crate::commands::CommandBatchError::LimitExceeded { limit: 2 },
        })
    ));
    assert_eq!(runner.components::<Ephemeral>().count(), 1);
    Ok(())
}

#[test]
fn frame_exit_discards_an_edge_not_yet_delivered_to_fixed_update() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;
    let camera = camera()?;
    let world = application.register_world("edge-exit", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let fixed_presses = Arc::new(AtomicUsize::new(0));
    let observed_fixed_presses = Arc::clone(&fixed_presses);
    application.add_system(Stage::FixedUpdate, move |input: FixedInput<TestAction>| {
        observed_fixed_presses
            .fetch_add(input.pressed(TestAction::Enter).count(), Ordering::SeqCst);
    });
    application.add_fallible_system(
        Stage::FrameUpdate,
        |input: FrameInput<TestAction>,
         mut commands: LogicCommands|
         -> Result<(), CommandEnqueueError> {
            if input.pressed(TestAction::Enter).next().is_some() {
                commands.request_exit()?;
            }
            Ok(())
        },
    );

    let mut runner = application.build_headless(world)?;
    let pressed = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let exit = runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?));
    let FrameOutcome::Advanced(exit_report) = exit else {
        panic!("zero-tick exit frame should be accepted");
    };
    assert!(exit_report.exit_requested());
    assert_eq!(fixed_presses.load(Ordering::SeqCst), 0);

    let continued = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(continued_report) = continued else {
        panic!("continued fixed frame should be accepted");
    };
    assert!(!continued_report.exit_requested());
    assert_eq!(continued_report.fixed_ticks_attempted(), 1);
    assert_eq!(fixed_presses.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn zero_tick_frames_reject_retained_fixed_edge_overflow_atomically() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_input_event_limit(2)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.insert_resource(RetainedEdgeObservation::default())?;
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, observe_retained_edges);

    let mut runner = application.build_headless(world)?;
    let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let released = [InputEvent::key(
        PhysicalKeyCode::KeyD,
        ButtonState::Released,
    )];

    assert!(matches!(
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?)),
        FrameOutcome::Advanced(_)
    ));
    assert!(matches!(
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &released, viewport()?)),
        FrameOutcome::Advanced(_)
    ));

    let rejected = runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?));
    assert!(matches!(
        rejected,
        FrameOutcome::Rejected(BeginFrameRejection::Input(
            InputCollectionError::RetainedFixedEdgeLimitExceeded {
                limit: 2,
                retained: 2,
                incoming: 1,
            }
        ))
    ));

    let delivered = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = delivered else {
        panic!("bounded frame should be accepted");
    };
    assert!(report.failure().is_none());
    let Some(observation) = runner.resource::<RetainedEdgeObservation>() else {
        panic!("retained-edge observation should exist");
    };
    assert_eq!(observation.pressed, 1);
    assert_eq!(observation.released, 1);
    assert!(!observation.held);

    assert!(matches!(
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?)),
        FrameOutcome::Advanced(_)
    ));
    Ok(())
}

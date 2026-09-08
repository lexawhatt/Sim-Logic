//! Runtime regressions for transitions.

use super::*;

#[test]
fn command_poison_rejects_transition_consumes_intent_and_reuses_queue() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    config.set_command_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("command-poison-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("command-poison-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FixedUpdate,
        |input: FixedInput<TestAction>,
         next: Option<Res<NextWorld>>,
         mut commands: LogicCommands| {
            let mut requested = false;
            if let Some(next) = next {
                for edge in input.pressed(TestAction::Enter) {
                    requested = true;
                    assert!(commands.replace_world(edge.intent(), next.0).is_ok());
                    assert!(matches!(
                        commands.spawn(Ephemeral),
                        Err(CommandEnqueueError::LimitExceeded { limit: 1 })
                    ));
                }
            }
            if !requested {
                assert!(commands.spawn(Ephemeral).is_ok());
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
        panic!("bounded command-poison frame should be accepted");
    };
    assert!(matches!(
        failed_report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FixedUpdate,
            error: crate::commands::CommandBatchError::LimitExceeded { limit: 1 },
        })
    ));
    assert!(matches!(failed_report.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);

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
    assert_eq!(recovered_report.spawned(), 1);
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.components::<Ephemeral>().count(), 1);
    Ok(())
}

#[test]
fn one_input_intent_commits_world_a_to_b_during_catch_up() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let mut camera_b = camera()?;
    camera_b.set_center(Vec2::new(7.0, 3.0))?;
    let circle_b = circle(Color::rgb8(32, 96, 240))?;
    let transform_b = Transform2d::new(Vec2::new(7.0, 3.0))?;
    let world_b = application.register_world("world-b", move |world| {
        world.spawn(camera_b)?;
        world.spawn((Ball, transform_b, circle_b))?;
        world.insert_resource(RetainedEdgeObservation::default())?;
        Ok(())
    })?;

    let camera_a = camera()?;
    let circle_a = circle(Color::rgb8(240, 96, 32))?;
    let world_a = application.register_world("world-a", move |world| {
        world.spawn(camera_a)?;
        world.spawn((Ball, Transform2d::default(), circle_a))?;
        world.insert_resource(NextWorld(world_b))?;
        Ok(())
    })?;

    // Two systems react to the same causal edge. Arbitration must coalesce
    // their identical requests instead of treating the catch-up frame as a
    // conflict or allowing a second fixed tick to repeat the request.
    application.add_system(Stage::FixedUpdate, request_next_world);
    application.add_system(Stage::FixedUpdate, request_next_world);
    application.add_system(
        Stage::FixedUpdate,
        |input: FixedInput<TestAction>, observation: Option<ResMut<RetainedEdgeObservation>>| {
            let Some(mut observation) = observation else {
                return;
            };
            observation.pressed = input.pressed(TestAction::Enter).count();
            observation.released = input.released(TestAction::Enter).count();
            observation.held = input.held(TestAction::Enter);
        },
    );

    let mut runner = application.build_headless(world_a)?;
    assert_eq!(runner.factory_name(world_a), Some("world-a"));
    assert_eq!(runner.factory_name(world_b), Some("world-b"));
    assert_eq!(runner.active_world_name(), "world-a");
    let old_generation = runner.world_generation();
    let events = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(50),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    let FrameTransition::Committed {
        old,
        new,
        target,
        warning,
    } = report.transition()
    else {
        panic!("input edge should commit World B");
    };
    assert_eq!(*old, old_generation);
    assert_eq!(*new, runner.world_generation());
    assert_eq!(*target, world_b);
    assert_eq!(runner.active_world_name(), "world-b");
    assert!(warning.is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert!(report.failure().is_none());
    assert_eq!(report.extracted_generation(), Some(*new));

    let lifecycle = runner.lifecycle();
    assert_eq!(lifecycle.len(), 3);
    assert_eq!(lifecycle[0].event(), LifecycleEvent::WorldEnter);
    assert_eq!(lifecycle[0].generation(), *old);
    assert_eq!(lifecycle[1].event(), LifecycleEvent::WorldExit);
    assert_eq!(lifecycle[1].generation(), *old);
    assert_eq!(lifecycle[2].event(), LifecycleEvent::WorldEnter);
    assert_eq!(lifecycle[2].generation(), *new);

    let extracted = runner
        .extracted_frame()
        .ok_or("committed World should be extracted in the same frame")?;
    assert_eq!(extracted.world_generation(), *new);
    assert_eq!(extracted.camera().center(), Vec2::new(7.0, 3.0));
    let [resolved] = extracted.resolved_circles() else {
        panic!("World B should extract exactly one circle");
    };
    assert_eq!(resolved.position(), Vec2::new(7.0, 3.0));

    let frame_snapshot = runner
        .active
        .world
        .get_resource::<FrameInputState<TestAction>>()
        .ok_or("committed World should own a frame snapshot")?;
    let fixed_snapshot = runner
        .active
        .world
        .get_resource::<FixedInputState<TestAction>>()
        .ok_or("committed World should own a fixed snapshot")?;
    assert!(!frame_snapshot.held(TestAction::Enter));
    assert_eq!(frame_snapshot.pressed(TestAction::Enter).count(), 0);
    assert!(!fixed_snapshot.held(TestAction::Enter));
    assert_eq!(fixed_snapshot.pressed(TestAction::Enter).count(), 0);

    let next = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        next,
        FrameOutcome::Advanced(ref report)
            if report.failure().is_none() && report.fixed_ticks_attempted() == 1
    ));
    let observation = runner
        .resource::<RetainedEdgeObservation>()
        .ok_or("target input observation should remain available")?;
    assert!(observation.held);
    assert_eq!(observation.pressed, 0);
    assert_eq!(observation.released, 0);
    Ok(())
}

#[test]
fn failed_candidate_preserves_old_generation_and_snaps_interpolation() -> Result<(), Box<dyn Error>>
{
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let failing_world = application.register_world("failing-world", |_world| {
        Err(WorldBuildError::user("deliberate candidate failure"))
    })?;
    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world_a = application.register_world("world-a", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Transform2d::default(), ball))?;
        world.insert_resource(NextWorld(failing_world))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);
    application.add_fallible_system(Stage::FixedUpdate, follow_ball);
    application.add_system(Stage::FixedUpdate, request_next_world);

    let mut runner = application.build_headless(world_a)?;
    let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let first = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &movement,
        viewport()?,
    ));
    let FrameOutcome::Advanced(first_report) = first else {
        panic!("bounded movement frame should be accepted");
    };
    assert!(first_report.failure().is_none());
    let first_extracted = runner
        .extracted_frame()
        .ok_or("movement frame should publish an extraction")?;
    let [first_circle] = first_extracted.resolved_circles() else {
        panic!("test World should extract exactly one circle");
    };
    assert!(first_circle.position().x() > 0.0);
    assert!(first_circle.position().x() < 1.0);

    let old_generation = runner.world_generation();
    let transition = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let second = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(9),
        &transition,
        viewport()?,
    ));
    let FrameOutcome::Advanced(second_report) = second else {
        panic!("bounded transition frame should be accepted");
    };

    assert!(matches!(
        second_report.transition(),
        FrameTransition::PreparationFailed { target, .. } if *target == failing_world
    ));
    assert!(second_report.failure().is_none());
    assert_eq!(second_report.fixed_ticks_attempted(), 1);
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(runner.lifecycle().len(), 1);
    assert!(runner.active.world.contains_resource::<CommandQueue>());

    let Some((_, transform)) = runner.components::<Transform2d>().next() else {
        panic!("old World should retain its ball");
    };
    assert_eq!(transform.previous_translation(), transform.translation());
    assert!(transform.translation().x() > 1.9);
    let Some((_, camera)) = runner.components::<ActiveCamera2d>().next() else {
        panic!("old World should retain its camera");
    };
    assert_eq!(camera.previous_center(), camera.center());
    assert!(camera.center().x() > 1.9);

    let extracted = runner
        .extracted_frame()
        .ok_or("old World should be extracted after failed preparation")?;
    assert_eq!(extracted.world_generation(), old_generation);
    let [resolved] = extracted.resolved_circles() else {
        panic!("old World should still extract exactly one circle");
    };
    assert_eq!(resolved.position(), transform.translation());
    assert_eq!(extracted.camera().center(), camera.center());
    Ok(())
}

#[test]
fn old_frame_token_is_invalid_while_waiting_for_fixed_delivery() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(StoredFrameIntent(None))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        move |input: FrameInput<TestAction>,
              mut stored: ResMut<StoredFrameIntent>,
              mut commands: LogicCommands| {
            if let Some(edge) = input.pressed(TestAction::Enter).next() {
                stored.0 = Some(edge.intent());
                return;
            }
            if let Some(intent) = stored.0.take() {
                assert!(commands.replace_world(intent, target).is_ok());
            }
        },
    );

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let events = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport()?));
    let FrameOutcome::Advanced(first_report) = first else {
        panic!("bounded frame should be accepted");
    };
    assert_eq!(first_report.fixed_ticks_attempted(), 0);
    assert!(matches!(first_report.transition(), FrameTransition::None));

    let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(second_report) = second else {
        panic!("bounded frame should be accepted");
    };
    assert_eq!(second_report.fixed_ticks_attempted(), 0);
    assert!(matches!(
        second_report.transition(),
        FrameTransition::Invalid(TransitionRequestFailure::InvalidIntent)
    ));
    assert!(second_report.failure().is_none());
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(runner.lifecycle().len(), 1);
    Ok(())
}

#[test]
fn frame_update_does_not_run_after_fixed_transition_request() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let target_camera = camera()?;
    let target = application.register_world("target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(NextWorld(target))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, request_next_world);
    let frame_runs = Arc::new(AtomicUsize::new(0));
    let observed_frame_runs = Arc::clone(&frame_runs);
    application.add_system(Stage::FrameUpdate, move || {
        observed_frame_runs.fetch_add(1, Ordering::SeqCst);
    });

    let mut runner = application.build_headless(source)?;
    let events = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(50),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(frame_runs.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn same_intent_with_different_targets_is_malformed_at_runtime() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let first_camera = camera()?;
    let first_target = application.register_world("first-target", move |world| {
        world.spawn(first_camera)?;
        Ok(())
    })?;
    let second_camera = camera()?;
    let second_target = application.register_world("second-target", move |world| {
        world.spawn(second_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        Ok(())
    })?;
    application.add_system(
        Stage::FixedUpdate,
        move |input: FixedInput<TestAction>, mut commands: LogicCommands| {
            for edge in input.pressed(TestAction::Enter) {
                assert!(commands.replace_world(edge.intent(), first_target).is_ok());
                assert!(commands.replace_world(edge.intent(), second_target).is_ok());
            }
        },
    );

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let events = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(50),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert!(matches!(
        report.transition(),
        FrameTransition::Rejected(TransitionRejection::MalformedIntent)
    ));
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert!(report.dropped_for_transition().is_some());
    assert_eq!(report.extracted_generation(), Some(old_generation));
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(runner.lifecycle().len(), 1);
    Ok(())
}

#[test]
fn different_intents_with_same_target_commit_with_runtime_warning() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_lifecycle_trace_limit(2)?;
    let mut application = Application::<TestAction>::new(config)?;

    let target_camera = camera()?;
    let target = application.register_world("target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move |mut commands: LogicCommands| {
        let first = commands.new_transition_intent();
        let second = commands.new_transition_intent();
        let (Ok(first), Ok(second)) = (first, second) else {
            panic!("fixed stage should issue two bounded direct intents");
        };
        assert!(commands.replace_world(first, target).is_ok());
        assert!(commands.replace_world(second, target).is_ok());
    });

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    let FrameTransition::Committed {
        old,
        new,
        target: committed_target,
        warning: Some(warning),
    } = report.transition()
    else {
        panic!("convergent direct intents should commit with a warning");
    };
    assert_eq!(*old, old_generation);
    assert_eq!(*new, runner.world_generation());
    assert_eq!(*committed_target, target);
    assert_eq!(warning.distinct_intents(), 2);
    assert!(report.failure().is_none());
    assert_eq!(runner.lifecycle().len(), 2);
    assert_eq!(runner.lifecycle()[0].event(), LifecycleEvent::WorldExit);
    assert_eq!(runner.lifecycle()[1].event(), LifecycleEvent::WorldEnter);
    assert_eq!(runner.dropped_lifecycle_records(), 1);
    Ok(())
}

#[test]
fn different_intents_with_different_targets_conflict_at_runtime() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;

    let first_camera = camera()?;
    let first_target = application.register_world("first-target", move |world| {
        world.spawn(first_camera)?;
        Ok(())
    })?;
    let second_camera = camera()?;
    let second_target = application.register_world("second-target", move |world| {
        world.spawn(second_camera)?;
        Ok(())
    })?;
    let source_camera = camera()?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move |mut commands: LogicCommands| {
        let first = commands.new_transition_intent();
        let second = commands.new_transition_intent();
        let (Ok(first), Ok(second)) = (first, second) else {
            panic!("fixed stage should issue two bounded direct intents");
        };
        assert!(commands.replace_world(first, first_target).is_ok());
        assert!(commands.replace_world(second, second_target).is_ok());
    });

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(50),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert!(matches!(
        report.transition(),
        FrameTransition::Rejected(TransitionRejection::Conflict)
    ));
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert!(report.dropped_for_transition().is_some());
    assert_eq!(report.extracted_generation(), Some(old_generation));
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(runner.lifecycle().len(), 1);
    Ok(())
}

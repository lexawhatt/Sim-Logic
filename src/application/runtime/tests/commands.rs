//! Runtime regressions for commands.

use super::*;

#[test]
fn command_spawn_snaps_supplied_interpolation_history() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut copied_history = Transform2d::default();
    copied_history.set_translation(Vec2::new(10.0, 0.0))?;
    application.add_system(Stage::FrameUpdate, move |mut commands: LogicCommands| {
        assert!(commands.spawn((Ball, copied_history, circle)).is_ok());
    });

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(8),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert!(report.failure().is_none());
    let extracted = runner
        .extracted_frame()
        .ok_or("successful frame should publish extraction")?;
    let [resolved] = extracted.resolved_circles() else {
        panic!("command should spawn exactly one circle");
    };
    assert_eq!(resolved.position(), Vec2::new(10.0, 0.0));
    Ok(())
}

#[test]
fn command_insert_crosses_the_stage_barrier_and_preserves_existing_motion()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(100), 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("insert-world", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, Transform2d::default()))?;
        world.insert_resource(InsertTarget(target))?;
        world.insert_resource(InsertObservation::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_insert_target);
    application.add_system(
        Stage::FrameUpdate,
        move |target: Res<InsertTarget>,
              mut observation: ResMut<InsertObservation>,
              mut commands: LogicCommands| {
            if !observation.queued {
                assert!(commands.insert(target.0, circle).is_ok());
                observation.queued = true;
            }
        },
    );
    application.add_system(Stage::FrameUpdate, observe_insert_visibility);

    let mut runner = application.build_headless(world)?;
    let inserted = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(150),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(inserted_report) = inserted else {
        panic!("bounded insert frame should be accepted");
    };
    assert!(inserted_report.failure().is_none());
    assert_eq!(inserted_report.fixed_ticks_attempted(), 1);
    assert_eq!(inserted_report.spawned(), 0);
    assert_eq!(inserted_report.despawned(), 0);
    assert_eq!(
        runner
            .resource::<InsertObservation>()
            .ok_or("insert observation should exist")?
            .same_stage_counts,
        [0]
    );

    let (target, _) = runner
        .components::<Ball>()
        .next()
        .ok_or("insert target should remain live")?;
    assert!(runner.component::<CircleVisual>(target).is_ok());
    let transform = runner.component::<Transform2d>(target)?;
    assert_eq!(transform.previous_translation(), Vec2::ZERO);
    assert_eq!(transform.translation(), Vec2::new(2.0, 0.0));
    let [resolved] = runner
        .extracted_frame()
        .ok_or("insert frame should publish extraction")?
        .resolved_circles()
    else {
        panic!("inserted visual should extract in the barrier frame");
    };
    assert_eq!(resolved.source(), target);
    assert_eq!(resolved.position(), Vec2::new(1.0, 0.0));

    let observed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        observed,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_eq!(
        runner
            .resource::<InsertObservation>()
            .ok_or("insert observation should remain")?
            .same_stage_counts,
        [0, 1]
    );
    Ok(())
}

#[test]
fn command_insert_can_disable_after_the_barrier_without_losing_identity()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(Ball, Disabled)>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("disable-world", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle))?;
        world.insert_resource(InsertTarget(target))?;
        world.insert_resource(InsertObservation::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |target: Res<InsertTarget>,
         mut observation: ResMut<InsertObservation>,
         mut commands: LogicCommands| {
            if !observation.queued {
                assert!(commands.insert(target.0, Disabled).is_ok());
                observation.queued = true;
            }
        },
    );
    application.add_system(Stage::FrameUpdate, observe_insert_visibility);

    let mut runner = application.build_headless(world)?;
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("initial extraction should exist")?
            .resolved_circles()
            .len(),
        1
    );
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("insert target should exist")?
        .0;
    let disabled = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        disabled,
        FrameOutcome::Advanced(ref report)
            if report.failure().is_none()
                && report.spawned() == 0
                && report.despawned() == 0
    ));
    assert!(runner.component::<Disabled>(target).is_ok());
    assert!(runner.component::<Ball>(target).is_ok());
    assert!(
        runner
            .extracted_frame()
            .ok_or("disabled frame should publish extraction")?
            .resolved_circles()
            .is_empty()
    );
    assert_eq!(
        runner
            .resource::<InsertObservation>()
            .ok_or("insert observation should exist")?
            .same_stage_counts,
        [1]
    );

    let next = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        next,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_eq!(
        runner
            .resource::<InsertObservation>()
            .ok_or("insert observation should remain")?
            .same_stage_counts,
        [1, 0]
    );
    Ok(())
}

#[test]
fn command_remove_crosses_each_fixed_barrier_before_extraction() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 4)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("remove-world", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle))?;
        world.insert_resource(InsertTarget(target))?;
        world.insert_resource(RemoveObservation::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_insert_target);
    application.add_system(
        Stage::FixedUpdate,
        |target: Res<InsertTarget>,
         mut observation: ResMut<RemoveObservation>,
         mut commands: LogicCommands| {
            if !observation.queued {
                assert!(commands.remove::<CircleVisual>(target.0).is_ok());
                observation.queued = true;
            }
        },
    );
    application.add_system(
        Stage::FixedUpdate,
        |visuals: Query<&CircleVisual, With<Ball>>, mut observation: ResMut<RemoveObservation>| {
            observation.same_stage_counts.push(visuals.iter().count());
        },
    );

    let mut runner = application.build_headless(world)?;
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("remove target should exist")?
        .0;
    let removed = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = removed else {
        panic!("bounded remove frame should be accepted");
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 2);
    assert_eq!(report.spawned(), 0);
    assert_eq!(report.despawned(), 0);
    assert_eq!(
        runner
            .resource::<RemoveObservation>()
            .ok_or("remove observation should exist")?
            .same_stage_counts,
        [1, 0]
    );
    assert!(matches!(
        runner.component::<CircleVisual>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(runner.component::<Transform2d>(target).is_ok());
    let transform = runner.component::<Transform2d>(target)?;
    assert_eq!(transform.previous_translation(), Vec2::new(2.0, 0.0));
    assert_eq!(transform.translation(), Vec2::new(4.0, 0.0));
    assert!(runner.component::<Ball>(target).is_ok());
    assert!(
        runner
            .extracted_frame()
            .ok_or("remove frame should publish extraction")?
            .resolved_circles()
            .is_empty()
    );
    Ok(())
}

#[test]
fn startup_remove_shapes_the_candidate_before_its_first_extraction() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("startup-remove", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle))?;
        world.insert_resource(InsertTarget(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::Startup,
        |target: Res<InsertTarget>, mut commands: LogicCommands| {
            assert!(commands.remove::<CircleVisual>(target.0).is_ok());
        },
    );

    let runner = application.build_headless(world)?;
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("startup remove target should exist")?
        .0;
    assert!(matches!(
        runner.component::<CircleVisual>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(runner.component::<Transform2d>(target).is_ok());
    assert!(
        runner
            .extracted_frame()
            .ok_or("startup removal should still permit initial extraction")?
            .resolved_circles()
            .is_empty()
    );
    Ok(())
}

#[test]
fn command_remove_disabled_enables_after_the_barrier_without_losing_identity()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(Ball, Disabled)>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("enable-world", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle, Disabled))?;
        world.insert_resource(InsertTarget(target))?;
        world.insert_resource(RemoveObservation::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |target: Res<InsertTarget>,
         mut observation: ResMut<RemoveObservation>,
         mut commands: LogicCommands| {
            if !observation.queued {
                assert!(commands.remove::<Disabled>(target.0).is_ok());
                observation.queued = true;
            }
        },
    );
    application.add_system(
        Stage::FrameUpdate,
        |balls: Query<&Ball>, mut observation: ResMut<RemoveObservation>| {
            observation.same_stage_counts.push(balls.iter().count());
        },
    );

    let mut runner = application.build_headless(world)?;
    assert!(
        runner
            .extracted_frame()
            .ok_or("initial extraction should exist")?
            .resolved_circles()
            .is_empty()
    );
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("enable target should exist")?
        .0;
    let enabled = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        enabled,
        FrameOutcome::Advanced(ref report)
            if report.failure().is_none()
                && report.spawned() == 0
                && report.despawned() == 0
    ));
    assert!(matches!(
        runner.component::<Disabled>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(runner.component::<Ball>(target).is_ok());
    let [resolved] = runner
        .extracted_frame()
        .ok_or("enabled frame should publish extraction")?
        .resolved_circles()
    else {
        panic!("enabled entity should extract after the barrier");
    };
    assert_eq!(resolved.source(), target);
    assert_eq!(
        runner
            .resource::<RemoveObservation>()
            .ok_or("remove observation should exist")?
            .same_stage_counts,
        [0]
    );

    let next = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        next,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_eq!(
        runner
            .resource::<RemoveObservation>()
            .ok_or("remove observation should remain")?
            .same_stage_counts,
        [0, 1]
    );
    Ok(())
}

#[test]
fn invalid_required_component_removal_rejects_exit_and_preserves_the_entity()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("invalid-remove", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle))?;
        world.insert_resource(InsertTarget(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |target: Res<InsertTarget>, mut commands: LogicCommands| {
            assert!(commands.remove::<Transform2d>(target.0).is_ok());
            assert!(commands.request_exit().is_ok());
        },
    );

    let mut runner = application.build_headless(world)?;
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("invalid-remove target should exist")?
        .0;
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded invalid-remove frame should be accepted");
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: crate::commands::CommandBatchError::RequiredComponentWouldBeMissing {
                entity,
                ..
            },
        }) if *entity == target
    ));
    assert!(!report.exit_requested());
    assert!(runner.component::<Ball>(target).is_ok());
    assert!(runner.component::<CircleVisual>(target).is_ok());
    assert!(runner.component::<Transform2d>(target).is_ok());
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("rejected removal should preserve the prior publication")?
            .resolved_circles()
            .len(),
        1
    );
    Ok(())
}

#[test]
fn successful_remove_commits_before_application_exit() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("remove-exit", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Ball, circle))?;
        world.insert_resource(InsertTarget(target))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |target: Res<InsertTarget>, mut commands: LogicCommands| {
            assert!(commands.remove::<CircleVisual>(target.0).is_ok());
            assert!(commands.request_exit().is_ok());
        },
    );

    let mut runner = application.build_headless(world)?;
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("remove-exit target should exist")?
        .0;
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded remove-exit frame should be accepted");
    };
    assert!(report.failure().is_none());
    assert!(report.exit_requested());
    assert!(report.extracted_generation().is_none());
    assert!(matches!(
        runner.component::<CircleVisual>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(runner.component::<Transform2d>(target).is_ok());
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("exit should retain only the previous publication")?
            .resolved_circles()
            .len(),
        1
    );
    Ok(())
}

#[test]
fn command_spawn_snaps_supplied_camera_history() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let initial_camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(initial_camera)?;
        Ok(())
    })?;
    let mut copied_history = camera()?;
    copied_history.set_center(Vec2::new(10.0, 4.0))?;
    application.add_system(
        Stage::FrameUpdate,
        move |cameras: Query<LogicEntityRef, With<ActiveCamera2d>>, mut commands: LogicCommands| {
            let current = cameras
                .single()
                .expect("the valid World should have one active camera")
                .handle();
            assert!(commands.despawn(current).is_ok());
            assert!(commands.spawn(copied_history).is_ok());
        },
    );

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(8),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert!(report.failure().is_none());
    let (_, camera) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("replacement camera should be inspectable")?;
    assert_eq!(camera.previous_center(), Vec2::new(10.0, 4.0));
    assert_eq!(camera.center(), Vec2::new(10.0, 4.0));
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("successful frame should publish extraction")?
            .camera()
            .center(),
        Vec2::new(10.0, 4.0)
    );
    Ok(())
}

#[test]
fn command_spawned_entity_can_be_resolved_and_despawned_later() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ephemeral>()?;
    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.insert_resource(SpawnThenDespawn::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FrameUpdate, spawn_then_despawn);

    let mut runner = application.build_headless(world)?;
    let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(first_report) = first else {
        panic!("bounded frame should be accepted");
    };
    assert_eq!(first_report.spawned(), 1);
    assert_eq!(runner.components::<Ephemeral>().count(), 1);
    let spawned = runner
        .components::<Ephemeral>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("first frame should expose the spawned entity")?;
    assert!(runner.component::<Ephemeral>(spawned).is_ok());

    let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(second_report) = second else {
        panic!("bounded frame should be accepted");
    };
    assert_eq!(second_report.despawned(), 1);
    assert_eq!(runner.components::<Ephemeral>().count(), 0);
    assert!(matches!(
        runner.component::<Ephemeral>(spawned),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    Ok(())
}

#[test]
fn managed_entity_limit_stays_exact_across_factory_and_stage_barriers() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_entity_limit(3)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ephemeral>()?;

    let camera = camera()?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, |mut commands: LogicCommands| {
        assert!(commands.spawn(Ephemeral).is_ok());
    });
    application.add_system(
        Stage::FixedUpdate,
        |entities: Query<LogicEntityRef, With<Ephemeral>>, mut commands: LogicCommands| {
            for entity in &entities {
                assert!(commands.despawn(entity.handle()).is_ok());
            }
            assert!(commands.spawn(Ephemeral).is_ok());
            assert!(commands.spawn(Ephemeral).is_ok());
        },
    );
    application.add_system(Stage::FrameUpdate, |mut commands: LogicCommands| {
        assert!(commands.spawn(Ephemeral).is_ok());
    });

    let mut runner = application.build_headless(world)?;
    assert_eq!(runner.components::<Ephemeral>().count(), 1);

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert_eq!(report.spawned(), 2);
    assert_eq!(report.despawned(), 1);
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: crate::commands::CommandBatchError::EntityLimitExceeded {
                limit: 3,
                requested: 4,
            },
        })
    ));
    assert_eq!(runner.components::<Ephemeral>().count(), 2);
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);
    Ok(())
}

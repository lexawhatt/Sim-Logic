//! Runtime regressions for identity.

use super::*;

#[test]
fn advanced_frames_advance_standalone_ecs_trackers() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("tracker-world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let before = runner.active.world.last_change_tick();

    let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        first,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    let after_first = runner.active.world.last_change_tick();
    assert_ne!(after_first, before);

    let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        second,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_ne!(runner.active.world.last_change_tick(), after_first);
    Ok(())
}

#[test]
fn managed_queries_never_visit_internal_resource_entities() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("managed-query-world", move |world| {
        world.insert_resource(ResourceOnlyComponent)?;
        world.insert_resource(ManagedQueryCounts::default())?;
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, count_managed_query_entities);

    let runner = application.build_headless(world)?;
    let counts = runner
        .resource::<ManagedQueryCounts>()
        .ok_or("Startup count resource should remain available")?;

    assert_eq!(counts.all, 1);
    assert_eq!(counts.resource_components, 0);
    Ok(())
}

#[test]
fn headless_component_lookup_rejects_another_application() -> Result<(), Box<dyn Error>> {
    let mut first_application = Application::<TestAction>::new(AppConfig::default())?;
    let first_camera = camera()?;
    let first_world = first_application.register_world("first-app", move |world| {
        world.spawn(first_camera)?;
        Ok(())
    })?;
    let first_runner = first_application.build_headless(first_world)?;

    let mut second_application = Application::<TestAction>::new(AppConfig::default())?;
    let second_camera = camera()?;
    let second_world = second_application.register_world("second-app", move |world| {
        world.spawn(second_camera)?;
        Ok(())
    })?;
    let second_runner = second_application.build_headless(second_world)?;
    let foreign = second_runner
        .components::<ActiveCamera2d>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("second application should contain its camera")?;

    assert!(matches!(
        first_runner.component::<ActiveCamera2d>(foreign),
        Err(QueryEntityError::ForeignWorld { .. })
    ));
    Ok(())
}

#[test]
fn headless_component_lookup_rejects_mismatched_private_provenance() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = camera()?;
    let world = application.register_world("private-provenance", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let entity = runner
        .components::<ActiveCamera2d>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("World should contain its camera")?;
    let corrupted = WorldGeneration::new(runner.application, 99);
    runner
        .active
        .world
        .entity_mut(entity.entity())
        .insert(ManagedEntity::for_generation(corrupted));

    assert!(matches!(
        runner.component::<ActiveCamera2d>(entity),
        Err(QueryEntityError::DoesNotMatch { entity: rejected }) if rejected == entity
    ));
    Ok(())
}

#[test]
fn later_system_observes_component_write_in_the_same_stage() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<SameStageValue>()?;
    let camera = camera()?;
    let world = application.register_world("same-stage-world", move |world| {
        world.insert_resource(SameStageObservation::default())?;
        world.spawn(camera)?;
        world.spawn(SameStageValue(0))?;
        Ok(())
    })?;
    application.add_system(Stage::Startup, write_same_stage_component);
    application.add_system(Stage::Startup, observe_same_stage_component);

    let runner = application.build_headless(world)?;

    assert_eq!(
        runner
            .resource::<SameStageObservation>()
            .and_then(|observation| observation.0),
        Some(7)
    );
    Ok(())
}

#[test]
fn retired_logic_entity_cannot_alias_a_reused_bevy_slot() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

    let camera_b = camera()?;
    let circle_b = circle(Color::WHITE)?;
    let world_b = application.register_world("world-b", move |world| {
        world.insert_resource(TargetWorld)?;
        world.spawn(camera_b)?;
        world.spawn((Ball, Transform2d::default(), circle_b))?;
        Ok(())
    })?;
    let camera_a = camera()?;
    let circle_a = circle(Color::WHITE)?;
    let world_a = application.register_world("world-a", move |world| {
        world.insert_resource(NextWorld(world_b))?;
        world.spawn(camera_a)?;
        world.spawn((Ball, Transform2d::default(), circle_a))?;
        Ok(())
    })?;

    let retained = Arc::new(Mutex::new(None));
    let retained_for_system = Arc::clone(&retained);
    let rejected = Arc::new(AtomicUsize::new(0));
    let rejected_by_system = Arc::clone(&rejected);
    application.add_system(Stage::FixedUpdate, request_next_world);
    application.add_system(
        Stage::FixedUpdate,
        move |target: Option<Res<TargetWorld>>, mut balls: Query<&mut Transform2d>| {
            if target.is_none() {
                return;
            }
            let entity = retained_for_system.lock().ok().and_then(|stored| *stored);
            let Some(entity) = entity else {
                return;
            };
            if matches!(
                balls.get_mut(entity),
                Err(QueryEntityError::ForeignWorld { .. })
            ) {
                rejected_by_system.store(1, Ordering::SeqCst);
            }
        },
    );

    let mut runner = application.build_headless(world_a)?;
    let old_entity = runner
        .components::<Ball>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("World A should contain a ball")?;
    let mut stored = retained.lock().map_err(|_| "test lock was poisoned")?;
    *stored = Some(old_entity);
    drop(stored);

    let enter = [InputEvent::key(
        PhysicalKeyCode::Enter,
        ButtonState::Pressed,
    )];
    let first = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &enter,
        viewport()?,
    ));
    assert!(matches!(
        first,
        FrameOutcome::Advanced(ref report)
            if matches!(report.transition(), FrameTransition::Committed { .. })
    ));
    assert!(matches!(
        runner.component::<Transform2d>(old_entity),
        Err(QueryEntityError::ForeignWorld { .. })
    ));
    let second = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        second,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_eq!(rejected.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn extraction_excludes_disabled_even_if_bevy_defaults_are_replaced() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.approve_component::<Disabled>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("world", move |world| {
        world.insert_resource(DefaultQueryFilters::empty())?;
        world.insert_resource(EnabledBallCount::default())?;
        world.spawn(camera)?;
        world.spawn((Disabled, camera))?;
        world.spawn((Ball, Disabled, Transform2d::default(), circle))?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |balls: Query<&Ball>, mut count: ResMut<EnabledBallCount>| {
            count.0 = balls.iter().count();
        },
    );

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        outcome,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    let extracted = runner
        .extracted_frame()
        .ok_or("initial World should publish extraction")?;
    assert!(extracted.resolved_circles().is_empty());
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 2);
    assert_eq!(
        runner
            .resource::<EnabledBallCount>()
            .ok_or("observation should remain present")?
            .0,
        0
    );
    let disabled_ball = runner
        .components::<Ball>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("headless inspection should include the disabled ball")?;
    assert!(runner.component::<Ball>(disabled_ball).is_ok());
    assert!(runner.component::<Transform2d>(disabled_ball).is_ok());
    assert!(matches!(
        runner.component::<Ephemeral>(disabled_ball),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    Ok(())
}

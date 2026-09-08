//! Runtime regressions for interpolation.

use super::*;

#[test]
fn fixed_input_moves_and_interpolates_a_circle() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world_a = application.register_world("world-a", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Transform2d::default(), ball))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);

    let mut runner = application.build_headless(world_a)?;
    let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded frame should be accepted");
    };

    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert!(report.failure().is_none());
    assert!(matches!(report.transition(), FrameTransition::None));
    let extracted = runner
        .extracted_frame()
        .ok_or("successful frame should publish extraction")?;
    let [resolved] = extracted.resolved_circles() else {
        panic!("test World should extract exactly one circle");
    };
    assert!((resolved.position().x() - 0.5).abs() < 0.001);
    Ok(())
}

#[test]
fn only_an_exactly_empty_fixed_stage_skips_interpolation_capture() -> Result<(), Box<dyn Error>> {
    for has_fixed_system in [false, true] {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        if has_fixed_system {
            application.add_fixed_system(read_transform_for_empty_stage_boundary);
        }
        let camera = camera()?;
        let transform = Transform2d::from_xy(3.0, -2.0)?;
        let world = application.register_world("capture-boundary", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, transform))?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;

        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded capture-boundary frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert_eq!(
            runner.active.previous_translations.len(),
            usize::from(has_fixed_system)
        );
        assert_eq!(
            runner.active.previous_camera_centers.len(),
            usize::from(has_fixed_system)
        );
        let (_, retained) = runner
            .components::<Transform2d>()
            .next()
            .expect("test Transform should remain managed");
        assert_eq!(retained.translation(), transform.translation());
        assert_eq!(retained.previous_translation(), transform.translation());
    }
    Ok(())
}

#[test]
fn fixed_camera_follow_matches_visual_interpolation_across_catch_up_and_zero_tick_frames()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world = application.register_world("camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Transform2d::default(), ball))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);
    application.add_fallible_system(Stage::FixedUpdate, follow_ball);

    let mut runner = application.build_headless(world)?;
    let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let catch_up = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(25),
        &movement,
        viewport()?,
    ));
    let FrameOutcome::Advanced(catch_up_report) = catch_up else {
        panic!("bounded catch-up frame should be accepted");
    };
    assert_eq!(catch_up_report.fixed_ticks_attempted(), 2);
    assert!(catch_up_report.failure().is_none());

    let (_, camera) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("active camera should remain inspectable")?;
    assert!((camera.previous_center().x() - 0.6).abs() < 0.001);
    assert!((camera.center().x() - 1.2).abs() < 0.001);
    let catch_up_frame = runner
        .extracted_frame()
        .ok_or("catch-up frame should publish extraction")?;
    assert!((catch_up_frame.camera().center().x() - 0.9).abs() < 0.001);
    assert!((catch_up_frame.resolved_circles()[0].position().x() - 0.9).abs() < 0.001);

    let zero_tick = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(2),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(zero_tick_report) = zero_tick else {
        panic!("bounded zero-tick frame should be accepted");
    };
    assert_eq!(zero_tick_report.fixed_ticks_attempted(), 0);
    assert!(zero_tick_report.failure().is_none());

    let (_, unchanged) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("active camera should remain inspectable")?;
    assert!((unchanged.previous_center().x() - 0.6).abs() < 0.001);
    assert!((unchanged.center().x() - 1.2).abs() < 0.001);
    let zero_tick_frame = runner
        .extracted_frame()
        .ok_or("zero-tick frame should publish extraction")?;
    assert!((zero_tick_frame.camera().center().x() - 1.02).abs() < 0.001);
    assert!((zero_tick_frame.resolved_circles()[0].position().x() - 1.02).abs() < 0.001);
    Ok(())
}

#[test]
fn diagonal_wasd_movement_is_normalized() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    application.bind_key(PhysicalKeyCode::KeyW, TestAction::MoveUp)?;

    let camera = camera()?;
    let ball = circle(Color::WHITE)?;
    let world = application.register_world("diagonal-world", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Transform2d::default(), ball))?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, move_ball);

    let mut runner = application.build_headless(world)?;
    let events = [
        InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
    ];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(17),
        &events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        panic!("bounded diagonal frame should be accepted");
    };
    assert!(report.failure().is_none());

    let ball_entity = runner
        .components::<Ball>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("moving ball should remain available")?;
    let (_, transform) = runner
        .components::<Transform2d>()
        .find(|(entity, _)| *entity == ball_entity)
        .ok_or("moving ball transform should remain available")?;
    let translation = transform.translation();
    let distance = (translation.x() * translation.x() + translation.y() * translation.y()).sqrt();
    assert!((distance - 1.0).abs() < 0.000_1);
    assert!((translation.x() - translation.y()).abs() < 0.000_1);
    Ok(())
}

#[test]
fn cached_interpolation_query_discovers_command_spawned_archetype() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Ball>()?;
    application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        move |balls: Query<&Ball>, mut commands: LogicCommands| {
            if balls.iter().next().is_none() {
                assert!(
                    commands
                        .spawn((Ball, Transform2d::default(), circle))
                        .is_ok()
                );
            }
        },
    );
    application.add_system(Stage::FixedUpdate, move_ball);

    let mut runner = application.build_headless(world)?;
    let spawned = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    assert!(matches!(
        spawned,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));

    let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    for events in [&movement[..], &[][..]] {
        let moved = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            events,
            viewport()?,
        ));
        assert!(matches!(
            moved,
            FrameOutcome::Advanced(ref report)
                if report.failure().is_none() && report.fixed_ticks_attempted() == 1
        ));
    }

    let (_, transform) = runner
        .components::<Transform2d>()
        .next()
        .ok_or("command-spawned transform should remain inspectable")?;
    assert!((transform.previous_translation().x() - 1.0).abs() < 0.000_1);
    assert!((transform.translation().x() - 2.0).abs() < 0.000_1);
    Ok(())
}

#[test]
fn cached_interpolation_query_advances_disabled_transform_history() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(Ball, Disabled)>()?;
    let camera = camera()?;
    let circle = circle(Color::WHITE)?;
    let initial = Transform2d::new(Vec2::new(3.0, 0.0))?;
    let world = application.register_world("world", move |world| {
        world.spawn(camera)?;
        world.spawn((Ball, Disabled, initial, circle))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let entity = runner
        .components::<Ball>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("disabled transform should remain inspectable")?;
    runner
        .active
        .world
        .get_mut::<Transform2d>(entity.entity())
        .ok_or("disabled transform should exist")?
        .set_translation(Vec2::new(7.0, 0.0))?;

    let active = &mut runner.active;
    assert!(capture_and_begin_fixed_interpolation(
        &mut active.world,
        &mut active.interpolation_queries,
        &mut active.previous_translations,
        &mut active.previous_camera_centers,
    ));
    let transform = active
        .world
        .get::<Transform2d>(entity.entity())
        .ok_or("disabled transform should survive capture")?;
    assert_eq!(transform.previous_translation(), Vec2::new(7.0, 0.0));
    Ok(())
}

#[test]
fn multiple_active_cameras_can_be_repaired_after_extraction_failure() -> Result<(), Box<dyn Error>>
{
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let initial_camera = camera()?;
    let extra_camera = ActiveCamera2d::new(Camera2d::new(Vec2::new(5.0, 0.0), 32.0)?);
    let world = application.register_world("repairable-camera", move |world| {
        world.spawn(initial_camera)?;
        world.insert_resource(CameraRepairPhase::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        move |cameras: Query<LogicEntityRef, With<ActiveCamera2d>>,
              mut phase: ResMut<CameraRepairPhase>,
              mut commands: LogicCommands| {
            if phase.0 == 0 {
                assert!(commands.spawn(extra_camera).is_ok());
                phase.0 = 1;
            } else if phase.0 == 1 {
                let extra = cameras
                    .iter()
                    .nth(1)
                    .expect("failed extraction should retain two cameras")
                    .handle();
                assert!(commands.despawn(extra).is_ok());
                phase.0 = 2;
            }
        },
    );

    let mut runner = application.build_headless(world)?;
    let broken = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(broken_report) = broken else {
        panic!("bounded camera-break frame should be accepted");
    };
    assert!(matches!(
        broken_report.failure(),
        Some(FrameFailure::Extraction(
            crate::ExtractionError::MultipleActiveCameras
        ))
    ));
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 2);

    let repaired = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(repaired_report) = repaired else {
        panic!("bounded camera-repair frame should be accepted");
    };
    assert!(repaired_report.failure().is_none());
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("repaired World should publish extraction")?
            .world_generation(),
        runner.world_generation()
    );
    Ok(())
}

#[test]
fn removed_active_camera_commits_and_can_be_reinserted_after_extraction_failure()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let initial_camera = camera()?;
    let world = application.register_world("removable-camera", move |world| {
        let target = world.spawn(initial_camera)?;
        world.insert_resource(InsertTarget(target))?;
        world.insert_resource(CameraRepairPhase::default())?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        |target: Res<InsertTarget>,
         mut phase: ResMut<CameraRepairPhase>,
         mut commands: LogicCommands| {
            if phase.0 == 0 {
                assert!(commands.remove::<ActiveCamera2d>(target.0).is_ok());
                phase.0 = 1;
            } else if phase.0 == 1 {
                let replacement = ActiveCamera2d::new(
                    Camera2d::new(Vec2::new(7.0, -3.0), 32.0)
                        .expect("replacement camera should be valid"),
                );
                assert!(commands.insert(target.0, replacement).is_ok());
                phase.0 = 2;
            }
        },
    );

    let mut runner = application.build_headless(world)?;
    let published_before = runner
        .extracted_frame()
        .ok_or("initial camera extraction should exist")?
        .camera()
        .center();
    let target = runner
        .resource::<InsertTarget>()
        .ok_or("camera target should exist")?
        .0;

    let broken = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(broken_report) = broken else {
        panic!("bounded camera-removal frame should be accepted");
    };
    assert!(matches!(
        broken_report.failure(),
        Some(FrameFailure::Extraction(
            crate::ExtractionError::MissingActiveCamera
        ))
    ));
    assert!(matches!(
        runner.component::<ActiveCamera2d>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("failed extraction should preserve the old publication")?
            .camera()
            .center(),
        published_before
    );

    let repaired = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(repaired_report) = repaired else {
        panic!("bounded camera-repair frame should be accepted");
    };
    assert!(repaired_report.failure().is_none());
    let camera = runner.component::<ActiveCamera2d>(target)?;
    assert_eq!(camera.previous_center(), Vec2::new(7.0, -3.0));
    assert_eq!(camera.center(), Vec2::new(7.0, -3.0));
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("repaired camera should publish")?
            .camera()
            .center(),
        Vec2::new(7.0, -3.0)
    );
    Ok(())
}

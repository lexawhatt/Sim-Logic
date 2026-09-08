use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, commands::CommandEnqueueError, prelude::*};

#[path = "../../examples/rectangle_room/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

#[test]
fn rectangle_room_interpolates_its_circle_between_four_rectangles() -> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(initial_world)?;
    let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(150),
        &pressed,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded rectangle-room frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);

    let (player, _) = runner
        .components::<game::Player>()
        .next()
        .ok_or("rectangle room should retain its player")?;
    let transform = runner.component::<Transform2d>(player)?;
    assert_eq!(transform.previous_translation(), Vec2::ZERO);
    assert_eq!(transform.translation(), Vec2::new(0.8, 0.0));

    let extracted = runner
        .extracted_frame()
        .ok_or("successful rectangle-room frame should be extracted")?;
    assert_eq!(extracted.resolved_rectangles().len(), 4);
    assert_eq!(extracted.resolved_circles().len(), 1);
    let body = extracted
        .resolved_circles()
        .iter()
        .find(|circle| circle.source() == player)
        .ok_or("player body should be extracted")?;
    assert_eq!(body.position(), Vec2::new(0.4, 0.0));
    assert_eq!(body.radius(), 0.5);
    Ok(())
}

#[test]
fn standard_rectangle_is_auto_approved_and_supplies_a_transform_unless_disabled()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Disabled>()?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let mut visual = RectangleVisual::new(Vec2::new(2.0, 4.0), Color::WHITE)?;
    visual.set_corner_radius(8.0)?;
    let initial = application.register_world("rectangle-filtering", move |world| {
        world.spawn(camera)?;
        world.spawn((Transform2d::default(), visual))?;
        world.spawn((Disabled, Transform2d::default(), visual))?;
        world.spawn(visual)?;
        Ok(())
    })?;

    let runner = application.build_headless(initial)?;
    let extracted = runner
        .extracted_frame()
        .ok_or("candidate validation should publish one rectangle")?;
    assert_eq!(extracted.resolved_rectangles().len(), 2);
    for rectangle in extracted.resolved_rectangles() {
        assert_eq!(rectangle.position(), Vec2::ZERO);
        assert_eq!(rectangle.size(), Vec2::new(2.0, 4.0));
        assert_eq!(rectangle.corner_radius(), 8.0);
    }
    Ok(())
}

#[derive(Component)]
struct CommandSpawned;

#[derive(Resource, Default)]
struct SpawnOnce(bool);

fn spawn_rectangle_once(
    mut state: ResMut<SpawnOnce>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    if state.0 {
        return Ok(());
    }
    state.0 = true;
    let mut visual = RectangleVisual::new(Vec2::new(3.0, 1.0), Color::WHITE)?;
    visual.set_corner_radius(0.2)?;
    commands.spawn((
        CommandSpawned,
        Transform2d::new(Vec2::new(7.0, -3.0))?,
        visual,
    ))?;
    Ok(())
}

fn command_spawn_application(
    limits: RenderLimits,
    initial_rectangle: bool,
) -> Result<(Application<TestAction>, WorldFactoryId), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    config.set_render_limits(limits);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<CommandSpawned>()?;
    application.add_fallible_system(Stage::FrameUpdate, spawn_rectangle_once);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let existing = RectangleVisual::new(Vec2::ONE, Color::rgb8(68, 144, 255))?;
    let initial = application.register_world("command-rectangle", move |world| {
        world.spawn(camera)?;
        world.insert_resource(SpawnOnce::default())?;
        if initial_rectangle {
            world.spawn((Transform2d::default(), existing))?;
        }
        Ok(())
    })?;
    Ok((application, initial))
}

#[test]
fn command_spawn_is_snapped_and_visible_after_the_frame_barrier() -> Result<(), Box<dyn Error>> {
    let (application, initial) = command_spawn_application(RenderLimits::default(), false)?;
    let mut runner = application.build_headless(initial)?;

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(50),
        &[],
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded command-spawn frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.spawned(), 1);
    let (entity, transform) = runner
        .components::<Transform2d>()
        .find(|(entity, _)| runner.component::<CommandSpawned>(*entity).is_ok())
        .ok_or("command-spawned rectangle should be live")?;
    assert_eq!(transform.previous_translation(), Vec2::new(7.0, -3.0));
    assert_eq!(transform.translation(), Vec2::new(7.0, -3.0));
    let extracted = runner
        .extracted_frame()
        .ok_or("successful command-spawn frame should extract")?;
    let [rectangle] = extracted.resolved_rectangles() else {
        return Err("command-spawn frame should contain one rectangle".into());
    };
    assert_eq!(rectangle.source(), entity);
    assert_eq!(rectangle.position(), Vec2::new(7.0, -3.0));
    Ok(())
}

#[test]
fn rectangle_limit_failure_keeps_the_last_complete_snapshot() -> Result<(), Box<dyn Error>> {
    let limits = RenderLimits::default().with_max_world_rectangles(1);
    let (application, initial) = command_spawn_application(limits, true)?;
    let mut runner = application.build_headless(initial)?;
    let previous_generation = runner.world_generation();
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("initial frame should exist")?
            .resolved_rectangles()
            .len(),
        1
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded over-limit frame was rejected before runtime".into());
    };
    assert_eq!(report.spawned(), 1);
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Extraction(
            ExtractionError::RectangleLimitExceeded { limit: 1 }
        ))
    ));
    assert_eq!(runner.world_generation(), previous_generation);
    assert_eq!(runner.components::<RectangleVisual>().count(), 2);
    let retained = runner
        .extracted_frame()
        .ok_or("failed extraction should retain diagnostic history")?;
    assert_eq!(retained.world_generation(), previous_generation);
    assert_eq!(retained.resolved_rectangles().len(), 1);
    Ok(())
}

#[derive(Resource)]
struct Route(WorldFactoryId);

fn replace_from_frame(
    route: Res<Route>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    let intent = commands.new_transition_intent()?;
    commands.replace_world(intent, route.0)
}

fn replacement_application(
    rectangle_limit: usize,
) -> Result<(Application<TestAction>, WorldFactoryId), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_world_rectangles(rectangle_limit));
    let mut application = Application::<TestAction>::new(config)?;
    application.add_fallible_system(Stage::FrameUpdate, replace_from_frame);

    let target_camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let target_visual = RectangleVisual::new(Vec2::new(4.0, 2.0), Color::WHITE)?;
    let target_circle = CircleVisual::new(0.25, Color::rgb8(255, 80, 80))?;
    let target_circle_transform = Transform2d::new(Vec2::new(8.0, 6.0))?;
    let target = application.register_world("rectangle-target", move |world| {
        world.spawn(target_camera)?;
        world.spawn((target_circle_transform, target_circle))?;
        world.spawn(target_visual)?;
        Ok(())
    })?;

    let old_camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let old_circle = CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?;
    let initial = application.register_world("old-circle", move |world| {
        world.spawn(old_camera)?;
        world.spawn(old_circle)?;
        world.insert_resource(Route(target))?;
        Ok(())
    })?;
    Ok((application, initial))
}

#[test]
fn rectangle_candidate_commits_or_stays_precommit_when_its_limit_rejects_it()
-> Result<(), Box<dyn Error>> {
    let (application, initial) = replacement_application(1)?;
    let mut runner = application.build_headless(initial)?;
    let old = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("valid replacement frame was rejected".into());
    };
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), old);
    let extracted = runner
        .extracted_frame()
        .ok_or("committed rectangle World should publish")?;
    assert_eq!(extracted.world_generation(), runner.world_generation());
    assert_eq!(extracted.resolved_rectangles().len(), 1);
    let [circle] = extracted.resolved_circles() else {
        return Err("valid replacement should publish its staged circle".into());
    };
    assert_eq!(circle.position(), Vec2::new(8.0, 6.0));

    let (application, initial) = replacement_application(0)?;
    let mut runner = application.build_headless(initial)?;
    let old = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("invalid candidate frame was rejected before arbitration".into());
    };
    assert!(matches!(
        report.transition(),
        FrameTransition::PreparationFailed {
            error: CandidateFailure::Extraction(ExtractionError::RectangleLimitExceeded {
                limit: 0
            }),
            ..
        }
    ));
    assert_eq!(runner.world_generation(), old);
    let retained = runner
        .extracted_frame()
        .ok_or("old complete snapshot should remain")?;
    assert_eq!(retained.world_generation(), old);
    assert_eq!(retained.resolved_circles().len(), 1);
    assert_eq!(retained.resolved_circles()[0].position(), Vec2::ZERO);
    assert_eq!(
        retained.resolved_circles()[0].source().world_generation(),
        old
    );
    assert!(retained.resolved_rectangles().is_empty());
    Ok(())
}

use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, commands::CommandEnqueueError, prelude::*};

#[path = "../../examples/acceleration_vectors/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

#[test]
fn acceleration_vector_example_interpolates_the_body_but_samples_the_current_acceleration()
-> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(initial_world)?;
    assert!(
        runner
            .extracted_frame()
            .ok_or("initial acceleration-vector frame should exist")?
            .resolved_lines()
            .is_empty(),
        "the valid zero acceleration should not require component removal"
    );

    let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(150),
        &pressed,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded acceleration-vector frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);

    let (body, acceleration) = runner
        .components::<LinearAcceleration2d>()
        .next()
        .ok_or("accelerating body should remain live")?;
    assert_eq!(acceleration.acceleration(), Vec2::new(5.0, 0.0));
    assert_eq!(
        runner.component::<LinearVelocity2d>(body)?.velocity(),
        Vec2::new(0.5, 0.0)
    );
    let transform = runner.component::<Transform2d>(body)?;
    assert_eq!(transform.previous_translation(), Vec2::ZERO);
    assert_eq!(transform.translation(), Vec2::new(0.05, 0.0));

    let extracted = runner
        .extracted_frame()
        .ok_or("successful acceleration-vector frame should be extracted")?;
    let [line] = extracted.resolved_lines() else {
        return Err("nonzero acceleration should produce one line".into());
    };
    assert_eq!(line.source(), body);
    assert_eq!(line.from(), Vec2::new(0.025, 0.0));
    assert_eq!(line.to(), Vec2::new(5.025, 0.0));

    let released = [InputEvent::key(
        PhysicalKeyCode::KeyD,
        ButtonState::Released,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &released, viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("acceleration release frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(
        runner
            .component::<LinearAcceleration2d>(body)?
            .acceleration(),
        Vec2::ZERO
    );
    assert!(
        runner
            .extracted_frame()
            .ok_or("release frame should publish")?
            .resolved_lines()
            .is_empty()
    );
    Ok(())
}

#[test]
fn standard_line_is_auto_approved_requires_only_a_transform_and_filters_disabled()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Disabled>()?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = LineVisual::new(Vec2::new(2.0, 1.0), 3.0, Color::WHITE)?;
    let zero = LineVisual::new(Vec2::ZERO, 3.0, Color::WHITE)?;
    let explicit = Transform2d::from_xy(4.0, -3.0)?;
    let initial = application.register_world("line-filtering", move |world| {
        world.spawn(camera)?;
        world.spawn((explicit, visual))?;
        world.spawn(visual)?;
        world.spawn(zero)?;
        world.spawn((Disabled, visual))?;
        Ok(())
    })?;

    let runner = application.build_headless(initial)?;
    assert_eq!(runner.components::<LineVisual>().count(), 4);
    assert_eq!(runner.components::<Transform2d>().count(), 4);
    assert_eq!(runner.components::<CircleCollider2d>().count(), 0);
    assert_eq!(runner.components::<RectangleCollider2d>().count(), 0);
    let extracted = runner
        .extracted_frame()
        .ok_or("candidate validation should publish lines")?;
    assert_eq!(extracted.resolved_lines().len(), 2);
    assert!(
        extracted.resolved_lines().iter().any(|line| {
            line.from() == Vec2::new(4.0, -3.0) && line.to() == Vec2::new(6.0, -2.0)
        })
    );
    assert!(
        extracted
            .resolved_lines()
            .iter()
            .any(|line| line.from() == Vec2::ZERO && line.to() == Vec2::new(2.0, 1.0))
    );
    Ok(())
}

#[derive(Component)]
struct EditedLine;

#[derive(Resource)]
struct LineEdit {
    target: LogicEntity,
    phase: u8,
}

fn edit_line(mut state: ResMut<LineEdit>, mut commands: Commands) -> LogicResult {
    match state.phase {
        0 => commands.insert(
            state.target,
            LineVisual::new(Vec2::new(2.0, 1.0), 2.0, Color::WHITE)?,
        )?,
        1 => commands.remove::<LineVisual>(state.target)?,
        _ => return Ok(()),
    }
    state.phase = state.phase.saturating_add(1);
    Ok(())
}

#[test]
fn command_insert_and_remove_cross_frame_barriers_without_stale_lines() -> Result<(), Box<dyn Error>>
{
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<EditedLine>()?;
    application.add_fallible_system(Stage::FrameUpdate, edit_line);
    let camera = ActiveCamera2d::centered(20.0)?;
    let transform = Transform2d::from_xy(3.0, -2.0)?;
    let initial = application.register_world("line-command-edit", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((EditedLine, transform))?;
        world.insert_resource(LineEdit { target, phase: 0 })?;
        Ok(())
    })?;

    let mut runner = application.build_headless(initial)?;
    assert!(
        runner
            .extracted_frame()
            .ok_or("initial command-edit frame should exist")?
            .resolved_lines()
            .is_empty()
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("line insert frame was rejected".into());
    };
    assert!(report.failure().is_none());
    let [line] = runner
        .extracted_frame()
        .ok_or("line insert should publish")?
        .resolved_lines()
    else {
        return Err("line insert should be visible after its barrier".into());
    };
    assert_eq!(line.from(), Vec2::new(3.0, -2.0));
    assert_eq!(line.to(), Vec2::new(5.0, -1.0));

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("line removal frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert!(
        runner
            .extracted_frame()
            .ok_or("line removal should publish")?
            .resolved_lines()
            .is_empty()
    );
    assert_eq!(runner.components::<EditedLine>().count(), 1);
    assert_eq!(runner.components::<Transform2d>().count(), 1);
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
    line_limit: usize,
) -> Result<(Application<TestAction>, WorldFactoryId), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_world_lines(line_limit));
    let mut application = Application::<TestAction>::new(config)?;
    application.add_fallible_system(Stage::FrameUpdate, replace_from_frame);

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target_line = LineVisual::new(Vec2::new(4.0, 2.0), 2.0, Color::WHITE)?;
    let target = application.register_world("line-target", move |world| {
        world.spawn(target_camera)?;
        world.spawn(target_line)?;
        Ok(())
    })?;

    let old_camera = ActiveCamera2d::centered(20.0)?;
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
fn line_candidate_commits_or_stays_precommit_when_its_limit_rejects_it()
-> Result<(), Box<dyn Error>> {
    let (application, initial) = replacement_application(1)?;
    let mut runner = application.build_headless(initial)?;
    let old = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("valid line replacement frame was rejected".into());
    };
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), old);
    let extracted = runner
        .extracted_frame()
        .ok_or("committed line World should publish")?;
    assert_eq!(extracted.world_generation(), runner.world_generation());
    assert_eq!(extracted.resolved_lines().len(), 1);
    assert!(extracted.resolved_circles().is_empty());

    let (application, initial) = replacement_application(0)?;
    let mut runner = application.build_headless(initial)?;
    let old = runner.world_generation();
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("invalid line candidate frame was rejected before arbitration".into());
    };
    assert!(matches!(
        report.transition(),
        FrameTransition::PreparationFailed {
            error: CandidateFailure::Extraction(ExtractionError::LineLimitExceeded { limit: 0 }),
            ..
        }
    ));
    assert_eq!(runner.world_generation(), old);
    let retained = runner
        .extracted_frame()
        .ok_or("old complete snapshot should remain")?;
    assert_eq!(retained.world_generation(), old);
    assert_eq!(retained.resolved_circles().len(), 1);
    assert!(retained.resolved_lines().is_empty());
    Ok(())
}

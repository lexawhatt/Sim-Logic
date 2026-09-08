use std::{error::Error, time::Duration};

use sim_logic::bevy_ecs::entity_disabling::Disabled;
use sim_logic::prelude::*;

#[path = "../../examples/camera_follow/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct InertOrigin;

#[derive(Component)]
struct InertExplicit;

#[derive(Component)]
struct OrderTarget;

#[derive(Component)]
struct ExtraCamera;

#[derive(Component)]
struct DiscardedCommand;

#[derive(Resource)]
struct LaterRan(bool);

#[derive(Resource)]
struct SpawnFollowOnce(bool);

#[derive(Resource)]
struct ReplacementRoute(WorldFactoryId);

#[derive(Resource)]
struct CameraRepair {
    original: LogicEntity,
    phase: u8,
}

fn test_application() -> Result<Application<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    Ok(Application::new(config)?)
}

fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

fn advance(runner: &mut HeadlessRunner<TestAction>) -> Result<LogicFrameReport, Box<dyn Error>> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?))
    else {
        return Err("bounded camera-follow frame was rejected".into());
    };
    Ok(report)
}

fn only<T: Component>(runner: &HeadlessRunner<TestAction>) -> Result<LogicEntity, Box<dyn Error>> {
    let mut matches = runner.components::<T>();
    let entity = matches.next().ok_or("expected one matching entity")?.0;
    if matches.next().is_some() {
        return Err("expected exactly one matching entity".into());
    }
    Ok(entity)
}

fn camera_center(runner: &HeadlessRunner<TestAction>) -> Result<Vec2, Box<dyn Error>> {
    let camera = only::<ActiveCamera2d>(runner)?;
    Ok(runner.component::<ActiveCamera2d>(camera)?.center())
}

fn assert_vec2_bits(actual: Vec2, expected: Vec2) {
    assert_eq!(actual.x().to_bits(), expected.x().to_bits());
    assert_eq!(actual.y().to_bits(), expected.y().to_bits());
}

#[test]
fn camera_and_player_share_fixed_step_interpolation() -> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(initial_world)?;
    let viewport = LogicalViewport::new(800.0, 600.0)?;

    let pressed = [InputEvent::key(
        PhysicalKeyCode::ArrowRight,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(150),
        &pressed,
        viewport,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded camera-follow frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);

    let (player_entity, _) = runner
        .components::<CameraFollowTarget2d>()
        .next()
        .ok_or("the World should contain one player")?;
    let player = runner.component::<Transform2d>(player_entity)?;
    assert_eq!(player.previous_translation(), Vec2::ZERO);
    assert_eq!(player.translation(), Vec2::new(0.8, 0.0));

    let (_, camera) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("the World should contain one active camera")?;
    assert_eq!(camera.previous_center(), Vec2::new(0.0, 2.0));
    assert_eq!(camera.center(), Vec2::new(0.8, 2.0));

    let extracted = runner
        .extracted_frame()
        .ok_or("successful camera-follow frame should be extracted")?;
    assert_eq!(extracted.camera().center(), Vec2::new(0.4, 2.0));
    let player_on_screen = extracted
        .resolved_circles()
        .iter()
        .find(|circle| circle.source() == player_entity)
        .ok_or("the extracted scene should contain the player")?;
    assert_eq!(player_on_screen.position(), Vec2::new(0.4, 0.0));

    let zero_tick =
        runner.advance_frame(FrameRequest::new(Duration::from_millis(25), &[], viewport));
    let FrameOutcome::Advanced(report) = zero_tick else {
        return Err("bounded zero-tick camera frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 0);
    let extracted = runner
        .extracted_frame()
        .ok_or("zero-tick camera frame should be extracted")?;
    assert_eq!(extracted.camera().center(), Vec2::new(0.6, 2.0));

    Ok(())
}

#[test]
fn target_is_auto_approved_requires_a_transform_and_is_inert_without_registration()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application()?;
    application.approve_components::<(InertOrigin, InertExplicit)>()?;
    let initial_center = Vec2::new(9.0, -4.0);
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let origin_target = CameraFollowTarget2d::default();
    let explicit_target = CameraFollowTarget2d::new(Vec2::new(1.0, -2.0))?;
    let explicit_transform = Transform2d::from_xy(3.0, 5.0)?;
    let world = application.register_world("inert-camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((InertOrigin, origin_target))?;
        world.spawn((InertExplicit, explicit_transform, explicit_target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let origin = only::<InertOrigin>(&runner)?;
    assert_vec2_bits(
        runner.component::<Transform2d>(origin)?.translation(),
        Vec2::ZERO,
    );
    let explicit = only::<InertExplicit>(&runner)?;
    assert_eq!(
        runner.component::<Transform2d>(explicit)?.translation(),
        Vec2::new(3.0, 5.0)
    );
    assert_eq!(
        runner
            .component::<CameraFollowTarget2d>(explicit)?
            .camera_offset(),
        Vec2::new(1.0, -2.0)
    );

    let report = advance(&mut runner)?;
    assert!(report.failure().is_none());
    assert_eq!(camera_center(&runner)?, initial_center);
    Ok(())
}

fn move_order_target(mut target: Single<&mut Transform2d, With<OrderTarget>>) -> LogicResult {
    target.translate_by(Vec2::new(1.0, 0.0))?;
    Ok(())
}

fn run_order_case(mode: u8) -> Result<(Vec2, Vec2, Vec2), Box<dyn Error>> {
    let mut application = test_application()?;
    application.approve_component::<OrderTarget>()?;
    match mode {
        0 => {
            application.add_fallible_fixed_system(move_order_target);
            application.add_camera_follow2d_system();
        }
        1 => {
            application.add_camera_follow2d_system();
            application.add_fallible_fixed_system(move_order_target);
        }
        2 => {
            application.add_camera_follow2d_system();
            application.add_fallible_fixed_system(move_order_target);
            application.add_camera_follow2d_system();
        }
        _ => return Err("unknown camera-follow order case".into()),
    }
    let initial_center = Vec2::new(0.0, 2.0);
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let follow = CameraFollowTarget2d::new(Vec2::new(0.0, 2.0))?;
    let world = application.register_world("camera-follow-order", move |world| {
        world.spawn(camera)?;
        world.spawn((OrderTarget, follow))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let report = advance(&mut runner)?;
    assert!(report.failure().is_none());
    let target = only::<OrderTarget>(&runner)?;
    let camera = only::<ActiveCamera2d>(&runner)?;
    let camera = runner.component::<ActiveCamera2d>(camera)?;
    Ok((
        runner.component::<Transform2d>(target)?.translation(),
        camera.previous_center(),
        camera.center(),
    ))
}

#[test]
fn adapter_observes_explicit_order_and_duplicate_registration() -> Result<(), Box<dyn Error>> {
    assert_eq!(
        run_order_case(0)?,
        (
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(1.0, 2.0)
        )
    );
    assert_eq!(
        run_order_case(1)?,
        (
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(0.0, 2.0)
        )
    );
    assert_eq!(run_order_case(2)?, run_order_case(0)?);
    Ok(())
}

fn mark_later(mut later: ResMut<LaterRan>) {
    later.0 = true;
}

fn queue_discarded_command(mut commands: Commands) -> LogicResult {
    commands.spawn(DiscardedCommand)?;
    Ok(())
}

#[test]
fn zero_and_disabled_targets_are_ignored_while_multiple_targets_fail_closed()
-> Result<(), Box<dyn Error>> {
    let mut no_target_application = test_application()?;
    no_target_application.add_camera_follow2d_system();
    let initial_center = Vec2::new(-0.0, f32::from_bits(1));
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let no_target_world =
        no_target_application.register_world("no-camera-follow-target", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
    let mut no_target_runner = no_target_application.build_headless(no_target_world)?;
    assert!(advance(&mut no_target_runner)?.failure().is_none());
    assert_vec2_bits(camera_center(&no_target_runner)?, initial_center);

    let mut disabled_application = test_application()?;
    disabled_application.add_camera_follow2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let enabled_transform = Transform2d::new(Vec2::new(-0.0, f32::from_bits(1)))?;
    let disabled_follow = CameraFollowTarget2d::new(Vec2::new(5.0, 5.0))?;
    let disabled_transform = Transform2d::from_xy(99.0, 99.0)?;
    let world = disabled_application.register_world("disabled-camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((CameraFollowTarget2d::default(), enabled_transform))?;
        world.spawn((Disabled, disabled_follow, disabled_transform))?;
        Ok(())
    })?;
    let mut disabled_runner = disabled_application.build_headless(world)?;
    assert!(advance(&mut disabled_runner)?.failure().is_none());
    assert_vec2_bits(
        camera_center(&disabled_runner)?,
        enabled_transform.translation(),
    );

    let mut multiple_application = test_application()?;
    multiple_application.approve_component::<DiscardedCommand>()?;
    multiple_application.add_fallible_fixed_system(queue_discarded_command);
    multiple_application.add_camera_follow2d_system();
    multiple_application.add_fixed_system(mark_later);
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let first_target = Transform2d::from_xy(1.0, 0.0)?;
    let second_target = Transform2d::from_xy(2.0, 0.0)?;
    let world = multiple_application.register_world("multiple-camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((CameraFollowTarget2d::default(), first_target))?;
        world.spawn((CameraFollowTarget2d::default(), second_target))?;
        world.insert_resource(LaterRan(false))?;
        Ok(())
    })?;
    let mut multiple_runner = multiple_application.build_headless(world)?;
    let report = advance(&mut multiple_runner)?;
    let failure = report
        .failure()
        .ok_or("multiple follow targets should fail the fixed stage")?;
    assert!(failure.to_string().contains("more than one enabled target"));
    assert_vec2_bits(camera_center(&multiple_runner)?, initial_center);
    assert!(
        !multiple_runner
            .resource::<LaterRan>()
            .ok_or("later-run state should remain available")?
            .0
    );
    assert_eq!(multiple_runner.components::<DiscardedCommand>().count(), 0);
    Ok(())
}

fn spawn_follow_once(mut state: ResMut<SpawnFollowOnce>, mut commands: Commands) -> LogicResult {
    if state.0 {
        return Ok(());
    }
    commands.spawn((
        CameraFollowTarget2d::new(Vec2::new(1.0, -1.0))?,
        Transform2d::from_xy(2.0, 3.0)?,
    ))?;
    state.0 = true;
    Ok(())
}

#[test]
fn command_spawned_first_target_follows_only_after_its_stage_barrier() -> Result<(), Box<dyn Error>>
{
    let mut application = test_application()?;
    application.add_fallible_fixed_system(spawn_follow_once);
    application.add_camera_follow2d_system();
    let initial_center = Vec2::new(9.0, 9.0);
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let world = application.register_world("command-camera-follow", move |world| {
        world.spawn(camera)?;
        world.insert_resource(SpawnFollowOnce(false))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let first = advance(&mut runner)?;
    assert!(first.failure().is_none());
    assert_eq!(first.spawned(), 1);
    assert_eq!(runner.components::<CameraFollowTarget2d>().count(), 1);
    assert_eq!(camera_center(&runner)?, initial_center);

    let second = advance(&mut runner)?;
    assert!(second.failure().is_none());
    assert_eq!(camera_center(&runner)?, Vec2::new(3.0, 2.0));
    Ok(())
}

fn request_replacement(
    route: Option<Res<ReplacementRoute>>,
    mut commands: Commands,
) -> LogicResult {
    let Some(route) = route else {
        return Ok(());
    };
    let intent = commands.new_transition_intent()?;
    commands.replace_world(intent, route.0)?;
    Ok(())
}

#[test]
fn replacement_keeps_factory_camera_until_the_first_later_fixed_tick() -> Result<(), Box<dyn Error>>
{
    let mut application = test_application()?;
    application.add_fallible_fixed_system(request_replacement);
    application.add_camera_follow2d_system();
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let candidate_center = Vec2::new(5.0, 5.0);
    let candidate_camera = ActiveCamera2d::new(Camera2d::new(candidate_center, 20.0)?);
    let follow = CameraFollowTarget2d::new(Vec2::new(1.0, -1.0))?;
    let candidate_target = Transform2d::from_xy(2.0, 3.0)?;
    let target = application.register_world("camera-follow-target", move |world| {
        world.spawn(candidate_camera)?;
        world.spawn((follow, candidate_target))?;
        Ok(())
    })?;
    let source = application.register_world("camera-follow-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(ReplacementRoute(target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let transition = advance(&mut runner)?;
    assert!(transition.failure().is_none());
    assert!(matches!(
        transition.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_eq!(camera_center(&runner)?, candidate_center);

    let followed = advance(&mut runner)?;
    assert!(followed.failure().is_none());
    assert_eq!(camera_center(&runner)?, Vec2::new(3.0, 2.0));
    Ok(())
}

fn cycle_camera_cardinality(
    mut repair: ResMut<CameraRepair>,
    extras: Query<LogicEntityRef, With<ExtraCamera>>,
    mut commands: Commands,
) -> LogicResult {
    match repair.phase {
        0 => {
            let extra = ActiveCamera2d::new(Camera2d::new(Vec2::new(4.0, 4.0), 20.0)?);
            commands.spawn((ExtraCamera, extra))?;
        }
        1 => {
            let extra = extras.single()?.handle();
            commands.despawn(extra)?;
        }
        2 => commands.remove::<ActiveCamera2d>(repair.original)?,
        3 => {
            let replacement = ActiveCamera2d::new(Camera2d::new(Vec2::new(7.0, -3.0), 20.0)?);
            commands.insert(repair.original, replacement)?;
        }
        _ => return Ok(()),
    }
    repair.phase += 1;
    Ok(())
}

#[test]
fn camera_cardinality_repair_commands_are_not_blocked_by_the_follow_adapter()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application()?;
    application.approve_component::<ExtraCamera>()?;
    application.add_fallible_fixed_system(cycle_camera_cardinality);
    application.add_camera_follow2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let target = CameraFollowTarget2d::new(Vec2::new(1.0, 2.0))?;
    let target_transform = Transform2d::from_xy(2.0, 3.0)?;
    let world = application.register_world("repair-camera-cardinality", move |world| {
        let original = world.spawn(camera)?;
        world.spawn((target, target_transform))?;
        world.insert_resource(CameraRepair { original, phase: 0 })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let duplicate = advance(&mut runner)?;
    assert!(duplicate.failure().is_some());
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 2);

    let duplicate_repaired = advance(&mut runner)?;
    assert!(duplicate_repaired.failure().is_none());
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);

    let missing = advance(&mut runner)?;
    assert!(missing.failure().is_some());
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 0);

    let missing_repaired = advance(&mut runner)?;
    assert!(missing_repaired.failure().is_none());
    assert_eq!(camera_center(&runner)?, Vec2::new(7.0, -3.0));

    let followed = advance(&mut runner)?;
    assert!(followed.failure().is_none());
    assert_eq!(camera_center(&runner)?, Vec2::new(3.0, 5.0));
    Ok(())
}

#[test]
fn computed_center_overflow_is_atomic_and_stops_later_systems() -> Result<(), Box<dyn Error>> {
    let mut application = test_application()?;
    application.add_camera_follow2d_system();
    application.add_fixed_system(mark_later);
    let initial_center = Vec2::new(4.0, -2.0);
    let camera = ActiveCamera2d::new(Camera2d::new(initial_center, 20.0)?);
    let follow = CameraFollowTarget2d::new(Vec2::splat(f32::MAX))?;
    let transform = Transform2d::new(Vec2::splat(f32::MAX))?;
    let world = application.register_world("overflow-camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((follow, transform))?;
        world.insert_resource(LaterRan(false))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let report = advance(&mut runner)?;
    let failure = report
        .failure()
        .ok_or("overflowing follow center should fail the fixed stage")?;
    assert!(failure.to_string().contains("camera center must be finite"));
    assert_eq!(camera_center(&runner)?, initial_center);
    assert!(
        !runner
            .resource::<LaterRan>()
            .ok_or("later-run state should remain available")?
            .0
    );
    Ok(())
}

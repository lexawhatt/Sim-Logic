use std::{error::Error, time::Duration};

use sim_logic::bevy_ecs::entity_disabling::Disabled;
use sim_logic::prelude::*;

const STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct InertBody;

#[derive(Component)]
struct ActiveBody;

#[derive(Component)]
struct ZeroBody;

#[derive(Component)]
struct DisabledBody;

#[derive(Component)]
struct OrderBody;

#[derive(Component)]
struct InsertedBody;

#[derive(Component)]
struct SpawnedBody;

#[derive(Component)]
struct ReplacementBody;

#[derive(Component)]
struct AcceleratedBeforeOverflowBody;

#[derive(Component)]
struct OverflowingBody;

#[derive(Resource)]
struct QueueState {
    target: LogicEntity,
    issued: bool,
}

#[derive(Resource)]
struct ReplacementRoute(WorldFactoryId);

fn application(max_catch_up_ticks: u32) -> Result<Application<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, max_catch_up_ticks)?);
    Ok(Application::new(config)?)
}

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, &[], viewport))
    else {
        return Err("bounded acceleration frame was rejected".into());
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

fn assert_vec2_bits(actual: Vec2, expected: Vec2) {
    assert_eq!(actual.x().to_bits(), expected.x().to_bits());
    assert_eq!(actual.y().to_bits(), expected.y().to_bits());
}

#[test]
fn acceleration_is_auto_approved_transitively_supplies_motion_and_is_inert_without_registration()
-> Result<(), Box<dyn Error>> {
    let mut application = application(1)?;
    application.approve_component::<InertBody>()?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let acceleration = LinearAcceleration2d::new(Vec2::new(10.0, -5.0))?;
    let explicit_transform = Transform2d::from_xy(4.0, -3.0)?;
    let explicit_velocity = LinearVelocity2d::new(Vec2::new(2.0, 1.0))?;
    let world = application.register_world("inert-acceleration", move |world| {
        world.spawn(camera)?;
        world.spawn((InertBody, acceleration))?;
        world.spawn((
            InertBody,
            explicit_transform,
            explicit_velocity,
            acceleration,
        ))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    assert_eq!(runner.components::<LinearAcceleration2d>().count(), 2);
    assert_eq!(runner.components::<LinearVelocity2d>().count(), 2);
    assert_eq!(runner.components::<Transform2d>().count(), 2);
    assert!(
        runner
            .components::<Transform2d>()
            .any(|(_, transform)| transform.translation() == Vec2::ZERO)
    );
    assert!(
        runner
            .components::<Transform2d>()
            .any(|(_, transform)| transform.translation() == Vec2::new(4.0, -3.0))
    );
    assert!(
        runner
            .components::<LinearVelocity2d>()
            .any(|(_, velocity)| velocity.velocity() == Vec2::ZERO)
    );
    assert!(
        runner
            .components::<LinearVelocity2d>()
            .any(|(_, velocity)| velocity.velocity() == Vec2::new(2.0, 1.0))
    );

    let report = advance(&mut runner, STEP)?;
    assert!(report.failure().is_none());
    assert!(
        runner
            .components::<Transform2d>()
            .any(|(_, transform)| transform.translation() == Vec2::ZERO)
    );
    assert!(
        runner
            .components::<Transform2d>()
            .any(|(_, transform)| transform.translation() == Vec2::new(4.0, -3.0))
    );
    Ok(())
}

#[test]
fn registered_acceleration_handles_catch_up_zero_and_disabled_without_moving_transforms()
-> Result<(), Box<dyn Error>> {
    let mut application = application(4)?;
    application.approve_components::<(ActiveBody, ZeroBody, DisabledBody)>()?;
    application.add_linear_acceleration2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let acceleration = LinearAcceleration2d::new(Vec2::new(10.0, -20.0))?;
    let zero = LinearAcceleration2d::new(Vec2::ZERO)?;
    let signed_velocity = LinearVelocity2d::new(Vec2::new(-0.0, f32::from_bits(1)))?;
    let disabled_velocity = LinearVelocity2d::new(Vec2::new(3.0, 4.0))?;
    let world = application.register_world("registered-acceleration", move |world| {
        world.spawn(camera)?;
        world.spawn((ActiveBody, acceleration))?;
        world.spawn((ZeroBody, zero, signed_velocity))?;
        world.spawn((DisabledBody, Disabled, acceleration, disabled_velocity))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let report = advance(&mut runner, STEP.saturating_mul(2))?;
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 2);

    let active = only::<ActiveBody>(&runner)?;
    let mut expected = Vec2::ZERO;
    for _ in 0..2 {
        expected += Vec2::new(10.0, -20.0) * STEP.as_secs_f32();
    }
    assert_vec2_bits(
        runner.component::<LinearVelocity2d>(active)?.velocity(),
        expected,
    );
    assert_eq!(
        runner.component::<Transform2d>(active)?.translation(),
        Vec2::ZERO,
        "the acceleration adapter must not move Transforms itself"
    );

    let zero = only::<ZeroBody>(&runner)?;
    assert_vec2_bits(
        runner.component::<LinearVelocity2d>(zero)?.velocity(),
        signed_velocity.velocity(),
    );
    let disabled = only::<DisabledBody>(&runner)?;
    assert_vec2_bits(
        runner.component::<LinearVelocity2d>(disabled)?.velocity(),
        disabled_velocity.velocity(),
    );
    Ok(())
}

fn set_motion(body: Single<(&mut LinearAcceleration2d, &mut LinearVelocity2d), With<OrderBody>>) {
    let (mut acceleration, mut velocity) = body.into_inner();
    acceleration
        .set_acceleration(Vec2::new(20.0, 0.0))
        .expect("test acceleration should be finite");
    velocity
        .set_velocity(Vec2::new(2.0, 0.0))
        .expect("test velocity should be finite");
}

fn run_order_case(mode: u8) -> Result<(Vec2, Vec2), Box<dyn Error>> {
    let mut application = application(1)?;
    application.approve_component::<OrderBody>()?;
    let acceleration = LinearAcceleration2d::new(Vec2::new(10.0, 0.0))?;
    let velocity = LinearVelocity2d::new(Vec2::new(1.0, 0.0))?;
    match mode {
        0 => {
            application.add_linear_acceleration2d_system();
            application.add_linear_velocity2d_system();
        }
        1 => {
            application.add_linear_velocity2d_system();
            application.add_linear_acceleration2d_system();
        }
        2 => {
            application.add_linear_acceleration2d_system();
            application.add_linear_acceleration2d_system();
            application.add_linear_velocity2d_system();
        }
        3 => {
            application.add_fixed_system(set_motion);
            application.add_linear_acceleration2d_system();
            application.add_linear_velocity2d_system();
        }
        _ => return Err("unknown order case".into()),
    }
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("acceleration-order", move |world| {
        world.spawn(camera)?;
        world.spawn((OrderBody, acceleration, velocity))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let report = advance(&mut runner, STEP)?;
    assert!(report.failure().is_none());
    let body = only::<OrderBody>(&runner)?;
    Ok((
        runner.component::<LinearVelocity2d>(body)?.velocity(),
        runner.component::<Transform2d>(body)?.translation(),
    ))
}

#[test]
fn acceleration_and_velocity_compose_in_explicit_order_with_duplicates_and_prior_writes()
-> Result<(), Box<dyn Error>> {
    assert_eq!(
        run_order_case(0)?,
        (Vec2::new(2.0, 0.0), Vec2::new(0.2, 0.0))
    );
    assert_eq!(
        run_order_case(1)?,
        (Vec2::new(2.0, 0.0), Vec2::new(0.1, 0.0))
    );
    assert_eq!(
        run_order_case(2)?,
        (Vec2::new(3.0, 0.0), Vec2::new(0.3, 0.0))
    );
    assert_eq!(
        run_order_case(3)?,
        (Vec2::new(4.0, 0.0), Vec2::new(0.4, 0.0))
    );
    Ok(())
}

fn queue_acceleration_once(mut state: ResMut<QueueState>, mut commands: Commands) -> LogicResult {
    if state.issued {
        return Ok(());
    }
    let acceleration = LinearAcceleration2d::new(Vec2::new(10.0, 0.0))?;
    commands.insert(state.target, acceleration)?;
    commands.spawn((SpawnedBody, acceleration))?;
    state.issued = true;
    Ok(())
}

#[test]
fn deferred_acceleration_insert_and_spawn_first_run_after_the_barrier() -> Result<(), Box<dyn Error>>
{
    let mut application = application(1)?;
    application.approve_components::<(InsertedBody, SpawnedBody)>()?;
    application.add_fallible_fixed_system(queue_acceleration_once);
    application.add_linear_acceleration2d_system();
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("deferred-acceleration", move |world| {
        world.spawn(camera)?;
        let target = world.spawn(InsertedBody)?;
        world.insert_resource(QueueState {
            target,
            issued: false,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let first = advance(&mut runner, STEP)?;
    assert!(first.failure().is_none());
    assert_eq!(first.spawned(), 1);
    for body in [
        only::<InsertedBody>(&runner)?,
        only::<SpawnedBody>(&runner)?,
    ] {
        assert_eq!(
            runner.component::<Transform2d>(body)?.translation(),
            Vec2::ZERO
        );
        assert_eq!(
            runner.component::<LinearVelocity2d>(body)?.velocity(),
            Vec2::ZERO
        );
    }

    let second = advance(&mut runner, STEP)?;
    assert!(second.failure().is_none());
    for body in [
        only::<InsertedBody>(&runner)?,
        only::<SpawnedBody>(&runner)?,
    ] {
        assert_eq!(
            runner.component::<LinearVelocity2d>(body)?.velocity(),
            Vec2::new(1.0, 0.0)
        );
        assert_eq!(
            runner.component::<Transform2d>(body)?.translation(),
            Vec2::new(0.1, 0.0)
        );
    }
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
fn replacement_world_accelerates_only_after_commit_on_its_first_fixed_tick()
-> Result<(), Box<dyn Error>> {
    let mut application = application(1)?;
    application.approve_component::<ReplacementBody>()?;
    application.add_fallible_fixed_system(request_replacement);
    application.add_linear_acceleration2d_system();
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let acceleration = LinearAcceleration2d::new(Vec2::new(10.0, 0.0))?;
    let target = application.register_world("acceleration-target", move |world| {
        world.spawn(camera)?;
        world.spawn((ReplacementBody, acceleration))?;
        Ok(())
    })?;
    let source = application.register_world("acceleration-source", move |world| {
        world.spawn(camera)?;
        world.insert_resource(ReplacementRoute(target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let transition = advance(&mut runner, STEP)?;
    assert!(transition.failure().is_none());
    assert!(matches!(
        transition.transition(),
        FrameTransition::Committed { .. }
    ));
    let body = only::<ReplacementBody>(&runner)?;
    assert_eq!(
        runner.component::<Transform2d>(body)?.translation(),
        Vec2::ZERO
    );

    let next = advance(&mut runner, STEP)?;
    assert!(next.failure().is_none());
    assert_eq!(
        runner.component::<LinearVelocity2d>(body)?.velocity(),
        Vec2::new(1.0, 0.0)
    );
    assert_eq!(
        runner.component::<Transform2d>(body)?.translation(),
        Vec2::new(0.1, 0.0)
    );
    Ok(())
}

#[test]
fn acceleration_overflow_keeps_the_failing_row_atomic_without_rolling_back_earlier_rows()
-> Result<(), Box<dyn Error>> {
    let mut application = application(1)?;
    application.approve_components::<(AcceleratedBeforeOverflowBody, OverflowingBody)>()?;
    application.add_linear_acceleration2d_system();
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let ordinary_acceleration = LinearAcceleration2d::new(Vec2::new(10.0, 0.0))?;
    let overflowing_acceleration = LinearAcceleration2d::new(Vec2::splat(f32::MAX))?;
    let overflowing_velocity = LinearVelocity2d::new(Vec2::splat(f32::MAX))?;
    let earlier_transform = Transform2d::from_xy(-4.0, 1.0)?;
    let failing_transform = Transform2d::from_xy(2.0, -3.0)?;
    let world = application.register_world("overflow-acceleration", move |world| {
        world.spawn(camera)?;
        // Separate marker archetypes make this fixture's processed-before-
        // failure row explicit without assigning a public order to entities.
        world.spawn((
            AcceleratedBeforeOverflowBody,
            ordinary_acceleration,
            earlier_transform,
        ))?;
        world.spawn((
            OverflowingBody,
            overflowing_acceleration,
            overflowing_velocity,
            failing_transform,
        ))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let earlier = only::<AcceleratedBeforeOverflowBody>(&runner)?;
    let failing = only::<OverflowingBody>(&runner)?;

    let report = advance(&mut runner, STEP)?;
    let failure = report
        .failure()
        .ok_or("velocity overflow should fail acceleration integration")?;
    assert!(
        failure
            .to_string()
            .contains("linear velocity must be finite")
    );
    assert_eq!(
        runner.component::<LinearVelocity2d>(earlier)?.velocity(),
        Vec2::new(1.0, 0.0),
        "an earlier successful row is not rolled back"
    );
    assert_eq!(
        runner.component::<LinearVelocity2d>(failing)?.velocity(),
        Vec2::splat(f32::MAX)
    );
    assert_eq!(
        *runner.component::<Transform2d>(earlier)?,
        earlier_transform,
        "the later velocity System must not run after acceleration fails"
    );
    assert_eq!(
        *runner.component::<Transform2d>(failing)?,
        failing_transform
    );
    Ok(())
}

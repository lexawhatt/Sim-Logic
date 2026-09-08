use std::{error::Error, time::Duration};

use sim_logic::bevy_ecs::entity_disabling::Disabled;
use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct MovingBody;

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
struct OverflowBody;

#[derive(Resource)]
struct Handles {
    cardinal: LogicEntity,
    diagonal: LogicEntity,
    disabled: LogicEntity,
}

#[derive(Resource, Default)]
struct OrderObservation(Vec2);

#[derive(Resource)]
struct QueueState {
    target: LogicEntity,
    issued: bool,
}

#[derive(Resource)]
struct ReplacementRoute(WorldFactoryId);

const STEP: Duration = Duration::from_millis(100);

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, &[], viewport))
    else {
        return Err("bounded motion frame was rejected".into());
    };
    Ok(report)
}

fn transform(
    runner: &HeadlessRunner<TestAction>,
    entity: LogicEntity,
) -> Result<Transform2d, Box<dyn Error>> {
    Ok(*runner.component::<Transform2d>(entity)?)
}

fn test_application(max_catch_up_ticks: u32) -> Result<Application<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, max_catch_up_ticks)?);
    Ok(Application::new(config)?)
}

#[test]
fn velocity_is_inert_until_explicit_registration_and_supplies_a_transform()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<MovingBody>()?;
    let velocity = LinearVelocity2d::new(Vec2::new(10.0, 0.0))?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("inert-velocity", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((MovingBody, velocity))?;
        world.insert_resource(Handles {
            cardinal: body,
            diagonal: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .cardinal;

    let report = advance(&mut runner, STEP)?;

    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(transform(&runner, body)?, Transform2d::default());
    Ok(())
}

#[test]
fn registered_velocity_integrates_each_catch_up_tick_and_excludes_disabled()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(4)?;
    application.approve_components::<(MovingBody, DisabledBody)>()?;
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let cardinal_velocity = LinearVelocity2d::new(Vec2::new(10.0, 0.0))?;
    let diagonal_velocity = LinearVelocity2d::new(Vec2::new(-20.0, 10.0))?;
    let diagonal_transform = Transform2d::from_xy(1.0, 2.0)?;
    let disabled_transform = Transform2d::from_xy(5.0, 6.0)?;
    let world = application.register_world("integrated-velocity", move |world| {
        world.spawn(camera)?;
        let cardinal = world.spawn((MovingBody, cardinal_velocity))?;
        let diagonal = world.spawn((MovingBody, diagonal_transform, diagonal_velocity))?;
        let disabled = world.spawn((
            DisabledBody,
            Disabled,
            disabled_transform,
            cardinal_velocity,
        ))?;
        world.insert_resource(Handles {
            cardinal,
            diagonal,
            disabled,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let handles = {
        let handles = runner.resource::<Handles>().ok_or("missing handles")?;
        Handles {
            cardinal: handles.cardinal,
            diagonal: handles.diagonal,
            disabled: handles.disabled,
        }
    };

    let zero = advance(&mut runner, Duration::ZERO)?;
    assert_eq!(zero.fixed_ticks_attempted(), 0);
    assert_eq!(
        transform(&runner, handles.cardinal)?,
        Transform2d::default()
    );

    let catch_up = advance(&mut runner, STEP.saturating_mul(2))?;

    assert!(catch_up.failure().is_none());
    assert_eq!(catch_up.fixed_ticks_attempted(), 2);
    let cardinal = transform(&runner, handles.cardinal)?;
    assert_eq!(cardinal.previous_translation(), Vec2::new(1.0, 0.0));
    assert_eq!(cardinal.translation(), Vec2::new(2.0, 0.0));
    let diagonal = transform(&runner, handles.diagonal)?;
    assert_eq!(diagonal.previous_translation(), Vec2::new(-1.0, 3.0));
    assert_eq!(diagonal.translation(), Vec2::new(-3.0, 4.0));
    let disabled = transform(&runner, handles.disabled)?;
    assert_eq!(disabled.previous_translation(), Vec2::new(5.0, 6.0));
    assert_eq!(disabled.translation(), Vec2::new(5.0, 6.0));
    Ok(())
}

fn choose_velocity(mut velocity: Single<&mut LinearVelocity2d, With<OrderBody>>) {
    velocity
        .set_velocity(Vec2::new(10.0, 0.0))
        .expect("test velocity should be finite");
}

fn observe_integrated_position(
    transform: Single<&Transform2d, With<OrderBody>>,
    mut observation: ResMut<OrderObservation>,
) {
    observation.0 = transform.translation();
}

#[test]
fn integration_uses_registration_order_and_duplicate_registration_runs_twice()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<OrderBody>()?;
    application.add_fixed_system(choose_velocity);
    application.add_linear_velocity2d_system();
    application.add_linear_velocity2d_system();
    application.add_fixed_system(observe_integrated_position);
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("ordered-velocity", move |world| {
        world.spawn(camera)?;
        world.spawn((OrderBody, LinearVelocity2d::default()))?;
        world.insert_resource(OrderObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let report = advance(&mut runner, STEP)?;

    assert!(report.failure().is_none());
    assert_eq!(
        runner.resource::<OrderObservation>().map(|value| value.0),
        Some(Vec2::new(2.0, 0.0))
    );
    Ok(())
}

fn queue_motion_once(mut state: ResMut<QueueState>, mut commands: Commands) -> LogicResult {
    if state.issued {
        return Ok(());
    }
    let velocity = LinearVelocity2d::new(Vec2::new(10.0, 0.0))?;
    commands.insert(state.target, velocity)?;
    commands.spawn((SpawnedBody, velocity))?;
    state.issued = true;
    Ok(())
}

#[test]
fn deferred_velocity_insert_and_spawn_first_integrate_on_the_next_tick()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_components::<(InsertedBody, SpawnedBody)>()?;
    application.add_fallible_fixed_system(queue_motion_once);
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("deferred-velocity", move |world| {
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
    let inserted = runner
        .components::<InsertedBody>()
        .next()
        .ok_or("inserted body disappeared")?
        .0;
    let spawned = runner
        .components::<SpawnedBody>()
        .next()
        .ok_or("spawned body did not commit")?
        .0;
    assert_eq!(transform(&runner, inserted)?.translation(), Vec2::ZERO);
    assert_eq!(transform(&runner, spawned)?.translation(), Vec2::ZERO);

    let second = advance(&mut runner, STEP)?;
    assert!(second.failure().is_none());
    assert_eq!(second.spawned(), 0);
    assert_eq!(
        transform(&runner, inserted)?.translation(),
        Vec2::new(1.0, 0.0)
    );
    assert_eq!(
        transform(&runner, spawned)?.translation(),
        Vec2::new(1.0, 0.0)
    );
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
fn replacement_world_uses_a_fresh_registered_velocity_system() -> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<ReplacementBody>()?;
    application.add_fallible_fixed_system(request_replacement);
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let velocity = LinearVelocity2d::new(Vec2::new(10.0, 0.0))?;
    let target = application.register_world("velocity-target", move |world| {
        world.spawn(camera)?;
        world.spawn((ReplacementBody, velocity))?;
        Ok(())
    })?;
    let source = application.register_world("velocity-source", move |world| {
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
    let body = runner
        .components::<ReplacementBody>()
        .next()
        .ok_or("replacement body missing")?
        .0;
    assert_eq!(transform(&runner, body)?.translation(), Vec2::ZERO);

    let next = advance(&mut runner, STEP)?;
    assert!(next.failure().is_none());
    assert_eq!(transform(&runner, body)?.translation(), Vec2::new(1.0, 0.0));
    Ok(())
}

#[test]
fn integration_overflow_is_an_ordinary_fixed_system_failure() -> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<OverflowBody>()?;
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let transform_at_limit = Transform2d::new(Vec2::splat(f32::MAX))?;
    let velocity_at_limit = LinearVelocity2d::new(Vec2::splat(f32::MAX))?;
    let world = application.register_world("overflow-velocity", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((OverflowBody, transform_at_limit, velocity_at_limit))?;
        world.insert_resource(Handles {
            cardinal: body,
            diagonal: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .cardinal;

    let report = advance(&mut runner, STEP)?;

    let failure = report
        .failure()
        .ok_or("overflow should fail the fixed System")?;
    assert!(failure.to_string().contains("translation must be finite"));
    let retained = transform(&runner, body)?;
    assert_eq!(retained.translation(), Vec2::splat(f32::MAX));
    assert_eq!(retained.previous_translation(), Vec2::splat(f32::MAX));
    Ok(())
}

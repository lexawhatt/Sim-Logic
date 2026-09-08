use std::{error::Error, time::Duration};

use sim_logic::bevy_ecs::entity_disabling::Disabled;
use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Left,
    Right,
    Down,
    Up,
}

const MOVEMENT: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
    TestAction::Left,
    TestAction::Right,
    TestAction::Down,
    TestAction::Up,
);
const ROTATED_MOVEMENT: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
    TestAction::Up,
    TestAction::Down,
    TestAction::Right,
    TestAction::Left,
);
const STEP: Duration = Duration::from_millis(100);

#[derive(Component)]
struct MovingBody;

#[derive(Component)]
struct RotatedBody;

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
    primary: LogicEntity,
    rotated: LogicEntity,
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

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, events, viewport))
    else {
        return Err("bounded digital-movement frame was rejected".into());
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
    let mut application = Application::new(config)?;
    application.bind_wasd(MOVEMENT)?;
    Ok(application)
}

fn press(key: PhysicalKeyCode) -> InputEvent {
    InputEvent::key(key, ButtonState::Pressed)
}

fn release(key: PhysicalKeyCode) -> InputEvent {
    InputEvent::key(key, ButtonState::Released)
}

#[test]
fn movement_is_inert_until_explicit_registration_and_supplies_a_transform()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<MovingBody>()?;
    let movement = DigitalMovement2d::new(MOVEMENT, 10.0)?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("inert-digital-movement", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((MovingBody, movement))?;
        world.insert_resource(Handles {
            primary: body,
            rotated: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .primary;

    let report = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;

    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(transform(&runner, body)?, Transform2d::default());
    Ok(())
}

#[test]
fn registered_movement_does_not_create_hidden_physical_bindings() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<MovingBody>()?;
    application.add_digital_movement2d_system();
    let movement = DigitalMovement2d::new(MOVEMENT, 10.0)?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("unbound-digital-movement", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((MovingBody, movement))?;
        world.insert_resource(Handles {
            primary: body,
            rotated: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .primary;

    let report = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;

    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 1);
    assert_eq!(transform(&runner, body)?, Transform2d::default());
    Ok(())
}

#[test]
fn registered_movement_samples_each_axis_per_tick_and_excludes_disabled()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(4)?;
    application.approve_components::<(MovingBody, RotatedBody, DisabledBody)>()?;
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let primary_movement = DigitalMovement2d::new(MOVEMENT, 10.0)?;
    let rotated_movement = DigitalMovement2d::new(ROTATED_MOVEMENT, 20.0)?;
    let disabled_transform = Transform2d::from_xy(5.0, 6.0)?;
    let world = application.register_world("integrated-digital-movement", move |world| {
        world.spawn(camera)?;
        let primary = world.spawn((MovingBody, primary_movement))?;
        let rotated = world.spawn((RotatedBody, rotated_movement))?;
        let disabled =
            world.spawn((DisabledBody, Disabled, disabled_transform, primary_movement))?;
        world.insert_resource(Handles {
            primary,
            rotated,
            disabled,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let handles = {
        let handles = runner.resource::<Handles>().ok_or("missing handles")?;
        Handles {
            primary: handles.primary,
            rotated: handles.rotated,
            disabled: handles.disabled,
        }
    };

    let zero = advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(zero.fixed_ticks_attempted(), 0);

    let idle = advance(&mut runner, STEP, &[])?;
    assert!(idle.failure().is_none());
    assert_eq!(idle.fixed_ticks_attempted(), 1);
    assert_eq!(transform(&runner, handles.primary)?, Transform2d::default());
    assert_eq!(transform(&runner, handles.rotated)?, Transform2d::default());

    let catch_up = advance(
        &mut runner,
        STEP.saturating_mul(2),
        &[press(PhysicalKeyCode::KeyD), press(PhysicalKeyCode::KeyW)],
    )?;
    assert!(catch_up.failure().is_none());
    assert_eq!(catch_up.fixed_ticks_attempted(), 2);

    let primary_step = Vec2::ONE.normalized() * 1.0;
    let primary = transform(&runner, handles.primary)?;
    assert_eq!(primary.previous_translation(), primary_step);
    assert_eq!(primary.translation(), primary_step * 2.0);

    let rotated_step = Vec2::new(-1.0, -1.0).normalized() * 2.0;
    let rotated = transform(&runner, handles.rotated)?;
    assert_eq!(rotated.previous_translation(), rotated_step);
    assert_eq!(rotated.translation(), rotated_step * 2.0);

    let disabled = transform(&runner, handles.disabled)?;
    assert_eq!(disabled.previous_translation(), Vec2::new(5.0, 6.0));
    assert_eq!(disabled.translation(), Vec2::new(5.0, 6.0));

    let cancelled = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyA)])?;
    assert!(cancelled.failure().is_none());
    let primary = transform(&runner, handles.primary)?;
    assert_eq!(primary.previous_translation(), primary_step * 2.0);
    assert_eq!(primary.translation(), primary_step * 2.0 + Vec2::Y);
    Ok(())
}

fn choose_speed(mut movement: Single<&mut DigitalMovement2d<TestAction>, With<OrderBody>>) {
    movement
        .set_speed(20.0)
        .expect("test movement speed should be valid");
}

fn observe_integrated_position(
    transform: Single<&Transform2d, With<OrderBody>>,
    mut observation: ResMut<OrderObservation>,
) {
    observation.0 = transform.translation();
}

#[test]
fn movement_uses_preceding_writes_and_duplicate_registration_and_composes_with_linear_velocity()
-> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<OrderBody>()?;
    application.add_fixed_system(choose_speed);
    application.add_digital_movement2d_system();
    application.add_linear_velocity2d_system();
    application.add_digital_movement2d_system();
    application.add_fixed_system(observe_integrated_position);
    let camera = ActiveCamera2d::centered(20.0)?;
    let movement = DigitalMovement2d::new(MOVEMENT, 0.0)?;
    let velocity = LinearVelocity2d::new(Vec2::new(0.0, 10.0))?;
    let world = application.register_world("ordered-digital-movement", move |world| {
        world.spawn(camera)?;
        world.spawn((OrderBody, movement, velocity))?;
        world.insert_resource(OrderObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let report = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;

    assert!(report.failure().is_none());
    assert_eq!(
        runner.resource::<OrderObservation>().map(|value| value.0),
        Some(Vec2::new(4.0, 1.0))
    );
    Ok(())
}

fn composition_runner(
    digital_first: bool,
) -> Result<(HeadlessRunner<TestAction>, LogicEntity), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_secs(1), 1)?);
    let mut application = Application::new(config)?;
    application.approve_component::<OrderBody>()?;
    application.bind_wasd(MOVEMENT)?;
    if digital_first {
        application.add_digital_movement2d_system();
        application.add_linear_velocity2d_system();
    } else {
        application.add_linear_velocity2d_system();
        application.add_digital_movement2d_system();
    }
    let camera = ActiveCamera2d::centered(20.0)?;
    let transform = Transform2d::from_xy(-1.0e20, 0.0)?;
    let movement = DigitalMovement2d::new(MOVEMENT, 1.0e20)?;
    let velocity = LinearVelocity2d::new(Vec2::new(3.0, 0.0))?;
    let world = application.register_world("motion-composition-order", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((OrderBody, transform, movement, velocity))?;
        world.insert_resource(Handles {
            primary: body,
            rotated: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .primary;
    Ok((runner, body))
}

#[test]
fn digital_and_linear_adapters_compose_in_explicit_nonassociative_order()
-> Result<(), Box<dyn Error>> {
    let (mut digital_first, digital_first_body) = composition_runner(true)?;
    let first = advance(
        &mut digital_first,
        Duration::from_secs(1),
        &[press(PhysicalKeyCode::KeyD)],
    )?;
    assert!(first.failure().is_none());
    assert_eq!(
        transform(&digital_first, digital_first_body)?.translation(),
        Vec2::new(3.0, 0.0)
    );

    let (mut linear_first, linear_first_body) = composition_runner(false)?;
    let second = advance(
        &mut linear_first,
        Duration::from_secs(1),
        &[press(PhysicalKeyCode::KeyD)],
    )?;
    assert!(second.failure().is_none());
    assert_eq!(
        transform(&linear_first, linear_first_body)?.translation(),
        Vec2::ZERO
    );
    Ok(())
}

fn queue_movement_once(mut state: ResMut<QueueState>, mut commands: Commands) -> LogicResult {
    if state.issued {
        return Ok(());
    }
    let movement = DigitalMovement2d::new(MOVEMENT, 10.0)?;
    commands.insert(state.target, movement)?;
    commands.spawn((SpawnedBody, movement))?;
    state.issued = true;
    Ok(())
}

#[test]
fn deferred_movement_insert_and_spawn_first_run_on_the_next_tick() -> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_components::<(InsertedBody, SpawnedBody)>()?;
    application.add_fallible_fixed_system(queue_movement_once);
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("deferred-digital-movement", move |world| {
        world.spawn(camera)?;
        let target = world.spawn(InsertedBody)?;
        world.insert_resource(QueueState {
            target,
            issued: false,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    let first = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;
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

    let second = advance(&mut runner, STEP, &[])?;
    assert!(second.failure().is_none());
    assert_eq!(transform(&runner, inserted)?.translation(), Vec2::X);
    assert_eq!(transform(&runner, spawned)?.translation(), Vec2::X);
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
fn replacement_world_uses_a_fresh_registered_movement_system() -> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<ReplacementBody>()?;
    application.add_fallible_fixed_system(request_replacement);
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let movement = DigitalMovement2d::new(MOVEMENT, 10.0)?;
    let target = application.register_world("digital-movement-target", move |world| {
        world.spawn(camera)?;
        world.spawn((ReplacementBody, movement))?;
        Ok(())
    })?;
    let source = application.register_world("digital-movement-source", move |world| {
        world.spawn(camera)?;
        world.insert_resource(ReplacementRoute(target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let transition = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;
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

    let next = advance(&mut runner, STEP, &[])?;
    assert!(next.failure().is_none());
    assert_eq!(transform(&runner, body)?.translation(), Vec2::X);
    Ok(())
}

#[test]
fn movement_overflow_is_an_ordinary_fixed_system_failure() -> Result<(), Box<dyn Error>> {
    let mut application = test_application(1)?;
    application.approve_component::<OverflowBody>()?;
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let transform_at_limit = Transform2d::new(Vec2::splat(f32::MAX))?;
    let movement_at_limit = DigitalMovement2d::new(MOVEMENT, f32::MAX)?;
    let world = application.register_world("overflow-digital-movement", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((OverflowBody, transform_at_limit, movement_at_limit))?;
        world.insert_resource(Handles {
            primary: body,
            rotated: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .primary;

    let report = advance(&mut runner, STEP, &[press(PhysicalKeyCode::KeyD)])?;

    let failure = report
        .failure()
        .ok_or("overflow should fail the fixed System")?;
    assert!(failure.to_string().contains("translation must be finite"));
    let retained = transform(&runner, body)?;
    assert_eq!(retained.translation(), Vec2::splat(f32::MAX));
    assert_eq!(retained.previous_translation(), Vec2::splat(f32::MAX));
    Ok(())
}

#[test]
fn zero_direction_is_a_no_op_before_speed_time_overflow() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    let long_step = Duration::from_secs(2);
    config.set_time(TimeConfig::new(long_step, 1)?);
    let mut application = Application::new(config)?;
    application.approve_component::<OverflowBody>()?;
    application.bind_wasd(MOVEMENT)?;
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(20.0)?;
    let movement = DigitalMovement2d::new(MOVEMENT, f32::MAX)?;
    let world = application.register_world("zero-direction-overflow", move |world| {
        world.spawn(camera)?;
        let body = world.spawn((OverflowBody, movement))?;
        world.insert_resource(Handles {
            primary: body,
            rotated: body,
            disabled: body,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let body = runner
        .resource::<Handles>()
        .ok_or("missing handles")?
        .primary;

    let idle = advance(&mut runner, long_step, &[])?;
    assert!(idle.failure().is_none());
    assert_eq!(transform(&runner, body)?, Transform2d::default());

    let opposed = advance(
        &mut runner,
        long_step,
        &[press(PhysicalKeyCode::KeyA), press(PhysicalKeyCode::KeyD)],
    )?;
    assert!(opposed.failure().is_none());
    assert_eq!(transform(&runner, body)?, Transform2d::default());

    let active = advance(&mut runner, long_step, &[release(PhysicalKeyCode::KeyA)])?;
    assert!(active.failure().is_some());
    assert_eq!(transform(&runner, body)?, Transform2d::default());
    Ok(())
}

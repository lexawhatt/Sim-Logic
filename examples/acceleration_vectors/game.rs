use sim_logic::prelude::*;

const ACCELERATION_MAGNITUDE: f32 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyAction {
    Up,
    Down,
    Left,
    Right,
}

const ACCELERATION_AXIS: DigitalAxis2d<BodyAction> = DigitalAxis2d::new(
    BodyAction::Left,
    BodyAction::Right,
    BodyAction::Down,
    BodyAction::Up,
);

#[derive(Component)]
pub struct Body;

fn sample_acceleration(
    input: FixedInput<BodyAction>,
    mut acceleration: Single<&mut LinearAcceleration2d, With<Body>>,
) -> LogicResult {
    acceleration.set_acceleration(
        input.normalized_digital_axis(ACCELERATION_AXIS) * ACCELERATION_MAGNITUDE,
    )?;
    Ok(())
}

fn sync_acceleration_visual(
    body: Single<(&LinearAcceleration2d, &mut LineVisual), With<Body>>,
) -> LogicResult {
    let (acceleration, mut visual) = body.into_inner();
    visual.set_vector(acceleration.acceleration())?;
    Ok(())
}

#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "the shared headless test uses the custom-time builder"
    )
)]
pub fn build_application() -> LogicResult<(Application<BodyAction>, WorldFactoryId)> {
    build_application_with_time(TimeConfig::default())
}

pub fn build_application_with_time(
    time: TimeConfig,
) -> LogicResult<(Application<BodyAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;
    application.approve_component::<Body>()?;
    application.bind_wasd_and_arrows(ACCELERATION_AXIS)?;

    application.add_fallible_fixed_system(sample_acceleration);
    application.add_linear_acceleration2d_system();
    application.add_linear_velocity2d_system();
    application.add_fallible_fixed_system(sync_acceleration_visual);

    let camera = ActiveCamera2d::centered(30.0)?;
    let background = WorldBackground::new(Color::rgb8(12, 18, 30))?;
    let body = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?;
    let acceleration_line = LineVisual::new(Vec2::ZERO, 3.0, Color::rgb8(255, 176, 70))?;
    let initial_world = application.register_world("acceleration-vectors", move |world| {
        world.insert_resource(background)?;
        world.spawn(camera)?;
        world.spawn((
            Body,
            LinearAcceleration2d::default(),
            body,
            acceleration_line,
        ))?;
        Ok(())
    })?;
    Ok((application, initial_world))
}

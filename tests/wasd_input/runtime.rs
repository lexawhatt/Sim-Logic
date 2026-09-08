use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

const MOVEMENT: DigitalAxis2d<Direction> = DigitalAxis2d::new(
    Direction::Left,
    Direction::Right,
    Direction::Down,
    Direction::Up,
);

#[derive(Resource, Default)]
struct AxisSample {
    value: Vec2,
    runs: usize,
}

fn sample_axis(input: FixedInput<Direction>, mut sample: ResMut<AxisSample>) {
    sample.value = input.normalized_digital_axis(MOVEMENT);
    sample.runs += 1;
}

fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

fn advance(
    runner: &mut HeadlessRunner<Direction>,
    events: &[InputEvent],
) -> Result<Vec2, Box<dyn Error>> {
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        events,
        viewport()?,
    ));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded WASD frame should be accepted".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("WASD frame failed: {failure}").into());
    }
    if report.fixed_ticks_attempted() != 1 {
        return Err("WASD frame should run exactly one fixed tick".into());
    }
    Ok(runner
        .resource::<AxisSample>()
        .ok_or("axis sample resource should remain available")?
        .value)
}

#[test]
fn public_wasd_binding_samples_normalized_cardinals_diagonal_and_opposite_cancellation()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 1)?);
    let mut application = Application::<Direction>::new(config)?;
    application.bind_wasd(MOVEMENT)?;
    application.add_system(Stage::FixedUpdate, sample_axis);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let world = application.register_world("wasd-input", move |world| {
        world.spawn(camera)?;
        world.insert_resource(AxisSample::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;

    assert_eq!(
        advance(
            &mut runner,
            &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
        )?,
        Vec2::Y
    );
    assert_eq!(
        advance(
            &mut runner,
            &[
                InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed),
            ],
        )?,
        Vec2::new(-1.0, 0.0)
    );
    assert_eq!(
        advance(
            &mut runner,
            &[
                InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::KeyS, ButtonState::Pressed),
            ],
        )?,
        Vec2::new(0.0, -1.0)
    );
    assert_eq!(
        advance(
            &mut runner,
            &[
                InputEvent::key(PhysicalKeyCode::KeyS, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            ],
        )?,
        Vec2::X
    );
    assert_eq!(
        advance(
            &mut runner,
            &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
        )?,
        Vec2::ONE.normalized()
    );
    assert_eq!(
        advance(
            &mut runner,
            &[InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed,)],
        )?,
        Vec2::Y,
        "held A and D cancel while held W remains positive y"
    );
    assert_eq!(
        runner
            .resource::<AxisSample>()
            .ok_or("axis sample resource should remain available")?
            .runs,
        6
    );
    Ok(())
}

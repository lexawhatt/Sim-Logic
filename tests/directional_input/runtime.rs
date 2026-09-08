use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Left,
    Right,
    Down,
    Up,
    Escape,
}

const MOVEMENT: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
    TestAction::Left,
    TestAction::Right,
    TestAction::Down,
    TestAction::Up,
);

const CANCELLED_MOVEMENT: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
    TestAction::Left,
    TestAction::Left,
    TestAction::Down,
    TestAction::Down,
);

const REPEATED_DIAGONAL: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
    TestAction::Left,
    TestAction::Right,
    TestAction::Down,
    TestAction::Right,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgePresenceSample {
    held: bool,
    has_press: bool,
    has_release: bool,
    presses: usize,
    releases: usize,
}

#[derive(Resource, Default)]
struct InputProbe {
    frame_axes: Vec<Vec2>,
    frame_directions: Vec<Vec2>,
    frame_escape_tokens: Vec<TransitionIntentToken>,
    frame_up_edges: Vec<EdgePresenceSample>,
    frame_up_edges_again: Vec<EdgePresenceSample>,
    fixed_axes: Vec<Vec2>,
    fixed_directions: Vec<Vec2>,
    fixed_cancelled_directions: Vec<Vec2>,
    fixed_repeated_directions: Vec<Vec2>,
    fixed_escape_tokens: Vec<TransitionIntentToken>,
    fixed_up_edges: Vec<EdgePresenceSample>,
    fixed_up_edges_again: Vec<EdgePresenceSample>,
}

fn fixed_up_edges(input: &FixedInput<TestAction>) -> EdgePresenceSample {
    let pressed_before: Vec<_> = input.pressed(TestAction::Up).collect();
    let released_before: Vec<_> = input.released(TestAction::Up).collect();
    let has_press = input.has_press_occurrence(TestAction::Up);
    let has_release = input.has_release_occurrence(TestAction::Up);
    assert_eq!(input.has_press_occurrence(TestAction::Up), has_press);
    assert_eq!(input.has_release_occurrence(TestAction::Up), has_release);
    assert_eq!(
        input.pressed(TestAction::Up).collect::<Vec<_>>(),
        pressed_before
    );
    assert_eq!(
        input.released(TestAction::Up).collect::<Vec<_>>(),
        released_before
    );
    EdgePresenceSample {
        held: input.held(TestAction::Up),
        has_press,
        has_release,
        presses: pressed_before.len(),
        releases: released_before.len(),
    }
}

fn frame_up_edges(input: &FrameInput<TestAction>) -> EdgePresenceSample {
    let pressed_before: Vec<_> = input.pressed(TestAction::Up).collect();
    let released_before: Vec<_> = input.released(TestAction::Up).collect();
    let has_press = input.has_press_occurrence(TestAction::Up);
    let has_release = input.has_release_occurrence(TestAction::Up);
    assert_eq!(input.has_press_occurrence(TestAction::Up), has_press);
    assert_eq!(input.has_release_occurrence(TestAction::Up), has_release);
    assert_eq!(
        input.pressed(TestAction::Up).collect::<Vec<_>>(),
        pressed_before
    );
    assert_eq!(
        input.released(TestAction::Up).collect::<Vec<_>>(),
        released_before
    );
    EdgePresenceSample {
        held: input.held(TestAction::Up),
        has_press,
        has_release,
        presses: pressed_before.len(),
        releases: released_before.len(),
    }
}

fn observe_fixed(input: FixedInput<TestAction>, mut probe: ResMut<InputProbe>) {
    probe.fixed_axes.push(input.digital_axis(MOVEMENT));
    let direction = input.normalized_digital_axis(MOVEMENT);
    assert_eq!(input.normalized_digital_axis(MOVEMENT), direction);
    probe.fixed_directions.push(direction);
    probe
        .fixed_cancelled_directions
        .push(input.normalized_digital_axis(CANCELLED_MOVEMENT));
    probe
        .fixed_repeated_directions
        .push(input.normalized_digital_axis(REPEATED_DIAGONAL));
    probe
        .fixed_escape_tokens
        .extend(input.pressed(TestAction::Escape).map(ActionEdge::intent));
    probe.fixed_up_edges.push(fixed_up_edges(&input));
}

fn observe_frame(input: FrameInput<TestAction>, mut probe: ResMut<InputProbe>) {
    probe.frame_axes.push(input.digital_axis(MOVEMENT));
    let direction = input.normalized_digital_axis(MOVEMENT);
    assert_eq!(input.normalized_digital_axis(MOVEMENT), direction);
    probe.frame_directions.push(direction);
    probe
        .frame_escape_tokens
        .extend(input.pressed(TestAction::Escape).map(ActionEdge::intent));
    probe.frame_up_edges.push(frame_up_edges(&input));
}

fn observe_fixed_again(input: FixedInput<TestAction>, mut probe: ResMut<InputProbe>) {
    probe.fixed_up_edges_again.push(fixed_up_edges(&input));
}

fn observe_frame_again(input: FrameInput<TestAction>, mut probe: ResMut<InputProbe>) {
    probe.frame_up_edges_again.push(frame_up_edges(&input));
}

fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, events, viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded directional-input frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("directional-input frame failed: {failure}").into());
    }
    Ok(report)
}

fn build_probe_runner() -> Result<HeadlessRunner<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
    let mut application = Application::new(config)?;
    application.bind_wasd_and_arrows(MOVEMENT)?;
    application.bind_key(PhysicalKeyCode::Escape, TestAction::Escape)?;
    application.add_fixed_system(observe_fixed);
    application.add_fixed_system(observe_fixed_again);
    application.add_frame_system(observe_frame);
    application.add_frame_system(observe_frame_again);
    let camera = ActiveCamera2d::centered(20.0)?;
    let world = application.register_world("directional-input", move |world| {
        world.spawn(camera)?;
        world.insert_resource(InputProbe::default())?;
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn last_fixed_axis(runner: &HeadlessRunner<TestAction>) -> Result<Vec2, Box<dyn Error>> {
    runner
        .resource::<InputProbe>()
        .and_then(|probe| probe.fixed_axes.last().copied())
        .ok_or_else(|| "fixed input was not sampled".into())
}

fn last_fixed_direction(runner: &HeadlessRunner<TestAction>) -> Result<Vec2, Box<dyn Error>> {
    runner
        .resource::<InputProbe>()
        .and_then(|probe| probe.fixed_directions.last().copied())
        .ok_or_else(|| "fixed normalized input was not sampled".into())
}

#[test]
fn arrows_escape_and_wasd_share_the_public_input_pipeline() -> Result<(), Box<dyn Error>> {
    let mut runner = build_probe_runner()?;
    let first = [
        InputEvent::key(PhysicalKeyCode::ArrowLeft, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Pressed),
    ];
    let zero_tick = advance(&mut runner, Duration::ZERO, &first)?;
    assert_eq!(zero_tick.fixed_ticks_attempted(), 0);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    assert_eq!(probe.frame_axes, [Vec2::new(-1.0, 0.0)]);
    assert_eq!(probe.frame_directions, [Vec2::new(-1.0, 0.0)]);
    let [frame_escape] = probe.frame_escape_tokens.as_slice() else {
        return Err("FrameUpdate should receive one Escape edge".into());
    };
    let frame_escape = *frame_escape;

    let catch_up = advance(&mut runner, FIXED_STEP.saturating_mul(3), &[])?;
    assert_eq!(catch_up.fixed_ticks_attempted(), 3);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    assert_eq!(probe.fixed_axes, [Vec2::new(-1.0, 0.0); 3]);
    assert_eq!(probe.fixed_directions, [Vec2::new(-1.0, 0.0); 3]);
    assert_eq!(probe.fixed_escape_tokens, [frame_escape]);

    let samples = [
        (
            [
                InputEvent::key(PhysicalKeyCode::ArrowLeft, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
            ],
            Vec2::X,
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::ArrowDown, ButtonState::Pressed),
            ],
            Vec2::new(0.0, -1.0),
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::ArrowDown, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::ArrowUp, ButtonState::Pressed),
            ],
            Vec2::Y,
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
            ],
            Vec2::ONE,
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::ArrowLeft, ButtonState::Pressed),
                InputEvent::key(PhysicalKeyCode::ArrowLeft, ButtonState::Pressed),
            ],
            Vec2::Y,
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
                InputEvent::key(PhysicalKeyCode::ArrowUp, ButtonState::Released),
            ],
            Vec2::Y,
        ),
        (
            [
                InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Released),
            ],
            Vec2::ZERO,
        ),
    ];

    for (events, expected) in samples {
        let report = advance(&mut runner, FIXED_STEP, &events)?;
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert_eq!(last_fixed_axis(&runner)?, expected);
        assert_eq!(last_fixed_direction(&runner)?, expected.normalized());
    }

    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    assert!(
        probe
            .fixed_cancelled_directions
            .iter()
            .all(|direction| *direction == Vec2::ZERO)
    );
    assert_eq!(
        probe.fixed_repeated_directions[6],
        Vec2::ONE.normalized(),
        "one held action used in both positive slots forms a unit diagonal"
    );
    Ok(())
}

#[test]
fn public_frame_and_fixed_normalized_axes_cover_all_held_combinations() -> Result<(), Box<dyn Error>>
{
    let mut runner = build_probe_runner()?;
    let keys = [
        PhysicalKeyCode::KeyA,
        PhysicalKeyCode::KeyD,
        PhysicalKeyCode::KeyS,
        PhysicalKeyCode::KeyW,
    ];
    let mut previous_mask = 0_u8;

    for mask in 0_u8..16 {
        let mut events = Vec::new();
        for (index, key) in keys.into_iter().enumerate() {
            let bit = 1_u8 << index;
            let was_held = previous_mask & bit != 0;
            let is_held = mask & bit != 0;
            if was_held != is_held {
                events.push(InputEvent::key(
                    key,
                    if is_held {
                        ButtonState::Pressed
                    } else {
                        ButtonState::Released
                    },
                ));
            }
        }

        let report = advance(&mut runner, FIXED_STEP, &events)?;
        assert_eq!(report.fixed_ticks_attempted(), 1);
        let expected_raw = Vec2::new(
            f32::from(mask & 0b0010 != 0) - f32::from(mask & 0b0001 != 0),
            f32::from(mask & 0b1000 != 0) - f32::from(mask & 0b0100 != 0),
        );
        let expected_direction = expected_raw.normalized();
        let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
        assert_eq!(
            probe.fixed_axes.last(),
            Some(&expected_raw),
            "mask {mask:04b}"
        );
        assert_eq!(
            probe.fixed_directions.last(),
            Some(&expected_direction),
            "fixed mask {mask:04b}"
        );
        assert_eq!(
            probe.frame_axes.last(),
            Some(&expected_raw),
            "mask {mask:04b}"
        );
        assert_eq!(
            probe.frame_directions.last(),
            Some(&expected_direction),
            "frame mask {mask:04b}"
        );
        previous_mask = mask;
    }
    Ok(())
}

#[test]
fn boolean_edges_are_repeatable_physical_occurrence_checks_with_fixed_retention()
-> Result<(), Box<dyn Error>> {
    let mut runner = build_probe_runner()?;
    let first = [
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::ArrowUp, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
    ];
    let zero_tick = advance(&mut runner, Duration::ZERO, &first)?;
    assert_eq!(zero_tick.fixed_ticks_attempted(), 0);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    let aliased = EdgePresenceSample {
        held: true,
        has_press: true,
        has_release: true,
        presses: 2,
        releases: 1,
    };
    assert_eq!(probe.frame_up_edges, [aliased]);
    assert_eq!(probe.frame_up_edges_again, [aliased]);
    assert!(probe.fixed_up_edges.is_empty());
    assert!(probe.fixed_up_edges_again.is_empty());

    let catch_up = advance(&mut runner, FIXED_STEP.saturating_mul(2), &[])?;
    assert_eq!(catch_up.fixed_ticks_attempted(), 2);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    let held_without_edges = EdgePresenceSample {
        held: true,
        has_press: false,
        has_release: false,
        presses: 0,
        releases: 0,
    };
    assert_eq!(probe.fixed_up_edges, [aliased, held_without_edges]);
    assert_eq!(probe.fixed_up_edges_again, probe.fixed_up_edges);
    assert_eq!(probe.frame_up_edges.last(), Some(&held_without_edges));
    assert_eq!(probe.frame_up_edges_again, probe.frame_up_edges);

    let released = [InputEvent::key(
        PhysicalKeyCode::ArrowUp,
        ButtonState::Released,
    )];
    let release = advance(&mut runner, FIXED_STEP, &released)?;
    assert_eq!(release.fixed_ticks_attempted(), 1);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    let final_release = EdgePresenceSample {
        held: false,
        has_press: false,
        has_release: true,
        presses: 0,
        releases: 1,
    };
    assert_eq!(probe.fixed_up_edges.last(), Some(&final_release));
    assert_eq!(probe.fixed_up_edges_again.last(), Some(&final_release));
    assert_eq!(probe.frame_up_edges.last(), Some(&final_release));
    assert_eq!(probe.frame_up_edges_again.last(), Some(&final_release));

    let tap = [
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
    ];
    let tap_frame = advance(&mut runner, FIXED_STEP, &tap)?;
    assert_eq!(tap_frame.fixed_ticks_attempted(), 1);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    let pressed_and_released = EdgePresenceSample {
        held: false,
        has_press: true,
        has_release: true,
        presses: 1,
        releases: 1,
    };
    assert_eq!(probe.fixed_up_edges.last(), Some(&pressed_and_released));
    assert_eq!(
        probe.fixed_up_edges_again.last(),
        Some(&pressed_and_released)
    );
    assert_eq!(probe.frame_up_edges.last(), Some(&pressed_and_released));
    assert_eq!(
        probe.frame_up_edges_again.last(),
        Some(&pressed_and_released)
    );

    let pressed = [InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed)];
    let press = advance(&mut runner, FIXED_STEP, &pressed)?;
    assert_eq!(press.fixed_ticks_attempted(), 1);
    let probe = runner.resource::<InputProbe>().ok_or("probe disappeared")?;
    let press_only = EdgePresenceSample {
        held: true,
        has_press: true,
        has_release: false,
        presses: 1,
        releases: 0,
    };
    assert_eq!(probe.fixed_up_edges.last(), Some(&press_only));
    assert_eq!(probe.fixed_up_edges_again.last(), Some(&press_only));
    assert_eq!(probe.frame_up_edges.last(), Some(&press_only));
    assert_eq!(probe.frame_up_edges_again.last(), Some(&press_only));
    Ok(())
}

#[derive(Resource)]
struct ReplacementRoute(WorldFactoryId);

#[derive(Resource, Default)]
struct ReplacementProbe {
    runs: usize,
    axis: Vec2,
    escape_held: bool,
    escape_presses: usize,
}

fn observe_or_replace(
    input: FixedInput<TestAction>,
    route: Option<Res<ReplacementRoute>>,
    mut probe: Option<ResMut<ReplacementProbe>>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    if let Some(probe) = probe.as_mut() {
        probe.runs += 1;
        probe.axis = input.digital_axis(MOVEMENT);
        probe.escape_held = input.held(TestAction::Escape);
        probe.escape_presses = input.pressed(TestAction::Escape).count();
    }
    let Some(route) = route else {
        return Ok(());
    };
    for edge in input.pressed(TestAction::Escape) {
        commands.replace_world(edge.intent(), route.0)?;
    }
    Ok(())
}

#[test]
fn held_arrow_survives_replacement_without_replaying_the_escape_edge() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::new(config)?;
    application.bind_arrows(MOVEMENT)?;
    application.bind_key(PhysicalKeyCode::Escape, TestAction::Escape)?;
    application.add_fallible_fixed_system(observe_or_replace);

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("target", move |world| {
        world.spawn(target_camera)?;
        world.insert_resource(ReplacementProbe::default())?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(ReplacementRoute(target))?;
        world.insert_resource(ReplacementProbe::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let transition = advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        ],
    )?;
    assert!(matches!(
        transition.transition(),
        FrameTransition::Committed { old, new, .. } if old != new
    ));
    assert_eq!(runner.active_world_name(), "target");
    let fresh = runner
        .resource::<ReplacementProbe>()
        .ok_or("target probe disappeared")?;
    assert_eq!(fresh.runs, 0);

    let next = advance(&mut runner, FIXED_STEP, &[])?;
    assert_eq!(next.fixed_ticks_attempted(), 1);
    let probe = runner
        .resource::<ReplacementProbe>()
        .ok_or("target probe disappeared")?;
    assert_eq!(probe.runs, 1);
    assert_eq!(probe.axis, Vec2::X);
    assert!(probe.escape_held);
    assert_eq!(probe.escape_presses, 0);
    Ok(())
}

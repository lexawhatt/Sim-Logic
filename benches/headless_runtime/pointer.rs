use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PointerAction(u8);

#[derive(Resource, Default)]
struct PointerChecksum(u64);

const KEYS: [PhysicalKeyCode; 11] = [
    PhysicalKeyCode::KeyW,
    PhysicalKeyCode::KeyA,
    PhysicalKeyCode::KeyS,
    PhysicalKeyCode::KeyD,
    PhysicalKeyCode::Enter,
    PhysicalKeyCode::Space,
    PhysicalKeyCode::ArrowLeft,
    PhysicalKeyCode::ArrowRight,
    PhysicalKeyCode::ArrowDown,
    PhysicalKeyCode::ArrowUp,
    PhysicalKeyCode::Escape,
];
const BUTTONS: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

fn observe_fixed(input: FixedInput<PointerAction>, mut checksum: ResMut<PointerChecksum>) {
    checksum.0 += u64::from(input.pointer().is_some());
    for index in 0..14 {
        let action = PointerAction(index);
        checksum.0 += u64::from(input.held(action));
        for edge in input.pressed(action).chain(input.released(action)) {
            checksum.0 += 1 + u64::from(edge.pointer().is_some());
        }
    }
}

fn observe_frame(input: FrameInput<PointerAction>, mut checksum: ResMut<PointerChecksum>) {
    checksum.0 += u64::from(input.pointer().is_some());
    for index in 0..14 {
        let action = PointerAction(index);
        checksum.0 += u64::from(input.held(action));
        for edge in input.pressed(action).chain(input.released(action)) {
            checksum.0 += 1 + u64::from(edge.pointer().is_some());
        }
    }
}

fn advance(
    runner: &mut HeadlessRunner<PointerAction>,
    viewport: LogicalViewport,
    events: &[InputEvent],
    ticks: u32,
) -> Result<(), Box<dyn Error>> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(FIXED_STEP * ticks, events, viewport))
    else {
        return Err(io::Error::other("pointer allocation frame was rejected").into());
    };
    if report.failure().is_some()
        || report.fixed_ticks_attempted() != ticks
        || report.exit_requested()
        || !matches!(report.transition(), FrameTransition::None)
        || report.extracted_generation() != Some(runner.world_generation())
    {
        return Err(
            io::Error::other("pointer allocation frame violated its delivery contract").into(),
        );
    }
    black_box(report);
    Ok(())
}

fn checksum(runner: &HeadlessRunner<PointerAction>) -> Result<u64, Box<dyn Error>> {
    Ok(runner
        .resource::<PointerChecksum>()
        .ok_or_else(|| io::Error::other("pointer checksum disappeared"))?
        .0)
}

pub(super) fn run_allocation_case() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<PointerAction>::new(AppConfig::default())?;
    for (index, key) in KEYS.into_iter().enumerate() {
        application.bind_key(key, PointerAction(index as u8))?;
    }
    for (index, button) in BUTTONS.into_iter().enumerate() {
        application.bind_mouse_button(button, PointerAction(11 + index as u8))?;
    }
    application.add_fixed_system(observe_fixed);
    application.add_frame_system(observe_frame);
    let camera = ActiveCamera2d::centered(40.0)?;
    let world = application.register_world("pointer-allocation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(PointerChecksum::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let first = PointerSample::new(LogicalScreenPosition::new(320.0, 180.0), viewport)?;
    let last = PointerSample::new(LogicalScreenPosition::new(960.0, 540.0), viewport)?;
    let mut press = [InputEvent::pointer_moved(first); 15];
    let mut release = [InputEvent::PointerLeft; 12];
    for (index, key) in KEYS.into_iter().enumerate() {
        press[index + 1] = InputEvent::key(key, ButtonState::Pressed);
        release[index + 1] = InputEvent::key(key, ButtonState::Released);
    }
    for (index, button) in BUTTONS.into_iter().enumerate() {
        press[index + 12] = InputEvent::mouse_button(button, ButtonState::Pressed);
    }
    let cycle = |runner: &mut HeadlessRunner<PointerAction>| -> Result<(), Box<dyn Error>> {
        advance(runner, viewport, &press, 1)?;
        advance(runner, viewport, &[InputEvent::pointer_moved(last)], 0)?;
        advance(runner, viewport, &release, 4)
    };
    for _ in 0..WARM_UP_FRAMES {
        cycle(&mut runner)?;
    }
    let before = checksum(&runner)?;
    begin_allocation_count();
    let measured = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            cycle(&mut runner)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured?;
    let delta = checksum(&runner)? - before;
    if delta != ALLOCATION_CHECK_FRAMES as u64 * 107 {
        return Err(
            io::Error::other(format!("pointer delivery checksum mismatch: {delta}")).into(),
        );
    }
    println!(
        "warmed_pointer_input cycles={ALLOCATION_CHECK_FRAMES} controls=14 ticks=1/0/4 allocation_calls={allocations} checksum_delta={delta}"
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed pointer frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

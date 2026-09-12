use std::time::Duration;

use sim_logic::prelude::*;

pub const STEP: Duration = Duration::from_millis(10);

pub const KEYS: [PhysicalKeyCode; 25] = [
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
    PhysicalKeyCode::KeyP,
    PhysicalKeyCode::KeyR,
    PhysicalKeyCode::KeyN,
    PhysicalKeyCode::Digit1,
    PhysicalKeyCode::Digit2,
    PhysicalKeyCode::Digit3,
    PhysicalKeyCode::Digit4,
    PhysicalKeyCode::Digit5,
    PhysicalKeyCode::F3,
    PhysicalKeyCode::F4,
    PhysicalKeyCode::F5,
    PhysicalKeyCode::F6,
    PhysicalKeyCode::F8,
    PhysicalKeyCode::F9,
];
pub const BUTTONS: [MouseButton; 3] = [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestAction {
    Shared,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub pointer: Option<PointerSample>,
    pub held: bool,
    pub presses: Vec<ActionEdge<TestAction>>,
    pub releases: Vec<ActionEdge<TestAction>>,
}

#[derive(Default)]
pub struct Probe {
    pub frames: Vec<Snapshot>,
    pub frame_edges: Vec<Vec<ActionEdge<TestAction>>>,
    pub fixed: Vec<Snapshot>,
}

fn observe_frame(input: FrameInput<TestAction>, mut probe: AppResMut<Probe>) {
    probe.frames.push(Snapshot {
        pointer: input.pointer(),
        held: input.held(TestAction::Shared),
        presses: input.pressed(TestAction::Shared).collect(),
        releases: input.released(TestAction::Shared).collect(),
    });
    probe.frame_edges.push(input.edges().collect());
}

fn observe_fixed(input: FixedInput<TestAction>, mut probe: AppResMut<Probe>) {
    probe.fixed.push(Snapshot {
        pointer: input.pointer(),
        held: input.held(TestAction::Shared),
        presses: input.pressed(TestAction::Shared).collect(),
        releases: input.released(TestAction::Shared).collect(),
    });
}

pub fn application(limit: usize, all_controls: bool) -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 4)?);
    config.set_input_event_limit(limit)?;
    let mut app = Application::new(config)?;
    app.register_app_resource(Probe::default())?;
    let keys: &[PhysicalKeyCode] = if all_controls {
        &KEYS
    } else {
        &[PhysicalKeyCode::Space, PhysicalKeyCode::KeyW]
    };
    for &key in keys {
        app.bind_key(key, TestAction::Shared)?;
    }
    for button in BUTTONS {
        app.bind_mouse_button(button, TestAction::Shared)?;
    }
    app.add_fixed_system(observe_fixed);
    app.add_frame_system(observe_frame);
    Ok(app)
}

pub fn register_world(
    app: &mut Application<TestAction>,
    name: &str,
) -> LogicResult<WorldFactoryId> {
    let camera = ActiveCamera2d::centered(1.0)?;
    Ok(app.register_world(name, move |world| {
        world.spawn(camera)?;
        Ok(())
    })?)
}

pub fn runner(limit: usize, all_controls: bool) -> LogicResult<HeadlessRunner<TestAction>> {
    let mut app = application(limit, all_controls)?;
    let initial = register_world(&mut app, "input-cancellation")?;
    Ok(app.build_headless(initial)?)
}

pub fn request(elapsed: Duration, events: &[InputEvent]) -> LogicResult<FrameRequest<'_>> {
    Ok(FrameRequest::new(
        elapsed,
        events,
        LogicalViewport::new(800.0, 600.0)?,
    ))
}

pub fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(request(elapsed, events)?) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

pub fn probe(runner: &HeadlessRunner<TestAction>) -> LogicResult<&Probe> {
    runner
        .app_resource::<Probe>()
        .ok_or_else(|| "probe missing".into())
}

pub fn last_frame(runner: &HeadlessRunner<TestAction>) -> LogicResult<&Snapshot> {
    probe(runner)?
        .frames
        .last()
        .ok_or_else(|| "frame observation missing".into())
}

pub fn sample(x: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, 50.0),
        LogicalViewport::new(900.0, 600.0)?,
    )?)
}

pub fn key(key: PhysicalKeyCode, state: ButtonState) -> InputEvent {
    InputEvent::key(key, state)
}
pub fn mouse(button: MouseButton, state: ButtonState) -> InputEvent {
    InputEvent::mouse_button(button, state)
}

pub fn controls() -> Vec<InputControl> {
    KEYS.into_iter()
        .map(InputControl::Key)
        .chain(BUTTONS.into_iter().map(InputControl::MouseButton))
        .collect()
}

pub fn all_presses() -> Vec<InputEvent> {
    KEYS.into_iter()
        .map(|k| key(k, ButtonState::Pressed))
        .chain(BUTTONS.into_iter().map(|b| mouse(b, ButtonState::Pressed)))
        .collect()
}

use std::time::Duration;

use sim_logic::prelude::*;

pub(super) const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TestAction {
    Place,
    Pause,
    Replace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) pointer: Option<PointerSample>,
    pub(super) held: bool,
    pub(super) presses: Vec<ActionEdge<TestAction>>,
    pub(super) releases: Vec<ActionEdge<TestAction>>,
}

#[derive(Default)]
pub(super) struct Probe {
    pub(super) fixed: Vec<Snapshot>,
    pub(super) frames: Vec<Snapshot>,
}

pub(super) fn sample(x: f32, y: f32, width: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(width, 600.0)?,
    )?)
}

pub(super) fn advance<A: Action>(
    runner: &mut HeadlessRunner<A>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    // A frame's current viewport deliberately differs from click samples.
    let viewport = LogicalViewport::new(1_000.0, 600.0)?;
    let report = match runner.advance_frame(FrameRequest::new(elapsed, events, viewport)) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    if let Some(failure) = report.failure() {
        return Err(format!("pointer frame failed: {failure}").into());
    }
    Ok(report)
}

pub(super) fn observe_fixed(
    input: FixedInput<TestAction>,
    frame: FrameInput<TestAction>,
    mut probe: AppResMut<Probe>,
) {
    assert_eq!(frame.pointer(), None);
    assert!(!frame.held(TestAction::Place));
    assert_eq!(frame.pressed(TestAction::Place).count(), 0);
    probe.fixed.push(Snapshot {
        pointer: input.pointer(),
        held: input.held(TestAction::Place),
        presses: input.pressed(TestAction::Place).collect(),
        releases: input.released(TestAction::Place).collect(),
    });
}

fn observe_frame(
    input: FrameInput<TestAction>,
    fixed: FixedInput<TestAction>,
    mut probe: AppResMut<Probe>,
) {
    assert_eq!(fixed.pointer(), None);
    assert!(!fixed.held(TestAction::Place));
    assert_eq!(fixed.pressed(TestAction::Place).count(), 0);
    probe.frames.push(Snapshot {
        pointer: input.pointer(),
        held: input.held(TestAction::Place),
        presses: input.pressed(TestAction::Place).collect(),
        releases: input.released(TestAction::Place).collect(),
    });
}

fn toggle_pause(
    input: FrameInput<TestAction>,
    time: FrameTime,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(TestAction::Pause) {
        commands.set_paused(!time.is_paused())?;
    }
    Ok(())
}

pub(super) fn application() -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
    let mut application = Application::new(config)?;
    application.register_app_resource(Probe::default())?;
    application.bind_mouse_button(MouseButton::Left, TestAction::Place)?;
    application.bind_key(PhysicalKeyCode::Space, TestAction::Place)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Pause)?;
    application.bind_key(PhysicalKeyCode::Escape, TestAction::Replace)?;
    application.add_fixed_system(observe_fixed);
    application.add_frame_system(observe_frame);
    application.add_fallible_frame_system(toggle_pause);
    Ok(application)
}

pub(super) fn register_world(
    application: &mut Application<TestAction>,
    name: &str,
) -> LogicResult<WorldFactoryId> {
    let camera = ActiveCamera2d::centered(20.0)?;
    Ok(application.register_world(name, move |world| {
        world.spawn(camera)?;
        Ok(())
    })?)
}

pub(super) fn runner() -> LogicResult<HeadlessRunner<TestAction>> {
    let mut application = application()?;
    let initial = register_world(&mut application, "pointer-probe")?;
    Ok(application.build_headless(initial)?)
}

pub(super) fn probe(runner: &HeadlessRunner<TestAction>) -> LogicResult<&Probe> {
    runner
        .app_resource::<Probe>()
        .ok_or_else(|| "probe disappeared".into())
}

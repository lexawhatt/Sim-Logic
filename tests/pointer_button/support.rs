use std::time::Duration;

use sim_logic::prelude::*;

pub(super) const STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TestAction {
    Shared,
    Disable,
    Enable,
    Replace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    First,
    Second,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventKind {
    Pressed,
    Clicked,
    InputCancelled(InputCancellationReason),
    ReleasedOutside,
    MissingPointer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RecordedEvent {
    pub(super) kind: EventKind,
    pub(super) target: Target,
    pub(super) pointer: Option<PointerSample>,
    pub(super) intent: TransitionIntentToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Observation {
    pub(super) edge: ActionEdge<TestAction>,
    pub(super) claimed: bool,
    pub(super) event: Option<RecordedEvent>,
    pub(super) captured: Option<Target>,
}

#[derive(Default)]
pub(super) struct Probe {
    pub(super) frames: Vec<Vec<Observation>>,
    pub(super) fixed: Vec<Vec<ActionEdge<TestAction>>>,
    pub(super) explicit_cancellations: Vec<Option<Target>>,
    pub(super) background_presses: usize,
    pub(super) background_releases: usize,
}

#[derive(Resource)]
pub(super) struct Interaction {
    pub(super) button: PointerButton<Target>,
    first: ScreenRectangleVisual,
    second: ScreenRectangleVisual,
    first_enabled: bool,
    unconditional_target: bool,
}

impl Interaction {
    fn new(
        first: ScreenRectangleVisual,
        second: ScreenRectangleVisual,
        unconditional_target: bool,
    ) -> Self {
        Self {
            button: PointerButton::new(MouseButton::Left),
            first,
            second,
            first_enabled: true,
            unconditional_target,
        }
    }

    fn hit(&self, pointer: Option<PointerSample>) -> Option<Target> {
        if self.unconditional_target {
            return Some(Target::First);
        }
        let pointer = pointer?;
        if self.first_enabled && self.first.contains_pointer(pointer) {
            Some(Target::First)
        } else if self.second.contains_pointer(pointer) {
            Some(Target::Second)
        } else {
            None
        }
    }
}

fn record(event: PointerButtonEvent<Target>) -> RecordedEvent {
    match event {
        PointerButtonEvent::Pressed {
            target,
            pointer,
            intent,
        } => RecordedEvent {
            kind: EventKind::Pressed,
            target,
            pointer: Some(pointer),
            intent,
        },
        PointerButtonEvent::Clicked {
            target,
            pointer,
            intent,
        } => RecordedEvent {
            kind: EventKind::Clicked,
            target,
            pointer: Some(pointer),
            intent,
        },
        PointerButtonEvent::Cancelled {
            target,
            reason,
            intent,
        } => RecordedEvent {
            kind: match reason {
                PointerButtonCancellation::Input(reason) => EventKind::InputCancelled(reason),
                PointerButtonCancellation::ReleasedOutside => EventKind::ReleasedOutside,
                PointerButtonCancellation::MissingPointer => EventKind::MissingPointer,
                _ => panic!("unexpected pointer button cancellation"),
            },
            target,
            pointer: None,
            intent,
        },
        _ => panic!("unexpected pointer button event"),
    }
}

fn observe_frame(
    input: FrameInput<TestAction>,
    mut interaction: ResMut<Interaction>,
    mut probe: AppResMut<Probe>,
) {
    let mut observations = Vec::new();
    for edge in input.edges() {
        if edge.state() == ButtonState::Pressed {
            match edge.action() {
                TestAction::Disable => {
                    probe
                        .explicit_cancellations
                        .push(interaction.button.cancel());
                    interaction.first_enabled = false;
                }
                TestAction::Enable => interaction.first_enabled = true,
                _ => {}
            }
        }
        let hit = interaction.hit(edge.pointer());
        let outcome = interaction.button.process(edge, hit);
        if !outcome.claimed() && edge.control() == InputControl::MouseButton(MouseButton::Left) {
            match edge.state() {
                ButtonState::Pressed => probe.background_presses += 1,
                ButtonState::Released => probe.background_releases += 1,
            }
        }
        observations.push(Observation {
            edge,
            claimed: outcome.claimed(),
            event: outcome.event().map(record),
            captured: interaction.button.captured(),
        });
    }
    probe.frames.push(observations);
}

fn observe_fixed(input: FixedInput<TestAction>, mut probe: AppResMut<Probe>) {
    probe.fixed.push(
        input
            .pressed(TestAction::Shared)
            .chain(input.released(TestAction::Shared))
            .collect(),
    );
}

pub(super) fn application() -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 4)?);
    config.set_input_event_limit(128)?;
    let mut app = Application::new(config)?;
    app.register_app_resource(Probe::default())?;
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        app.bind_mouse_button(button, TestAction::Shared)?;
    }
    app.bind_key(PhysicalKeyCode::Space, TestAction::Shared)?;
    app.bind_key(PhysicalKeyCode::KeyD, TestAction::Disable)?;
    app.bind_key(PhysicalKeyCode::KeyN, TestAction::Enable)?;
    app.bind_key(PhysicalKeyCode::Escape, TestAction::Replace)?;
    app.add_fixed_system(observe_fixed);
    app.add_frame_system(observe_frame);
    Ok(app)
}

pub(super) fn register_world(
    app: &mut Application<TestAction>,
    name: &str,
    unconditional_target: bool,
) -> LogicResult<WorldFactoryId> {
    let camera = ActiveCamera2d::centered(20.0)?;
    let first = rectangle(10.0, 10.0, 100.0, 100.0)?;
    let second = rectangle(150.0, 10.0, 100.0, 100.0)?;
    Ok(app.register_world(name, move |world| {
        world.spawn(camera)?;
        world.insert_resource(Interaction::new(first, second, unconditional_target))?;
        Ok(())
    })?)
}

pub(super) fn runner(unconditional_target: bool) -> LogicResult<HeadlessRunner<TestAction>> {
    let mut app = application()?;
    let initial = register_world(&mut app, "pointer-buttons", unconditional_target)?;
    Ok(app.build_headless(initial)?)
}

pub(super) fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    let request = FrameRequest::new(elapsed, events, LogicalViewport::new(1_000.0, 700.0)?);
    match runner.advance_frame(request) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

pub(super) fn probe(runner: &HeadlessRunner<TestAction>) -> LogicResult<&Probe> {
    runner
        .app_resource::<Probe>()
        .ok_or_else(|| "probe missing".into())
}

pub(super) fn frame(runner: &HeadlessRunner<TestAction>) -> LogicResult<&[Observation]> {
    probe(runner)?
        .frames
        .last()
        .map(Vec::as_slice)
        .ok_or_else(|| "frame missing".into())
}

pub(super) fn captured(runner: &HeadlessRunner<TestAction>) -> LogicResult<Option<Target>> {
    Ok(runner
        .resource::<Interaction>()
        .ok_or("interaction missing")?
        .button
        .captured())
}

pub(super) fn events(runner: &HeadlessRunner<TestAction>) -> LogicResult<Vec<(EventKind, Target)>> {
    Ok(probe(runner)?
        .frames
        .iter()
        .flatten()
        .filter_map(|entry| entry.event.map(|event| (event.kind, event.target)))
        .collect())
}

pub(super) fn sample(x: f32, y: f32, width: f32, height: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(width, height)?,
    )?)
}

pub(super) fn motion(x: f32, y: f32) -> LogicResult<InputEvent> {
    Ok(InputEvent::pointer_moved(sample(x, y, 800.0, 600.0)?))
}

pub(super) fn press() -> InputEvent {
    InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed)
}

pub(super) fn release() -> InputEvent {
    InputEvent::mouse_button(MouseButton::Left, ButtonState::Released)
}

pub(super) fn key(key: PhysicalKeyCode, state: ButtonState) -> InputEvent {
    InputEvent::key(key, state)
}

pub(super) fn rectangle(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
) -> LogicResult<ScreenRectangleVisual> {
    Ok(ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, y),
        LogicalScreenVector::new(width, height),
        Color::WHITE,
    )?)
}

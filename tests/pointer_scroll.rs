//! Public wheel delivery, ordering, units, and lifecycle boundaries without a window.

use sim_logic::prelude::*;
use std::{collections::HashSet, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Click,
    Replace,
}

#[derive(Debug, Clone)]
struct Sample {
    events: Vec<FrameInputEvent<Action>>,
    scrolls: Vec<PointerScrollEvent>,
    pointer: Option<PointerSample>,
    focused: bool,
}

#[derive(Default)]
struct Probe {
    frames: Vec<Sample>,
    fixed_edges: usize,
    ticks: usize,
}

fn observe_frame(input: FrameInput<Action>, mut probe: AppResMut<Probe>) {
    probe.frames.push(Sample {
        events: input.events().collect(),
        scrolls: input.scroll_events().collect(),
        pointer: input.pointer(),
        focused: !input.focus_lost(),
    });
}

fn observe_fixed(
    frame: FrameInput<Action>,
    fixed: FixedInput<Action>,
    mut probe: AppResMut<Probe>,
) {
    assert_eq!(
        frame.events().count(),
        0,
        "frame events are inactive during fixed ticks"
    );
    assert_eq!(frame.scroll_events().count(), 0);
    probe.fixed_edges +=
        fixed.pressed(Action::Click).count() + fixed.released(Action::Click).count();
    probe.ticks += 1;
}

fn runner(limit: usize) -> LogicResult<HeadlessRunner<Action>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 4)?);
    config.set_input_event_limit(limit)?;
    let mut app = Application::new(config)?;
    app.register_app_resource(Probe::default())?;
    app.bind_mouse_button(MouseButton::Left, Action::Click)?;
    app.bind_key(PhysicalKeyCode::KeyN, Action::Replace)?;
    app.add_frame_system(observe_frame);
    app.add_fixed_system(observe_fixed);
    let camera = ActiveCamera2d::centered(20.0)?;
    let target = app.register_world("scroll-target", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    app.add_fallible_frame_system(
        move |input: FrameInput<Action>, mut commands: Commands| -> LogicResult {
            if let Some(edge) = input.pressed(Action::Replace).next() {
                commands.replace_world(edge.intent(), target)?;
            }
            Ok(())
        },
    );
    let initial = app.register_world("scroll-initial", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    Ok(app.build_headless(initial)?)
}

fn viewport() -> LogicResult<LogicalViewport> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

fn point(x: f32, y: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        viewport()?,
    )?)
}

fn advance(
    runner: &mut HeadlessRunner<Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(elapsed, events, viewport()?)) {
        FrameOutcome::Advanced(report) if report.failure().is_none() => Ok(report),
        outcome => Err(format!("scroll frame failed: {outcome:?}").into()),
    }
}

fn probe(runner: &HeadlessRunner<Action>) -> LogicResult<&Probe> {
    runner
        .app_resource::<Probe>()
        .ok_or_else(|| "missing scroll probe".into())
}

fn last(runner: &HeadlessRunner<Action>) -> LogicResult<&Sample> {
    probe(runner)?
        .frames
        .last()
        .ok_or_else(|| "missing scroll frame".into())
}

#[test]
fn wheel_and_clicks_preserve_interleaving_event_positions_and_distinct_units() -> LogicResult {
    let mut runner = runner(16)?;
    let a = point(20.0, 30.0)?;
    let b = point(120.0, 130.0)?;
    let c = point(220.0, 230.0)?;
    let line = ScrollDelta::lines(0.5, 1.25)?;
    let pixels = ScrollDelta::pixels(-7.25, 2.5)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(a),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(b),
            InputEvent::mouse_wheel(line),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::mouse_wheel(pixels),
            InputEvent::pointer_moved(c),
        ],
    )?;
    let sample = last(&runner)?;
    let [
        FrameInputEvent::Action(press),
        FrameInputEvent::Scroll(first),
        FrameInputEvent::Action(release),
        FrameInputEvent::Scroll(second),
    ] = sample.events.as_slice()
    else {
        return Err("wheel must stay between its neighboring action edges".into());
    };
    assert_eq!(press.pointer(), Some(a));
    assert_eq!(first.pointer(), Some(b));
    assert_eq!(release.pointer(), Some(b));
    assert_eq!(second.pointer(), Some(b));
    assert_eq!(first.delta(), line);
    assert_eq!(second.delta(), pixels);
    assert_eq!(sample.pointer, Some(c));
    assert_eq!(sample.scrolls, [*first, *second]);
    Ok(())
}

#[test]
fn wheel_is_frame_only_and_survives_pause_without_fixed_replay() -> LogicResult {
    let mut runner = runner(8)?;
    let wheel = InputEvent::mouse_wheel(ScrollDelta::lines(0.0, 0.125)?);
    advance(&mut runner, Duration::ZERO, &[wheel, wheel])?;
    assert_eq!(
        last(&runner)?.scrolls.len(),
        2,
        "identical occurrences do not coalesce"
    );
    advance(&mut runner, Duration::from_millis(30), &[])?;
    assert!(last(&runner)?.scrolls.is_empty());
    assert_eq!(probe(&runner)?.ticks, 3);
    assert_eq!(probe(&runner)?.fixed_edges, 0);
    runner.set_paused(true);
    advance(&mut runner, Duration::from_millis(30), &[wheel])?;
    assert_eq!(last(&runner)?.scrolls.len(), 1);
    assert_eq!(probe(&runner)?.ticks, 3);
    runner.set_paused(false);
    advance(&mut runner, Duration::from_millis(10), &[])?;
    assert!(last(&runner)?.events.is_empty());
    assert_eq!(probe(&runner)?.ticks, 4);
    Ok(())
}

#[test]
fn unknown_pointer_and_pointer_leave_remain_explicit_in_order() -> LogicResult {
    let mut runner = runner(8)?;
    let wheel = InputEvent::mouse_wheel(ScrollDelta::pixels(2.0, -3.0)?);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            wheel,
            InputEvent::pointer_moved(point(30.0, 40.0)?),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            wheel,
            InputEvent::PointerLeft,
            wheel,
        ],
    )?;
    let events = &last(&runner)?.events;
    assert_eq!(last(&runner)?.scrolls[0].pointer(), None);
    assert_eq!(
        last(&runner)?.scrolls[1].pointer(),
        Some(point(30.0, 40.0)?)
    );
    assert_eq!(last(&runner)?.scrolls[2].pointer(), None);
    let FrameInputEvent::Action(cancelled) = events[3] else {
        return Err("pointer leave must stay between wheel events".into());
    };
    assert_eq!(
        cancelled.cancellation_reason(),
        Some(InputCancellationReason::PointerLeft)
    );
    Ok(())
}

#[test]
fn focus_loss_discards_all_wheel_but_retains_action_cancellation() -> LogicResult {
    let mut runner = runner(8)?;
    let wheel = InputEvent::mouse_wheel(ScrollDelta::lines(0.0, 1.0)?);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            wheel,
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::FocusLost,
            wheel,
        ],
    )?;
    assert!(!last(&runner)?.focused);
    assert!(last(&runner)?.scrolls.is_empty());
    assert_eq!(last(&runner)?.events.len(), 2);
    advance(&mut runner, Duration::ZERO, &[wheel])?;
    assert!(last(&runner)?.focused);
    assert_eq!(last(&runner)?.scrolls.len(), 1);
    Ok(())
}

#[test]
fn wheel_counts_against_physical_budget_and_rejected_batch_does_not_replay() -> LogicResult {
    let mut runner = runner(2)?;
    let wheel = InputEvent::mouse_wheel(ScrollDelta::lines(0.0, 1.0)?);
    let initial = point(10.0, 20.0)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::pointer_moved(initial), wheel],
    )?;
    let rejected = [
        InputEvent::pointer_moved(point(200.0, 300.0)?),
        wheel,
        wheel,
    ];
    assert!(matches!(
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &rejected, viewport()?)),
        FrameOutcome::Rejected(BeginFrameRejection::Input(
            InputCollectionError::EventLimitExceeded {
                limit: 2,
                received: 3
            }
        ))
    ));
    assert_eq!(probe(&runner)?.frames.len(), 1);
    advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(last(&runner)?.pointer, Some(initial));
    assert!(last(&runner)?.events.is_empty());
    Ok(())
}

#[test]
fn wheel_is_retired_at_world_replacement() -> LogicResult {
    let mut runner = runner(8)?;
    let report = advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::mouse_wheel(ScrollDelta::lines(0.0, 1.0)?),
            InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Pressed),
        ],
    )?;
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    advance(&mut runner, Duration::from_millis(10), &[])?;
    assert!(last(&runner)?.events.is_empty());
    assert_eq!(probe(&runner)?.fixed_edges, 0);
    Ok(())
}

#[test]
fn wheel_validation_bounds_and_hash_identity_preserve_units_and_zero() -> LogicResult {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            ScrollDelta::lines(invalid, 0.0),
            Err(ScrollDeltaError::NonFinite)
        );
        assert_eq!(
            ScrollDelta::pixels(0.0, invalid),
            Err(ScrollDeltaError::NonFinite)
        );
    }
    let maximum = ScrollDelta::MAX_DISPLACEMENT;
    assert_eq!(
        ScrollDelta::lines(maximum + 1.0, 0.0),
        Err(ScrollDeltaError::OutOfRange)
    );
    assert_eq!(
        ScrollDelta::pixels(0.0, -maximum - 1.0),
        Err(ScrollDeltaError::OutOfRange)
    );
    assert_eq!(ScrollDelta::pixels(maximum, -maximum)?.y(), -maximum);
    let mut values = HashSet::new();
    values.insert(ScrollDelta::lines(-0.0, 0.0)?);
    values.insert(ScrollDelta::lines(0.0, -0.0)?);
    values.insert(ScrollDelta::pixels(0.0, 0.0)?);
    assert_eq!(values.len(), 2);
    assert_eq!(ScrollDelta::lines(0.0, 0.0)?.unit(), ScrollUnit::Lines);
    assert_eq!(ScrollDelta::pixels(0.0, 0.0)?.unit(), ScrollUnit::Pixels);
    Ok(())
}

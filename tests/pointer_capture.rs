//! Public, window-free relative motion and capture-intent contract.

use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Capture,
    Release,
}

#[derive(Clone, Copy, Debug)]
struct Sample {
    motion: RelativePointerMotion,
    focus_lost: bool,
    requested: bool,
}

#[derive(Default)]
struct Probe {
    frames: Vec<Sample>,
    fixed: usize,
    focused_ticks: usize,
    lost_focus_ticks: usize,
}

fn route(
    input: FrameInput<Action>,
    fixed: FixedInput<Action>,
    mut capture: AppResMut<PointerCapture>,
    mut probe: AppResMut<Probe>,
) {
    assert!(!fixed.focus_lost());
    if input.focus_lost() {
        assert!(
            !capture.requested(),
            "shared core cancels intent before systems run"
        );
    }
    if input.focus_lost() || input.has_press_occurrence(Action::Release) {
        capture.release();
    } else if input.has_press_occurrence(Action::Capture) {
        capture.request_lock();
    }
    probe.frames.push(Sample {
        motion: input.relative_motion(),
        focus_lost: input.focus_lost(),
        requested: capture.requested(),
    });
}

fn fixed(
    frame: FrameInput<Action>,
    input: FixedInput<Action>,
    capture: AppRes<PointerCapture>,
    mut probe: AppResMut<Probe>,
) {
    assert_eq!(frame.relative_motion(), RelativePointerMotion::ZERO);
    assert!(!frame.focus_lost());
    probe.fixed += 1;
    if input.focus_lost() {
        assert!(
            !capture.requested(),
            "capture intent clears before any fixed tick"
        );
        probe.lost_focus_ticks += 1;
        return;
    }
    probe.focused_ticks += 1;
}

fn runner() -> LogicResult<HeadlessRunner<Action>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 4)?);
    let mut app = Application::new(config)?;
    app.register_app_resource(PointerCapture::default())?;
    app.register_app_resource(Probe::default())?;
    app.bind_mouse_button(MouseButton::Left, Action::Capture)?;
    app.bind_key(PhysicalKeyCode::Escape, Action::Release)?;
    app.add_frame_system(route);
    app.add_fixed_system(fixed);
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = app.register_world("relative-motion", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    Ok(app.build_headless(initial)?)
}

fn advance(
    runner: &mut HeadlessRunner<Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult {
    let report = match runner.advance_frame(FrameRequest::new(
        elapsed,
        events,
        LogicalViewport::new(800.0, 600.0)?,
    )) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    if let Some(failure) = report.failure() {
        return Err(format!("motion frame failed: {failure}").into());
    }
    Ok(())
}

fn last(runner: &HeadlessRunner<Action>) -> LogicResult<Sample> {
    runner
        .app_resource::<Probe>()
        .and_then(|probe| probe.frames.last())
        .copied()
        .ok_or_else(|| "missing motion probe".into())
}

fn motion(x: f64, y: f64) -> LogicResult<InputEvent> {
    Ok(InputEvent::relative_pointer_motion(
        RelativePointerMotion::new(x, y)?,
    ))
}

#[test]
fn finite_motion_sums_once_without_fixed_catch_up_replay() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[motion(2.5, -4.0)?, motion(7.5, 1.0)?],
    )?;
    assert_eq!(
        last(&runner)?.motion,
        RelativePointerMotion::new(10.0, -3.0)?
    );
    assert!(!last(&runner)?.focus_lost);
    advance(&mut runner, Duration::from_millis(30), &[])?;
    assert_eq!(last(&runner)?.motion, RelativePointerMotion::ZERO);
    assert_eq!(runner.app_resource::<Probe>().ok_or("probe")?.fixed, 3);
    runner.set_paused(true);
    advance(&mut runner, Duration::from_millis(30), &[motion(1.0, 2.0)?])?;
    assert_eq!(last(&runner)?.motion, RelativePointerMotion::new(1.0, 2.0)?);
    assert_eq!(runner.app_resource::<Probe>().ok_or("probe")?.fixed, 3);
    Ok(())
}

#[test]
fn focus_loss_without_held_controls_clears_capture_and_whole_batch_motion() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
    )?;
    assert!(last(&runner)?.requested);
    let capture = runner.app_resource::<PointerCapture>().ok_or("capture")?;
    assert_eq!(capture.status(), PointerCaptureStatus::Released);
    assert!(
        !capture.is_captured(),
        "headless intent must not invent OS success"
    );
    advance(
        &mut runner,
        Duration::ZERO,
        &[motion(5.0, 4.0)?, InputEvent::FocusLost, motion(8.0, 9.0)?],
    )?;
    assert!(last(&runner)?.focus_lost);
    assert!(!last(&runner)?.requested);
    assert_eq!(last(&runner)?.motion, RelativePointerMotion::ZERO);
    advance(&mut runner, Duration::ZERO, &[])?;
    assert!(!last(&runner)?.focus_lost);
    assert!(!last(&runner)?.requested, "later frames do not reacquire");
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Pressed,
        )],
    )?;
    assert!(last(&runner)?.requested);
    Ok(())
}

#[test]
fn rejected_accumulated_motion_does_not_commit_a_frame_or_capture_mutation() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Pressed,
        )],
    )?;
    let before = runner.app_resource::<Probe>().ok_or("probe")?.frames.len();
    let events = [
        motion(RelativePointerMotion::MAX_DISPLACEMENT, 0.0)?,
        motion(1.0, 0.0)?,
    ];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::ZERO,
        &events,
        LogicalViewport::new(800.0, 600.0)?,
    ));
    assert!(matches!(
        outcome,
        FrameOutcome::Rejected(BeginFrameRejection::Input(
            InputCollectionError::RelativeMotion(RelativePointerMotionError::OutOfRange)
        ))
    ));
    assert_eq!(
        runner.app_resource::<Probe>().ok_or("probe")?.frames.len(),
        before
    );
    assert!(
        runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    advance(&mut runner, Duration::ZERO, &[motion(3.0, 0.0)?])?;
    assert_eq!(last(&runner)?.motion, RelativePointerMotion::new(3.0, 0.0)?);
    Ok(())
}

#[test]
fn motion_rejects_nonfinite_and_out_of_range_and_canonicalizes_signed_zero() -> LogicResult {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            RelativePointerMotion::new(value, 0.0),
            Err(RelativePointerMotionError::NonFinite)
        );
        assert_eq!(
            RelativePointerMotion::new(0.0, value),
            Err(RelativePointerMotionError::NonFinite)
        );
    }
    assert_eq!(
        RelativePointerMotion::new(RelativePointerMotion::MAX_DISPLACEMENT + 1.0, 0.0),
        Err(RelativePointerMotionError::OutOfRange)
    );
    assert_eq!(
        RelativePointerMotion::new(-0.0, 0.0)?,
        RelativePointerMotion::ZERO
    );
    let max = RelativePointerMotion::new(
        RelativePointerMotion::MAX_DISPLACEMENT,
        -RelativePointerMotion::MAX_DISPLACEMENT,
    )?;
    assert_eq!(max.checked_add(RelativePointerMotion::ZERO)?, max);
    Ok(())
}

#[test]
fn focus_boundary_stops_all_same_frame_catch_up_ticks_before_frame_routing() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
    )?;
    advance(
        &mut runner,
        Duration::from_millis(30),
        &[InputEvent::FocusLost],
    )?;
    let probe = runner.app_resource::<Probe>().ok_or("probe")?;
    assert_eq!(probe.fixed, 3);
    assert_eq!(probe.lost_focus_ticks, 3);
    assert_eq!(probe.focused_ticks, 0);
    advance(&mut runner, Duration::from_millis(10), &[])?;
    assert_eq!(
        runner.app_resource::<Probe>().ok_or("probe")?.focused_ticks,
        1
    );
    // A zero-tick boundary has no delayed fixed flag; cancellation edges keep
    // their separate retained delivery contract.
    advance(&mut runner, Duration::ZERO, &[InputEvent::FocusLost])?;
    advance(&mut runner, Duration::from_millis(10), &[])?;
    assert_eq!(
        runner
            .app_resource::<Probe>()
            .ok_or("probe")?
            .lost_focus_ticks,
        3
    );
    assert_eq!(
        runner.app_resource::<Probe>().ok_or("probe")?.focused_ticks,
        2
    );
    Ok(())
}

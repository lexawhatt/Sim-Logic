//! Public-runtime smoke checks of the actual desktop example's shared scene.

use std::time::Duration;

use sim_logic::prelude::*;

#[path = "../../examples/text_labels/scene.rs"]
mod scene;

const STEP: Duration = Duration::from_millis(100);

fn advance(
    runner: &mut HeadlessRunner<scene::Action>,
    delta: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        delta,
        events,
        LogicalViewport::new(1280.0, 720.0)?,
    )) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

fn snapshot(runner: &HeadlessRunner<scene::Action>) -> LogicResult<&ExtractedFrame> {
    runner
        .extracted_frame()
        .ok_or_else(|| "example snapshot missing".into())
}

fn counter(runner: &HeadlessRunner<scene::Action>) -> LogicResult<&str> {
    snapshot(runner)?
        .resolved_screen_texts()
        .iter()
        .find(|label| label.text().ends_with(" seconds"))
        .map(ResolvedScreenText::text)
        .ok_or_else(|| "example counter missing".into())
}

fn key(code: PhysicalKeyCode, state: ButtonState) -> InputEvent {
    InputEvent::key(code, state)
}

#[test]
fn actual_example_initial_content_and_counter_pause_resume_use_public_input() -> LogicResult {
    let (app, initial) = scene::build_application()?;
    let mut runner = app.build_headless(initial)?;
    assert_eq!(runner.text_font_count(), 3);
    assert!(runner.text_font_bytes() > 0);
    let frame = snapshot(&runner)?;
    assert_eq!(frame.resolved_screen_texts().len(), 11);
    assert_eq!(frame.resolved_screen_images().len(), 1);
    assert_eq!(frame.resolved_screen_rectangles().len(), 4);
    assert!(
        frame
            .resolved_screen_texts()
            .iter()
            .any(|label| label.text() == "Hello, world!  Привет, мир!")
    );
    assert!(
        frame
            .resolved_screen_texts()
            .iter()
            .any(|label| label.text() == "Нормальный текст. Наконец-то.")
    );
    assert_eq!(counter(&runner)?, "0 seconds");

    // Small exact ticks exercise the fixed clock without catch-up truncation.
    for _ in 0..10 {
        advance(&mut runner, STEP, &[])?;
    }
    assert_eq!(counter(&runner)?, "1 seconds");
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    for _ in 0..20 {
        advance(&mut runner, STEP, &[])?;
    }
    assert_eq!(counter(&runner)?, "1 seconds");

    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Released)],
    )?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Released)],
    )?;
    for _ in 0..10 {
        advance(&mut runner, STEP, &[])?;
    }
    assert_eq!(counter(&runner)?, "2 seconds");
    Ok(())
}

#[test]
fn actual_example_enter_replaces_world_and_keeps_font_registrations() -> LogicResult {
    let (app, initial) = scene::build_application()?;
    let mut runner = app.build_headless(initial)?;
    let generation = runner.world_generation();
    let font_bytes = runner.text_font_bytes();
    let mut fonts = Vec::new();
    for label in snapshot(&runner)?.resolved_screen_texts() {
        if !fonts.contains(label.font()) {
            fonts.push(label.font().clone());
        }
    }
    assert_eq!(fonts.len(), 3);
    for _ in 0..10 {
        advance(&mut runner, STEP, &[])?;
    }
    assert_eq!(counter(&runner)?, "1 seconds");
    // The routing adapter runs in FixedUpdate, not in a zero-delta frame.
    let pending = advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Enter, ButtonState::Pressed)],
    )?;
    assert!(matches!(pending.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), generation);
    let changed = advance(&mut runner, STEP, &[])?;
    assert!(matches!(
        changed.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), generation);
    assert_eq!(runner.text_font_count(), 3);
    assert_eq!(runner.text_font_bytes(), font_bytes);
    let frame = snapshot(&runner)?;
    assert_eq!(frame.resolved_screen_texts().len(), 11);
    assert!(
        frame
            .resolved_screen_texts()
            .iter()
            .any(|label| label.text() == "Новый World. Тот же шрифт.")
    );
    assert!(
        frame
            .resolved_screen_texts()
            .iter()
            .all(|label| fonts.contains(label.font()))
    );
    assert!(
        frame
            .resolved_screen_texts()
            .iter()
            .all(|label| label.source().world_generation() == runner.world_generation())
    );
    assert_eq!(counter(&runner)?, "0 seconds");
    Ok(())
}

#[test]
fn actual_example_escape_exits_while_paused_without_a_fixed_tick() -> LogicResult {
    let (app, initial) = scene::build_application()?;
    let mut runner = app.build_headless(initial)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    let generation = runner.world_generation();
    let exit = advance(
        &mut runner,
        Duration::ZERO,
        &[
            key(PhysicalKeyCode::Space, ButtonState::Released),
            key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        ],
    )?;
    assert!(exit.exit_requested());
    assert!(matches!(exit.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), generation);
    Ok(())
}

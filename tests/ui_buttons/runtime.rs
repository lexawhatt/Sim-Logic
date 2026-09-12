use std::time::Duration;

use sim_logic::prelude::*;

use super::{app, scene, view};

const STEP: Duration = Duration::from_millis(10);
const COUNT: (f32, f32) = (100.0, 120.0);
const PAUSE: (f32, f32) = (350.0, 120.0);
const TOGGLE: (f32, f32) = (600.0, 120.0);
const NEXT: (f32, f32) = (850.0, 120.0);
const GAME: (f32, f32) = (720.0, 560.0);

fn runner() -> LogicResult<HeadlessRunner<app::Action>> {
    let (application, initial) = app::build_application(TimeConfig::new(STEP, 4)?)?;
    Ok(application.build_headless(initial)?)
}

fn pointer((x, y): (f32, f32)) -> LogicResult<InputEvent> {
    Ok(InputEvent::pointer_moved(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(1000.0, 700.0)?,
    )?))
}

fn mouse(state: ButtonState) -> InputEvent {
    InputEvent::mouse_button(MouseButton::Left, state)
}

fn click(position: (f32, f32)) -> LogicResult<[InputEvent; 3]> {
    Ok([
        pointer(position)?,
        mouse(ButtonState::Pressed),
        mouse(ButtonState::Released),
    ])
}

fn advance(
    runner: &mut HeadlessRunner<app::Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
        elapsed,
        events,
        LogicalViewport::new(1000.0, 700.0)?,
    )) else {
        return Err("example frame rejected".into());
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    Ok(report)
}

fn counters(runner: &HeadlessRunner<app::Action>) -> LogicResult<&app::Counters> {
    runner
        .app_resource::<app::Counters>()
        .ok_or_else(|| "persistent counters".into())
}

fn board(runner: &HeadlessRunner<app::Action>) -> LogicResult<&scene::Board> {
    runner
        .resource::<scene::Board>()
        .ok_or_else(|| "World-local board".into())
}

#[test]
fn event_time_clicks_are_exclusive_and_retained_fixed_edges_never_repeat_ui() -> LogicResult {
    let mut runner = runner()?;
    let count_click = click(COUNT)?;
    let game_click = click(GAME)?;
    let events = [
        count_click[0],
        count_click[1],
        count_click[2],
        game_click[0],
        game_click[1],
        game_click[2],
        pointer(NEXT)?,
    ];
    let zero = advance(&mut runner, Duration::ZERO, &events)?;
    assert_eq!(zero.fixed_ticks_attempted(), 0);
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(counters(&runner)?.background, 1);
    assert_eq!(counters(&runner)?.replacements, 0);
    assert_eq!(runner.components::<scene::Marker>().count(), 1);
    let caught_up = advance(&mut runner, STEP * 3, &[])?;
    assert_eq!(caught_up.fixed_ticks_attempted(), 3);
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(counters(&runner)?.background, 1);
    assert_eq!(board(&runner)?.ticks, 3);
    // A keyboard alias must never become a mouse press or place a marker.
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
        ],
    )?;
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(counters(&runner)?.background, 1);
    Ok(())
}

#[test]
fn cancelled_and_outside_gestures_cannot_commit_another_target() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(COUNT)?,
            mouse(ButtonState::Pressed),
            pointer(GAME)?,
            mouse(ButtonState::Released),
        ],
    )?;
    assert_eq!(counters(&runner)?.cancellations, 1);
    assert_eq!(counters(&runner)?.background, 0);
    // An outside press belongs to the game; releasing over COUNT cannot adopt it.
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(GAME)?,
            mouse(ButtonState::Pressed),
            pointer(COUNT)?,
            mouse(ButtonState::Released),
        ],
    )?;
    assert_eq!(counters(&runner)?.background, 1);
    assert_eq!(counters(&runner)?.clicks, 0);
    for cancellation in [InputEvent::PointerLeft, InputEvent::FocusLost] {
        advance(
            &mut runner,
            Duration::ZERO,
            &[pointer(COUNT)?, mouse(ButtonState::Pressed), cancellation],
        )?;
        assert!(board(&runner)?.pointer.captured().is_none());
    }
    assert_eq!(counters(&runner)?.cancellations, 3);
    assert_eq!(counters(&runner)?.clicks, 0);
    advance(&mut runner, Duration::ZERO, &click(COUNT)?)?;
    assert_eq!(counters(&runner)?.clicks, 1);
    Ok(())
}

#[test]
fn disabling_during_capture_suppresses_release_even_if_reenabled_first() -> LogicResult {
    let mut runner = runner()?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(COUNT)?,
            mouse(ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            mouse(ButtonState::Released),
        ],
    )?;
    assert!(board(&runner)?.count_enabled);
    assert_eq!(counters(&runner)?.cancellations, 1);
    assert_eq!(counters(&runner)?.clicks, 0);
    assert_eq!(counters(&runner)?.background, 0);
    advance(&mut runner, Duration::ZERO, &click(COUNT)?)?;
    assert_eq!(counters(&runner)?.clicks, 1);
    advance(&mut runner, Duration::ZERO, &click(TOGGLE)?)?;
    assert!(!board(&runner)?.count_enabled);
    advance(&mut runner, Duration::ZERO, &click(COUNT)?)?;
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(
        counters(&runner)?.background,
        0,
        "disabled panel is not background"
    );
    Ok(())
}

#[test]
fn pause_keeps_buttons_live_and_freezes_fixed_ticks_and_background() -> LogicResult {
    let mut runner = runner()?;
    advance(&mut runner, STEP, &[])?;
    advance(&mut runner, Duration::ZERO, &click(PAUSE)?)?;
    assert!(runner.is_paused());
    let paused = advance(&mut runner, STEP * 3, &click(COUNT)?)?;
    assert_eq!(paused.fixed_ticks_attempted(), 0);
    advance(&mut runner, Duration::ZERO, &click(GAME)?)?;
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(counters(&runner)?.background, 0);
    assert_eq!(board(&runner)?.ticks, 1);
    advance(&mut runner, Duration::ZERO, &click(PAUSE)?)?;
    assert!(!runner.is_paused());
    advance(&mut runner, STEP, &click(GAME)?)?;
    assert_eq!(board(&runner)?.ticks, 2);
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(counters(&runner)?.background, 1);
    Ok(())
}

#[test]
fn two_factory_replacements_reset_local_ownership_and_keep_counters() -> LogicResult {
    let mut runner = runner()?;
    advance(&mut runner, STEP, &click(COUNT)?)?;
    advance(&mut runner, Duration::ZERO, &click(GAME)?)?;
    advance(&mut runner, Duration::ZERO, &click(TOGGLE)?)?;
    let old_target = board(&runner)?.count_target;
    let old_generation = runner.world_generation();
    let next = click(NEXT)?;
    let tail = click(GAME)?;
    let replaced = advance(
        &mut runner,
        Duration::ZERO,
        &[next[0], next[1], next[2], tail[0], tail[1], tail[2]],
    )?;
    assert!(matches!(
        replaced.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), old_generation);
    assert_eq!(runner.active_world_name(), "ui-buttons-harbor");
    assert_eq!(board(&runner)?.world_index, 1);
    assert!(board(&runner)?.count_enabled);
    assert!(board(&runner)?.pointer.captured().is_none());
    assert_ne!(board(&runner)?.count_target, old_target);
    assert_eq!(board(&runner)?.ticks, 0);
    assert_eq!(runner.components::<scene::Marker>().count(), 0);
    assert_eq!(counters(&runner)?.clicks, 1);
    assert_eq!(
        counters(&runner)?.background,
        1,
        "old-World tail must not place"
    );
    assert_eq!(counters(&runner)?.replacements, 1);
    advance(&mut runner, Duration::ZERO, &click(COUNT)?)?;
    let returned = advance(&mut runner, Duration::ZERO, &click(NEXT)?)?;
    assert!(matches!(
        returned.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_eq!(runner.active_world_name(), "ui-buttons-garden");
    assert_eq!(board(&runner)?.world_index, 0);
    assert_eq!(board(&runner)?.ticks, 0);
    assert!(board(&runner)?.pointer.captured().is_none());
    assert_eq!(counters(&runner)?.clicks, 2);
    assert_eq!(counters(&runner)?.background, 1);
    assert_eq!(counters(&runner)?.replacements, 2);
    Ok(())
}

#[test]
fn marker_limit_and_clear_leave_the_persistent_game_counter_intact() -> LogicResult {
    let mut runner = runner()?;
    for _ in 0..app::MAX_MARKERS + 1 {
        advance(&mut runner, STEP, &click(GAME)?)?;
    }
    assert_eq!(
        runner.components::<scene::Marker>().count(),
        app::MAX_MARKERS
    );
    assert_eq!(counters(&runner)?.background, app::MAX_MARKERS as u32);
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(PhysicalKeyCode::KeyR, ButtonState::Pressed)],
    )?;
    assert_eq!(runner.components::<scene::Marker>().count(), 0);
    assert_eq!(counters(&runner)?.background, app::MAX_MARKERS as u32);
    advance(&mut runner, Duration::ZERO, &click(GAME)?)?;
    assert_eq!(runner.components::<scene::Marker>().count(), 1);
    assert_eq!(counters(&runner)?.background, app::MAX_MARKERS as u32 + 1);
    Ok(())
}

#[test]
fn managed_labels_are_small_and_retain_unchanged_text_until_a_displayed_value_changes()
-> LogicResult {
    let mut runner = runner()?;
    assert_eq!(runner.text_font_count(), 2);
    let font_bytes = runner.text_font_bytes();
    let snapshot = runner.extracted_frame().ok_or("initial snapshot")?;
    assert_eq!(snapshot.resolved_screen_rectangles().len(), 7);
    assert_eq!(snapshot.resolved_screen_texts().len(), 28);
    let (timer, text) = runner
        .components::<ScreenTextVisual>()
        .find(|(_, text)| text.text() == "0 s")
        .ok_or("simulation time label")?;
    let initial_storage = text.text().as_ptr();
    // Both idle frames and sub-second simulation changes retain the same label.
    advance(&mut runner, Duration::ZERO, &[])?;
    for _ in 0..24 {
        advance(&mut runner, STEP * 4, &[])?;
    }
    let unchanged = runner.component::<ScreenTextVisual>(timer)?;
    assert_eq!(unchanged.text(), "0 s");
    assert_eq!(unchanged.text().as_ptr(), initial_storage);
    advance(&mut runner, STEP * 4, &[])?;
    assert_eq!(runner.component::<ScreenTextVisual>(timer)?.text(), "1 s");
    advance(&mut runner, Duration::ZERO, &click(COUNT)?)?;
    advance(&mut runner, Duration::ZERO, &click(NEXT)?)?;
    assert_eq!(runner.text_font_count(), 2);
    assert_eq!(runner.text_font_bytes(), font_bytes);
    let snapshot = runner.extracted_frame().ok_or("replacement snapshot")?;
    assert_eq!(snapshot.resolved_screen_rectangles().len(), 7);
    assert_eq!(snapshot.resolved_screen_texts().len(), 28);
    assert!(
        snapshot
            .resolved_screen_texts()
            .iter()
            .any(|label| label.text() == "0 s")
    );
    // Factories/Startup cannot read application resources. Canonical counters
    // already survived, while inherited labels refresh on the next frame.
    assert_eq!(counters(&runner)?.clicks, 1);
    advance(&mut runner, Duration::ZERO, &[])?;
    let (clicks, _) = runner
        .components::<view::Label>()
        .find(|(_, label)| {
            matches!(
                label,
                view::Label::Value {
                    kind: view::Value::Clicks,
                    ..
                }
            )
        })
        .ok_or("click label in replacement")?;
    assert_eq!(runner.component::<ScreenTextVisual>(clicks)?.text(), "1");
    Ok(())
}

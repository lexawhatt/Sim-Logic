//! Public-runtime acceptance with synthetic input only; no window or GPU.

use std::time::Duration;

use sim_logic::prelude::*;

use super::territory_wars::{
    self, Action, Session,
    layout::{self, Layout},
    simulation::{FACTIONS, Faction, FactionDiagnostics, Game, HEIGHT, NEUTRAL, Phase, WIDTH},
};

const SEED: u64 = territory_wars::DEFAULT_SEED;
const TICK: Duration = Duration::from_millis(100);

#[test]
fn prepared_demo_uses_real_rules_and_remains_unassisted() -> LogicResult {
    let (app, initial) = territory_wars::app::build_demo(SEED, true)?;
    let mut runner = app.build_headless(initial)?;
    let mut expected = Game::new(SEED);
    assert!(expected.start(home(&expected)));
    for tick in 0..240 {
        if tick % 15 == 0 {
            let _ = expected.expand(25);
        }
        expected.tick();
    }
    advance(&mut runner, Duration::ZERO, &[], viewport())?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    assert!(session(&runner).debug);
    assert!(!session(&runner).cheats && !session(&runner).assisted);
    for _ in 0..5 {
        expected.tick();
    }
    advance(&mut runner, TICK * 5, &[], viewport())?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    Ok(())
}

fn viewport() -> LogicalViewport {
    LogicalViewport::new(1440.0, 900.0).unwrap()
}

fn build() -> LogicResult<HeadlessRunner<Action>> {
    let (application, initial) = territory_wars::build_application(SEED)?;
    Ok(application.build_headless(initial)?)
}

fn session(runner: &HeadlessRunner<Action>) -> &Session {
    runner.app_resource::<Session>().unwrap()
}

fn advance(
    runner: &mut HeadlessRunner<Action>,
    delta: Duration,
    events: &[InputEvent],
    viewport: LogicalViewport,
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(delta, events, viewport)) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

fn key(key: PhysicalKeyCode) -> [InputEvent; 2] {
    [
        InputEvent::key(key, ButtonState::Pressed),
        InputEvent::key(key, ButtonState::Released),
    ]
}

fn press(runner: &mut HeadlessRunner<Action>, code: PhysicalKeyCode) -> LogicResult {
    advance(runner, Duration::ZERO, &key(code), viewport())?;
    Ok(())
}

fn pointer(x: f32, y: f32, viewport: LogicalViewport) -> InputEvent {
    let position = Layout::new(viewport).position(x, y);
    InputEvent::pointer_moved(PointerSample::new(position, viewport).unwrap())
}

fn click(x: f32, y: f32, viewport: LogicalViewport) -> [InputEvent; 3] {
    [
        pointer(x, y, viewport),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
    ]
}

fn home(game: &Game) -> usize {
    game.owners()
        .iter()
        .enumerate()
        .filter(|(_, owner)| **owner == NEUTRAL)
        .min_by_key(|(cell, _)| {
            (cell % WIDTH).abs_diff(WIDTH / 2) + (cell / WIDTH).abs_diff(HEIGHT / 2)
        })
        .unwrap()
        .0
}

fn started() -> LogicResult<(HeadlessRunner<Action>, Game)> {
    let mut runner = build()?;
    let cell = home(&session(&runner).game);
    let (x, y) = layout::cell_center(cell);
    advance(
        &mut runner,
        Duration::ZERO,
        &click(x, y, viewport()),
        viewport(),
    )?;
    let mut expected = Game::new(SEED);
    assert!(expected.start(cell));
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    Ok((runner, expected))
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    owners: Vec<u8>,
    factions: [Faction; FACTIONS],
    diagnostics: [FactionDiagnostics; FACTIONS],
    armies: [u32; FACTIONS],
    phase: Phase,
    ticks: u64,
    seed: u64,
    bots: bool,
}

fn snapshot(game: &Game) -> Snapshot {
    Snapshot {
        owners: game.owners().to_vec(),
        factions: *game.factions(),
        diagnostics: *game.diagnostics(),
        armies: std::array::from_fn(|index| game.campaign_remaining(index)),
        phase: game.phase(),
        ticks: game.elapsed_ticks(),
        seed: game.seed(),
        bots: game.bots_enabled(),
    }
}

#[test]
fn zero_tick_home_click_uses_its_own_pointer_and_viewport() -> LogicResult {
    let mut runner = build()?;
    let cell = home(&session(&runner).game);
    let (x, y) = layout::cell_center(cell);
    let captured = LogicalViewport::new(720.0, 1000.0)?;
    let mut events = click(x, y, captured).to_vec();
    events.push(pointer(0.0, 0.0, viewport()));
    let report = advance(&mut runner, Duration::ZERO, &events, viewport())?;
    assert_eq!(report.fixed_ticks_attempted(), 0);
    assert_eq!(session(&runner).game.phase(), Phase::Running);
    assert_eq!(session(&runner).game.factions()[0].capital, cell);
    assert_eq!(session(&runner).game.elapsed_ticks(), 0);
    Ok(())
}

#[test]
fn missing_pointer_water_and_letterbox_clicks_do_not_choose_a_home() -> LogicResult {
    let mut runner = build()?;
    let initial = snapshot(&session(&runner).game);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    let (x, y) = layout::cell_center(0);
    advance(
        &mut runner,
        Duration::ZERO,
        &click(x, y, viewport()),
        viewport(),
    )?;
    let portrait = LogicalViewport::new(400.0, 1000.0)?;
    let sample = PointerSample::new(LogicalScreenPosition::new(200.0, 2.0), portrait)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(sample),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        portrait,
    )?;
    assert_eq!(snapshot(&session(&runner).game), initial);
    Ok(())
}

#[test]
fn one_click_dispatches_once_and_catch_up_does_not_repeat_it() -> LogicResult {
    let (mut runner, mut expected) = started()?;
    let neutral = expected
        .owners()
        .iter()
        .position(|&owner| owner == NEUTRAL)
        .unwrap();
    let (x, y) = layout::cell_center(neutral);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(x, y, viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        viewport(),
    )?;
    expected.expand(25)?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    for _ in 0..2 {
        let report = advance(&mut runner, TICK * 8, &[], viewport())?;
        assert_eq!(report.fixed_ticks_attempted(), 8);
        for _ in 0..8 {
            expected.tick();
        }
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    }
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Released,
        )],
        viewport(),
    )?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    Ok(())
}

#[test]
fn pause_owns_simultaneous_orders_in_either_event_order() -> LogicResult {
    let (mut runner, expected) = started()?;
    let neutral = expected
        .owners()
        .iter()
        .position(|&owner| owner == NEUTRAL)
        .unwrap();
    let (x, y) = layout::cell_center(neutral);
    for pause_first in [false, true] {
        let mut events = key(PhysicalKeyCode::Space).to_vec();
        events.extend(click(x, y, viewport()));
        if pause_first {
            events.splice(0..0, key(PhysicalKeyCode::KeyP));
        } else {
            events.extend(key(PhysicalKeyCode::KeyP));
        }
        advance(&mut runner, Duration::ZERO, &events, viewport())?;
        assert!(session(&runner).paused);
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
        let report = advance(&mut runner, TICK * 8, &[], viewport())?;
        assert_eq!(report.fixed_ticks_attempted(), 0);
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
        press(&mut runner, PhysicalKeyCode::KeyP)?;
        assert!(!session(&runner).paused);
    }
    Ok(())
}

#[test]
fn restart_discards_same_frame_pause_cheats_and_start_click() -> LogicResult {
    let (mut runner, _) = started()?;
    press(&mut runner, PhysicalKeyCode::F4)?;
    press(&mut runner, PhysicalKeyCode::F5)?;
    assert!(session(&runner).assisted);
    press(&mut runner, PhysicalKeyCode::KeyP)?;
    let initial = Game::new(SEED);
    let (x, y) = layout::cell_center(home(&initial));
    let mut events = click(x, y, viewport()).to_vec();
    for code in [
        PhysicalKeyCode::KeyP,
        PhysicalKeyCode::F5,
        PhysicalKeyCode::KeyR,
    ] {
        events.extend(key(code));
    }
    advance(&mut runner, Duration::ZERO, &events, viewport())?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&initial));
    assert!(!session(&runner).paused);
    assert!(!session(&runner).cheats);
    assert!(!session(&runner).assisted);
    assert_eq!(session(&runner).percent, 25);
    advance(&mut runner, TICK, &[], viewport())?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&initial));
    let mut events = key(PhysicalKeyCode::KeyR).to_vec();
    events.extend(key(PhysicalKeyCode::KeyN));
    advance(&mut runner, Duration::ZERO, &events, viewport())?;
    assert_eq!(
        snapshot(&session(&runner).game),
        snapshot(&Game::new(SEED + 1))
    );
    Ok(())
}

#[test]
fn debug_is_read_only_including_the_future_random_sequence() -> LogicResult {
    let (mut runner, mut expected) = started()?;
    press(&mut runner, PhysicalKeyCode::F3)?;
    assert!(session(&runner).debug);
    assert!(!session(&runner).assisted);
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    press(&mut runner, PhysicalKeyCode::Space)?;
    expected.expand(25)?;
    for _ in 0..5 {
        advance(&mut runner, TICK * 8, &[], viewport())?;
        for _ in 0..8 {
            expected.tick();
        }
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    }
    press(&mut runner, PhysicalKeyCode::F3)?;
    assert!(!session(&runner).debug);
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    Ok(())
}

#[test]
fn cheats_require_arming_and_assisted_stays_after_disarming() -> LogicResult {
    let (mut runner, expected) = started()?;
    for code in [
        PhysicalKeyCode::F5,
        PhysicalKeyCode::F6,
        PhysicalKeyCode::F8,
    ] {
        press(&mut runner, code)?;
    }
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    assert!(!session(&runner).cheats);
    assert!(!session(&runner).assisted);
    press(&mut runner, PhysicalKeyCode::F4)?;
    assert!(session(&runner).cheats);
    assert!(!session(&runner).assisted);
    press(&mut runner, PhysicalKeyCode::F5)?;
    let granted = snapshot(&session(&runner).game);
    assert!(granted.factions[0].troops > expected.factions()[0].troops);
    assert!(granted.factions[0].troops <= granted.factions[0].land as u32 * 80);
    press(&mut runner, PhysicalKeyCode::F5)?;
    assert_eq!(snapshot(&session(&runner).game), granted);
    press(&mut runner, PhysicalKeyCode::F4)?;
    assert!(!session(&runner).cheats);
    assert!(session(&runner).assisted);
    for code in [
        PhysicalKeyCode::F5,
        PhysicalKeyCode::F6,
        PhysicalKeyCode::F8,
    ] {
        press(&mut runner, code)?;
    }
    assert_eq!(snapshot(&session(&runner).game), granted);
    Ok(())
}

#[test]
fn armed_ai_toggle_and_forced_win_are_explicit_and_terminal() -> LogicResult {
    let (mut runner, _) = started()?;
    press(&mut runner, PhysicalKeyCode::F4)?;
    press(&mut runner, PhysicalKeyCode::F6)?;
    assert!(!session(&runner).game.bots_enabled());
    assert!(session(&runner).assisted);
    press(&mut runner, PhysicalKeyCode::F6)?;
    assert!(session(&runner).game.bots_enabled());
    press(&mut runner, PhysicalKeyCode::F8)?;
    assert_eq!(session(&runner).game.phase(), Phase::Won);
    let won = snapshot(&session(&runner).game);
    advance(&mut runner, TICK * 8, &[], viewport())?;
    assert_eq!(snapshot(&session(&runner).game), won);
    Ok(())
}

#[test]
fn single_step_only_advances_paused_game_once_per_press() -> LogicResult {
    let (mut runner, mut expected) = started()?;
    press(&mut runner, PhysicalKeyCode::F9)?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    press(&mut runner, PhysicalKeyCode::KeyP)?;
    advance(
        &mut runner,
        TICK * 8,
        &[InputEvent::key(PhysicalKeyCode::F9, ButtonState::Pressed)],
        viewport(),
    )?;
    expected.tick();
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    advance(&mut runner, TICK * 8, &[], viewport())?;
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(PhysicalKeyCode::F9, ButtonState::Released)],
        viewport(),
    )?;
    press(&mut runner, PhysicalKeyCode::F9)?;
    expected.tick();
    assert!(session(&runner).paused);
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    Ok(())
}

#[test]
fn slider_release_endpoint_survives_later_motion_and_letterboxing() -> LogicResult {
    let mut runner = build()?;
    let portrait = LogicalViewport::new(720.0, 1000.0)?;
    let x = layout::SLIDER.x;
    let y = layout::SLIDER.y + 20.0;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(x, y, portrait),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            pointer(x + layout::SLIDER.width, y, portrait),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            pointer(x, y, viewport()),
        ],
        viewport(),
    )?;
    assert_eq!(session(&runner).percent, 100);
    press(&mut runner, PhysicalKeyCode::Digit1)?;
    assert_eq!(session(&runner).percent, 10);
    for _ in 0..3 {
        press(&mut runner, PhysicalKeyCode::ArrowLeft)?;
    }
    assert_eq!(session(&runner).percent, 5);
    press(&mut runner, PhysicalKeyCode::Digit5)?;
    press(&mut runner, PhysicalKeyCode::ArrowRight)?;
    assert_eq!(session(&runner).percent, 100);
    Ok(())
}

#[test]
fn resized_views_keep_finite_geometry_assets_and_entity_pool() -> LogicResult {
    let (mut runner, expected) = started()?;
    let rectangle_count = runner.components::<ScreenRectangleVisual>().count();
    let image_count = runner.components::<ScreenImageVisual>().count();
    let generation = runner.world_generation();
    let pixel_bytes = runner.image_pixel_bytes();
    assert!((1..=1024 * 1024).contains(&pixel_bytes));
    for (width, height) in [
        (1.0, 1.0),
        (320.0, 1000.0),
        (2000.0, 220.0),
        (1440.0, 900.0),
    ] {
        let report = advance(
            &mut runner,
            Duration::ZERO,
            &[],
            LogicalViewport::new(width, height)?,
        )?;
        assert_eq!(report.spawned(), 0);
        assert_eq!(report.despawned(), 0);
        assert_eq!(runner.world_generation(), generation);
        assert_eq!(
            runner.components::<ScreenRectangleVisual>().count(),
            rectangle_count
        );
        assert_eq!(
            runner.components::<ScreenImageVisual>().count(),
            image_count
        );
        assert_eq!(runner.image_asset_count(), 1);
        assert_eq!(runner.image_pixel_bytes(), pixel_bytes);
        let frame = runner.extracted_frame().ok_or("frame")?;
        assert!(!frame.resolved_screen_rectangles().is_empty());
        assert!(!frame.resolved_screen_images().is_empty());
        for rectangle in frame.resolved_screen_rectangles() {
            assert!(rectangle.position().is_finite());
            assert!(rectangle.size().is_finite());
            assert!(rectangle.color().is_normalized());
        }
        for image in frame.resolved_screen_images() {
            assert!(image.position().is_finite());
            assert!(image.size().is_finite());
            assert!(image.tint().is_normalized());
        }
        let last_panel = frame.resolved_screen_rectangles().last().unwrap();
        let first_label = frame.resolved_screen_images().first().unwrap();
        assert!(last_panel.draw_order_depth() < first_label.draw_order_depth());
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    }
    Ok(())
}

#[test]
fn exit_precedes_restart_and_cheats() -> LogicResult {
    let (mut runner, expected) = started()?;
    let mut events = key(PhysicalKeyCode::KeyN).to_vec();
    events.extend(key(PhysicalKeyCode::F4));
    events.extend(key(PhysicalKeyCode::Escape));
    let report = advance(&mut runner, Duration::ZERO, &events, viewport())?;
    assert!(report.exit_requested());
    assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    assert!(!session(&runner).cheats);
    Ok(())
}

use crate::game_example::{self, FIXED_STEP, GameAction, game::GameState, render};
use sim_logic::prelude::*;
use std::time::Duration;

fn advance(
    runner: &mut HeadlessRunner<GameAction>,
    delta: Duration,
    events: &[InputEvent],
    width: f32,
    height: f32,
) -> LogicResult<LogicFrameReport> {
    let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
        delta,
        events,
        LogicalViewport::new(width, height)?,
    )) else {
        return Err("Iron Maze frame rejected".into());
    };
    if let Some(error) = report.failure() {
        return Err(format!("Iron Maze frame failed: {error}").into());
    }
    Ok(report)
}

#[test]
fn real_input_moves_turns_fires_and_restart_resets_without_rebuilding_the_screen() -> LogicResult {
    let (application, initial) = game_example::build_application()?;
    let mut runner = application.build_headless(initial)?;
    let generation = runner.world_generation();
    let state = runner.resource::<GameState>().ok_or("game")?.clone();
    let idle = advance(&mut runner, Duration::ZERO, &[], 1280.0, 720.0)?;
    assert_eq!(idle.fixed_ticks_attempted(), 0);
    assert!(
        !runner
            .extracted_frame()
            .ok_or("frame")?
            .resolved_screen_rectangles()
            .is_empty()
    );
    advance(
        &mut runner,
        FIXED_STEP * 2,
        &[
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        1280.0,
        720.0,
    )?;
    let played = runner.resource::<GameState>().ok_or("game")?;
    assert_ne!(played.player.position, state.player.position);
    assert!(played.player.angle > 0.0);
    assert_eq!(played.player.ammo, 59);
    let restarted = advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        1280.0,
        720.0,
    )?;
    assert!(matches!(restarted.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), generation);
    assert_eq!(restarted.spawned(), 0);
    assert_eq!(restarted.despawned(), 0);
    assert_eq!(
        runner.resource::<GameState>().ok_or("game")?,
        &GameState::new()
    );
    let background = runner
        .components::<ScreenRectangleVisual>()
        .map(|(_, rectangle)| rectangle)
        .find(|rectangle| rectangle.draw_order_depth() == 0.0)
        .ok_or("canvas")?;
    assert_eq!(background.size(), LogicalScreenVector::new(1280.0, 720.0));
    advance(&mut runner, FIXED_STEP * 2, &[], 1280.0, 720.0)?;
    assert_eq!(
        runner.resource::<GameState>().ok_or("game")?.player.ammo,
        60
    );
    let exit = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Escape,
            ButtonState::Pressed,
        )],
        1280.0,
        720.0,
    )?;
    assert!(exit.exit_requested());
    Ok(())
}

#[test]
fn quick_click_retains_until_tick_but_catch_up_does_not_replay_shot() -> LogicResult {
    let (application, initial) = game_example::build_application()?;
    let mut runner = application.build_headless(initial)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        800.0,
        600.0,
    )?;
    assert_eq!(
        runner.resource::<GameState>().ok_or("game")?.player.ammo,
        60
    );
    let caught_up = advance(&mut runner, FIXED_STEP * 8, &[], 800.0, 600.0)?;
    assert_eq!(caught_up.fixed_ticks_attempted(), 8);
    assert_eq!(
        runner.resource::<GameState>().ok_or("game")?.player.ammo,
        59
    );
    for _ in 0..8 {
        advance(&mut runner, FIXED_STEP * 8, &[], 800.0, 600.0)?;
    }
    assert_eq!(
        runner.resource::<GameState>().ok_or("game")?.player.ammo,
        59
    );
    Ok(())
}

#[test]
fn resized_and_small_viewports_keep_finite_bounded_geometry_and_reuse_entities() -> LogicResult {
    let (application, initial) = game_example::build_application()?;
    let mut runner = application.build_headless(initial)?;
    let count = runner.components::<ScreenRectangleVisual>().count();
    assert_eq!(count, render::MAX_QUADS);
    for (width, height) in [
        (640.0, 360.0),
        (320.0, 900.0),
        (2000.0, 300.0),
        (1.0, 1.0),
        (1920.0, 1080.0),
    ] {
        let report = advance(&mut runner, FIXED_STEP * 2, &[], width, height)?;
        assert_eq!(report.spawned(), 0);
        assert_eq!(report.despawned(), 0);
        assert_eq!(runner.components::<ScreenRectangleVisual>().count(), count);
        for (_, visual) in runner.components::<ScreenRectangleVisual>() {
            assert!(visual.position().is_finite());
            assert!(visual.size().is_finite());
            assert!(visual.size().to_vec2().x() > 0.0);
            assert!(visual.size().to_vec2().y() > 0.0);
            assert!(visual.color().is_normalized());
        }
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("frame")?
                .resolved_screen_rectangles()
                .len(),
            count
        );
    }
    Ok(())
}

#[test]
fn reset_waits_for_fixed_delivery_discards_quick_shots_and_does_not_repeat() -> LogicResult {
    let (application, initial) = game_example::build_application()?;
    let mut runner = application.build_headless(initial)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        1280.0,
        720.0,
    )?;
    assert_eq!(runner.resource::<GameState>().ok_or("game")?.ticks, 0);
    advance(&mut runner, FIXED_STEP * 8, &[], 1280.0, 720.0)?;
    let state = runner.resource::<GameState>().ok_or("game")?;
    assert_eq!(state.ticks, 7, "one reset, then seven later catch-up ticks");
    assert_eq!(
        state.player.ammo, 60,
        "the pre-reset quick click was consumed"
    );
    advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
        1280.0,
        720.0,
    )?;
    assert_eq!(runner.resource::<GameState>().ok_or("game")?.ticks, 8);
    advance(
        &mut runner,
        FIXED_STEP * 8,
        &[
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        1280.0,
        720.0,
    )?;
    let state = runner.resource::<GameState>().ok_or("game")?;
    assert_eq!(state.ticks, 7);
    assert_eq!(
        state.player.ammo, 59,
        "still-held fire is legitimate in later ticks"
    );
    assert_ne!(state.player.position, GameState::new().player.position);
    Ok(())
}

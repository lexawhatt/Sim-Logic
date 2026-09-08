use std::time::Duration;

use sim_logic::prelude::*;

#[path = "../../examples/click_to_place/game.rs"]
mod game;

use super::support::{FIXED_STEP, advance, sample};

fn runner() -> LogicResult<HeadlessRunner<game::DemoAction>> {
    let (application, initial) = game::build_application(TimeConfig::new(FIXED_STEP, 4)?)?;
    Ok(application.build_headless(initial)?)
}

fn positions(runner: &HeadlessRunner<game::DemoAction>) -> LogicResult<Vec<Vec2>> {
    runner
        .components::<game::PlacedMarker>()
        .map(|(entity, _)| Ok(runner.component::<Transform2d>(entity)?.translation()))
        .collect()
}

#[test]
fn shared_example_places_at_each_click_and_its_viewport_then_clears_and_exits() -> LogicResult {
    let mut runner = runner()?;
    let first = sample(200.0, 150.0, 800.0)?;
    let second = sample(700.0, 450.0, 1_000.0)?;
    let latest = sample(600.0, 100.0, 1_200.0)?;
    let waiting = advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(first),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::pointer_moved(second),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(latest),
        ],
    )?;
    assert_eq!(waiting.spawned(), 0);
    assert!(positions(&runner)?.is_empty());
    let placed = advance(&mut runner, FIXED_STEP * 3, &[])?;
    assert_eq!(placed.fixed_ticks_attempted(), 3);
    assert_eq!(placed.spawned(), 2);
    let positions = positions(&runner)?;
    let camera = Camera2d::new(Vec2::ZERO, 20.0)?;
    assert_eq!(positions.len(), 2);
    assert!(positions.contains(&first.world_position(camera)?));
    assert!(positions.contains(&second.world_position(camera)?));
    assert!(!positions.contains(&latest.world_position(camera)?));
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("extraction")?
            .resolved_circles()
            .len(),
        2
    );

    let cleared = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::mouse_button(
            MouseButton::Right,
            ButtonState::Pressed,
        )],
    )?;
    assert_eq!(cleared.despawned(), 2);
    assert_eq!(runner.components::<game::PlacedMarker>().count(), 0);
    let exited = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Escape,
            ButtonState::Pressed,
        )],
    )?;
    assert!(exited.exit_requested());
    Ok(())
}

#[test]
fn shared_example_bounds_one_batch_and_total_live_markers_and_clear_wins() -> LogicResult {
    let mut runner = runner()?;
    let mut events = vec![InputEvent::pointer_moved(sample(400.0, 300.0, 800.0)?)];
    for _ in 0..game::MAX_MARKERS + 10 {
        events.extend([
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ]);
    }
    let filled = advance(&mut runner, FIXED_STEP, &events)?;
    assert_eq!(filled.spawned(), game::MAX_MARKERS);
    assert_eq!(
        runner.components::<game::PlacedMarker>().count(),
        game::MAX_MARKERS
    );
    let full = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Pressed,
        )],
    )?;
    assert_eq!(full.spawned(), 0);
    assert_eq!(
        runner.components::<game::PlacedMarker>().count(),
        game::MAX_MARKERS
    );
    let cleared = advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Right, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    assert_eq!(cleared.despawned(), game::MAX_MARKERS);
    assert_eq!(cleared.spawned(), 0);
    assert_eq!(runner.components::<game::PlacedMarker>().count(), 0);
    advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    assert_eq!(runner.components::<game::PlacedMarker>().count(), 1);
    Ok(())
}

#[test]
fn shared_example_ignores_clicks_until_a_pointer_sample_is_known() -> LogicResult {
    let mut runner = runner()?;
    let unknown = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::mouse_button(
            MouseButton::Left,
            ButtonState::Pressed,
        )],
    )?;
    assert_eq!(unknown.spawned(), 0);
    advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::pointer_moved(sample(400.0, 300.0, 800.0)?),
            InputEvent::PointerLeft,
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    assert_eq!(runner.components::<game::PlacedMarker>().count(), 0);
    Ok(())
}

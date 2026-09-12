//! Headless playback of the actual menu and mouse-ownership systems.

use super::{app, model, scene, view};
use sim_logic::prelude::*;
use std::time::Duration;

#[path = "menu_clicks.rs"]
mod menu_clicks;

fn viewport() -> LogicResult<LogicalViewport> {
    Ok(LogicalViewport::new(1100.0, 720.0)?)
}

fn key(key: PhysicalKeyCode) -> [InputEvent; 2] {
    [
        InputEvent::key(key, ButtonState::Pressed),
        InputEvent::key(key, ButtonState::Released),
    ]
}

fn click(point: [f32; 2]) -> LogicResult<[InputEvent; 3]> {
    Ok([
        InputEvent::pointer_moved(PointerSample::new(
            LogicalScreenPosition::new(point[0], point[1]),
            viewport()?,
        )?),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
    ])
}

fn advance(runner: &mut HeadlessRunner<app::Action>, events: &[InputEvent]) -> LogicResult {
    advance_for(runner, Duration::ZERO, events)
}

fn advance_for(
    runner: &mut HeadlessRunner<app::Action>,
    elapsed: Duration,
    events: &[InputEvent],
) -> LogicResult {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(elapsed, events, viewport()?))
    else {
        return Err("frame rejected".into());
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    Ok(())
}

fn ready() -> LogicResult<HeadlessRunner<app::Action>> {
    let (app, initial) =
        app::build_application(Default::default(), "unused-interface-v2.save".into())?;
    let mut runner = app.build_headless(initial)?;
    for _ in 0..100 {
        advance(&mut runner, &[])?;
        if runner.resource::<scene::Local>().ok_or("local")?.phase == scene::Phase::Ready {
            return Ok(runner);
        }
    }
    Err("streaming failed to become ready within its budget".into())
}

fn session(runner: &HeadlessRunner<app::Action>) -> &app::Session {
    runner
        .app_resource::<app::Session>()
        .expect("session fixture")
}

#[test]
fn capture_click_never_breaks_and_relative_motion_is_frame_only() -> LogicResult {
    let mut runner = ready()?;
    let old = session(&runner).game.player;
    advance(&mut runner, &click([550.0, 360.0])?)?;
    assert_eq!(session(&runner).edits, 0);
    assert!(
        runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    advance(
        &mut runner,
        &[InputEvent::relative_pointer_motion(
            RelativePointerMotion::new(40.0, -20.0)?,
        )],
    )?;
    let turned = session(&runner).game.player;
    assert!((turned.yaw - old.yaw - 0.1).abs() < 0.0001);
    assert!((turned.pitch - old.pitch - 0.05).abs() < 0.0001);
    advance(&mut runner, &[])?;
    assert_eq!(session(&runner).game.player, turned);
    Ok(())
}

#[test]
fn creative_menu_assigns_the_selected_hotbar_slot_without_editing_or_saving() -> LogicResult {
    let mut runner = ready()?;
    assert!(session(&runner).game.creative);
    advance(&mut runner, &key(PhysicalKeyCode::Digit9))?;
    assert_eq!(session(&runner).game.inventory.selected_slot(), 8);
    advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
    assert!(runner.is_paused());
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Creative
    );
    assert!(
        !runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    let index = 7;
    assert_ne!(
        session(&runner).game.inventory.hotbar()[8],
        model::Block::SOLID[index]
    );
    let [x, y, w, h] = view::Layout::new(viewport()?)
        .panel(view::Panel::Button(view::Button::CreativeBlock(index)));
    advance(&mut runner, &click([x + w * 0.5, y + h * 0.5])?)?;
    assert_eq!(
        session(&runner).game.inventory.hotbar()[8],
        model::Block::SOLID[index]
    );
    assert_eq!(session(&runner).edits, 0);
    assert!(!session(&runner).notice.starts_with("Saved"));
    advance(&mut runner, &key(PhysicalKeyCode::F5))?;
    assert!(!runner.is_paused());
    assert!(
        runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    Ok(())
}

#[test]
fn double_space_enables_flight_and_pause_cancels_vertical_motion() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &key(PhysicalKeyCode::Space))?;
    advance_for(
        &mut runner,
        Duration::from_millis(100),
        &key(PhysicalKeyCode::Space),
    )?;
    assert!(runner.resource::<scene::Local>().ok_or("local")?.flying);
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    let low = session(&runner).game.player.position[1];
    advance_for(&mut runner, Duration::from_millis(100), &[])?;
    assert!(session(&runner).game.player.position[1] > low + 0.3);
    advance(
        &mut runner,
        &[
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::ShiftLeft, ButtonState::Pressed),
        ],
    )?;
    let high = session(&runner).game.player.position[1];
    advance_for(&mut runner, Duration::from_millis(50), &[])?;
    assert!(session(&runner).game.player.position[1] < high);
    advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
    let paused = session(&runner).game.player;
    advance_for(&mut runner, Duration::from_millis(100), &[])?;
    assert_eq!(session(&runner).game.player, paused);
    assert_eq!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .flight_vertical,
        0.0
    );
    Ok(())
}

#[test]
fn focus_loss_without_held_buttons_opens_pause_and_discards_motion() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &click([550.0, 360.0])?)?;
    let old = session(&runner).game.player;
    advance(
        &mut runner,
        &[
            InputEvent::relative_pointer_motion(RelativePointerMotion::new(400.0, 200.0)?),
            InputEvent::FocusLost,
        ],
    )?;
    assert!(runner.is_paused());
    assert_eq!(session(&runner).game.player.yaw, old.yaw);
    assert_eq!(session(&runner).game.player.pitch, old.pitch);
    assert!(
        !runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Pause
    );
    advance(&mut runner, &[])?;
    assert!(
        !runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    Ok(())
}

#[test]
fn opening_then_closing_a_menu_in_one_frame_does_not_apply_old_motion() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &click([550.0, 360.0])?)?;
    let old = session(&runner).game.player;
    let mut events = vec![InputEvent::relative_pointer_motion(
        RelativePointerMotion::new(40.0, 20.0)?,
    )];
    events.extend(key(PhysicalKeyCode::F5));
    events.extend(key(PhysicalKeyCode::F5));
    advance(&mut runner, &events)?;
    assert!(!runner.is_paused());
    assert_eq!(session(&runner).game.player.yaw, old.yaw);
    assert_eq!(session(&runner).game.player.pitch, old.pitch);
    advance(&mut runner, &key(PhysicalKeyCode::F3))?;
    assert!(runner.resource::<scene::Local>().ok_or("local")?.debug);
    let debug_text: Vec<_> = runner
        .components::<view::Label>()
        .filter_map(|(entity, label)| matches!(label, view::Label::Debug(_)).then_some(entity))
        .map(|entity| {
            runner
                .component::<ScreenTextVisual>(entity)
                .map(|visual| visual.text())
        })
        .collect::<Result<_, _>>()?;
    assert_eq!(debug_text.len(), view::DEBUG_LINES);
    assert!(debug_text.iter().any(|text| text.starts_with("Target:")));
    assert!(debug_text.iter().any(|text| text.starts_with("Mouse:")));
    assert!(
        debug_text
            .iter()
            .any(|text| text.starts_with("Host frame interval:"))
    );
    assert_eq!(session(&runner).edits, 0);
    Ok(())
}

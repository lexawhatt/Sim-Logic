//! Real clicks normally span frames; pausing gameplay must not cancel UI holds.

use super::*;

const FRAME: Duration = Duration::from_millis(16);

fn button_click(button: view::Button) -> LogicResult<[InputEvent; 3]> {
    let [x, y, width, height] = view::Layout::new(viewport()?).panel(view::Panel::Button(button));
    click([x + width * 0.5, y + height * 0.5])
}

fn multi_frame_click(
    runner: &mut HeadlessRunner<app::Action>,
    button: view::Button,
) -> LogicResult {
    let events = button_click(button)?;
    advance_for(runner, FRAME, &events[..2])?;
    for _ in 0..3 {
        assert_eq!(
            runner
                .resource::<scene::Local>()
                .ok_or("local")?
                .pointer
                .captured(),
            Some(button),
            "an unchanged menu must retain the pressed target until release"
        );
        advance_for(runner, FRAME, &[])?;
    }
    advance_for(runner, FRAME, &events[2..])?;
    assert!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .pointer
            .captured()
            .is_none()
    );
    Ok(())
}

#[test]
fn every_palette_block_can_replace_a_different_block_with_a_multi_frame_click() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &key(PhysicalKeyCode::Digit9))?;
    advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
    let stopped = session(&runner).game.player;
    for (index, block) in model::Block::SOLID.into_iter().enumerate() {
        assert_ne!(session(&runner).game.inventory.selected(), block);
        multi_frame_click(&mut runner, view::Button::CreativeBlock(index))?;
        assert_eq!(session(&runner).game.inventory.hotbar()[8], block);
        assert_eq!(session(&runner).game.player, stopped);
        assert_eq!(session(&runner).edits, 0);
        assert!(runner.is_paused());
        assert!(session(&runner).notice.starts_with(block.name()));
        let icon = runner
            .components::<view::Panel>()
            .find_map(|(entity, panel)| {
                matches!(panel, view::Panel::Icon(view::Button::Slot(8), 0)).then_some(entity)
            })
            .ok_or("hotbar icon")?;
        assert_eq!(
            runner.component::<ScreenRectangleVisual>(icon)?.color(),
            block.color(1)
        );
    }
    Ok(())
}

#[test]
fn paused_buttons_hotbar_and_close_accept_multi_frame_clicks() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &key(PhysicalKeyCode::Escape))?;
    multi_frame_click(&mut runner, view::Button::Creative)?;
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Creative
    );
    multi_frame_click(&mut runner, view::Button::Slot(6))?;
    assert_eq!(session(&runner).game.inventory.selected_slot(), 6);
    multi_frame_click(&mut runner, view::Button::Close)?;
    assert!(!runner.is_paused());
    assert!(
        runner
            .app_resource::<PointerCapture>()
            .ok_or("capture")?
            .requested()
    );
    assert_eq!(session(&runner).edits, 0);
    advance(&mut runner, &key(PhysicalKeyCode::Escape))?;
    multi_frame_click(&mut runner, view::Button::Pause)?;
    assert!(!runner.is_paused());
    assert_eq!(session(&runner).edits, 0);
    Ok(())
}

#[test]
fn external_pause_cancels_a_pending_hotbar_gesture() -> LogicResult {
    let mut runner = ready()?;
    let original = session(&runner).game.inventory.selected_slot();
    assert_ne!(original, 6);
    let events = button_click(view::Button::Slot(6))?;
    advance_for(&mut runner, FRAME, &events[..2])?;
    assert_eq!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .pointer
            .captured(),
        Some(view::Button::Slot(6))
    );
    runner.set_paused(true);
    advance_for(&mut runner, FRAME, &[])?;
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Pause
    );
    assert!(
        runner
            .resource::<scene::Local>()
            .ok_or("local")?
            .pointer
            .captured()
            .is_none()
    );
    advance_for(&mut runner, FRAME, &events[2..])?;
    assert_eq!(session(&runner).game.inventory.selected_slot(), original);
    multi_frame_click(&mut runner, view::Button::Creative)?;
    multi_frame_click(&mut runner, view::Button::Slot(6))?;
    assert_eq!(session(&runner).game.inventory.selected_slot(), 6);
    assert_eq!(session(&runner).edits, 0);
    Ok(())
}

#[test]
fn first_fresh_click_after_focus_loss_is_not_swallowed() -> LogicResult {
    let mut runner = ready()?;
    advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
    let original = session(&runner).game.inventory.hotbar();
    let events = button_click(view::Button::CreativeBlock(7))?;
    advance_for(&mut runner, FRAME, &events[..2])?;
    advance_for(&mut runner, FRAME, &[InputEvent::FocusLost])?;
    assert_eq!(session(&runner).game.inventory.hotbar(), original);
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Pause
    );
    // FocusLost already synthesized the old release. Do not invent another
    // physical release before the first fresh click on the pause menu.
    advance_for(&mut runner, FRAME, &[])?;
    advance(&mut runner, &button_click(view::Button::Creative)?)?;
    assert_eq!(
        runner.resource::<scene::Local>().ok_or("local")?.menu,
        view::Menu::Creative
    );
    multi_frame_click(&mut runner, view::Button::CreativeBlock(7))?;
    assert_eq!(
        session(&runner).game.inventory.selected(),
        model::Block::SOLID[7]
    );
    assert_eq!(session(&runner).edits, 0);
    Ok(())
}

#[test]
fn cancelled_or_retargeted_multi_frame_clicks_do_not_assign_blocks() -> LogicResult {
    for boundary in 0..4 {
        let mut runner = ready()?;
        advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
        let original = session(&runner).game.inventory.hotbar();
        let events = button_click(view::Button::CreativeBlock(7))?;
        advance_for(&mut runner, FRAME, &events[..2])?;
        match boundary {
            0 => advance_for(&mut runner, FRAME, &[InputEvent::PointerLeft])?,
            1 => advance_for(&mut runner, FRAME, &[InputEvent::FocusLost])?,
            2 => {
                // The same target reappearing must not revive the old press.
                advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
                advance(&mut runner, &key(PhysicalKeyCode::KeyE))?;
            }
            _ => {
                let other = button_click(view::Button::CreativeBlock(8))?;
                advance_for(&mut runner, FRAME, &other[..1])?;
            }
        }
        advance_for(&mut runner, FRAME, &events[2..])?;
        assert_eq!(session(&runner).game.inventory.hotbar(), original);
        assert_eq!(session(&runner).edits, 0);
        assert!(
            runner
                .resource::<scene::Local>()
                .ok_or("local")?
                .pointer
                .captured()
                .is_none()
        );
    }
    Ok(())
}

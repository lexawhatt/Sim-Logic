use std::{collections::HashSet, time::Duration};

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn unknown_mouse_coordinates_do_not_make_mouse_edges_keyboard_or_cancelled() -> LogicResult {
    let mut runner = runner(16, false)?;
    advance(
        &mut runner,
        STEP,
        &[
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Released),
        ],
    )?;
    let frame = last_frame(&runner)?;
    assert!(frame.held, "keyboard still holds the shared action");
    assert_eq!(frame.presses.len(), 2);
    assert_eq!(
        frame.presses[0].control(),
        InputControl::Key(PhysicalKeyCode::Space)
    );
    assert_eq!(
        frame.presses[1].control(),
        InputControl::MouseButton(MouseButton::Left)
    );
    assert_eq!(
        frame.releases[0].control(),
        InputControl::MouseButton(MouseButton::Left)
    );
    for edge in probe(&runner)?.frame_edges.last().ok_or("edges")? {
        assert_eq!(edge.pointer(), None);
        assert_eq!(edge.cancellation_reason(), None);
        assert!(!edge.is_cancelled());
    }
    Ok(())
}

#[test]
fn genuine_releases_pointer_leave_and_focus_loss_have_distinct_provenance() -> LogicResult {
    let mut runner = runner(32, false)?;
    let position = sample(100.0)?;
    advance(
        &mut runner,
        STEP,
        &[
            InputEvent::pointer_moved(position),
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Released),
            mouse(MouseButton::Left, ButtonState::Pressed),
            InputEvent::PointerLeft,
            mouse(MouseButton::Right, ButtonState::Pressed),
            InputEvent::FocusLost,
        ],
    )?;
    let frame = last_frame(&runner)?;
    assert!(!frame.held);
    assert_eq!(frame.pointer, None);
    assert_eq!(frame.releases.len(), 4);
    assert_eq!(frame.releases[0].pointer(), Some(position));
    assert_eq!(frame.releases[0].cancellation_reason(), None);
    assert_eq!(
        frame.releases[1].cancellation_reason(),
        Some(InputCancellationReason::PointerLeft)
    );
    assert_eq!(
        frame.releases[2].control(),
        InputControl::Key(PhysicalKeyCode::Space)
    );
    assert_eq!(
        frame.releases[3].control(),
        InputControl::MouseButton(MouseButton::Right)
    );
    for edge in &frame.releases[2..] {
        assert_eq!(
            edge.cancellation_reason(),
            Some(InputCancellationReason::FocusLost)
        );
        assert!(edge.is_cancelled());
        assert_eq!(edge.pointer(), None);
    }
    assert!(frame.presses.iter().all(|edge| !edge.is_cancelled()));
    Ok(())
}

#[test]
fn one_focus_loss_releases_all_28_controls_once_in_catalog_order() -> LogicResult {
    let mut runner = runner(64, true)?;
    let mut presses = all_presses();
    presses.reverse();
    advance(&mut runner, STEP, &presses)?;
    assert!(last_frame(&runner)?.held);
    advance(
        &mut runner,
        STEP,
        &[InputEvent::FocusLost, InputEvent::FocusLost],
    )?;
    let released = &last_frame(&runner)?.releases;
    assert_eq!(released.len(), 28);
    assert_eq!(
        released
            .iter()
            .map(|edge| edge.control())
            .collect::<Vec<_>>(),
        controls()
    );
    assert_eq!(
        released
            .iter()
            .map(|edge| edge.intent())
            .collect::<HashSet<_>>()
            .len(),
        28
    );
    assert!(
        released
            .iter()
            .all(|edge| edge.state() == ButtonState::Released
                && edge.is_cancelled()
                && edge.cancellation_reason() == Some(InputCancellationReason::FocusLost)
                && edge.pointer().is_none())
    );
    assert!(!last_frame(&runner)?.held);
    Ok(())
}

#[test]
fn repeated_loss_and_late_physical_releases_do_not_generate_duplicates() -> LogicResult {
    let mut runner = runner(16, false)?;
    advance(
        &mut runner,
        STEP,
        &[
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    advance(&mut runner, STEP, &[InputEvent::FocusLost])?;
    let cancelled = last_frame(&runner)?.releases.clone();
    advance(
        &mut runner,
        STEP,
        &[
            InputEvent::FocusLost,
            key(PhysicalKeyCode::Space, ButtonState::Released),
            mouse(MouseButton::Left, ButtonState::Released),
            InputEvent::PointerLeft,
        ],
    )?;
    assert!(last_frame(&runner)?.releases.is_empty());
    assert!(!last_frame(&runner)?.held);
    advance(
        &mut runner,
        STEP,
        &[key(PhysicalKeyCode::Space, ButtonState::Pressed)],
    )?;
    let fresh = last_frame(&runner)?.presses[0];
    assert!(!fresh.is_cancelled());
    assert_eq!(fresh.control(), InputControl::Key(PhysicalKeyCode::Space));
    assert!(cancelled.iter().all(|edge| edge.intent() != fresh.intent()));
    Ok(())
}

#[test]
fn focus_loss_occurs_at_its_event_position_and_does_not_latch_a_focus_gate() -> LogicResult {
    let mut runner = runner(16, false)?;
    let first = sample(20.0)?;
    let next = sample(80.0)?;
    advance(
        &mut runner,
        STEP,
        &[
            InputEvent::pointer_moved(first),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::FocusLost,
            InputEvent::pointer_moved(next),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    let frame = last_frame(&runner)?;
    assert!(frame.held);
    assert_eq!(frame.pointer, Some(next));
    let edges = probe(&runner)?.frame_edges.last().ok_or("edges")?;
    assert_eq!(edges.len(), 2);
    assert!(edges[0].is_cancelled());
    assert_eq!(edges[0].pointer(), None);
    assert_eq!(edges[1].state(), ButtonState::Pressed);
    assert!(!edges[1].is_cancelled());
    assert_eq!(edges[1].pointer(), Some(next));
    Ok(())
}

#[test]
fn pointer_leave_cancels_only_mouse_while_keyboard_keeps_shared_action_held() -> LogicResult {
    let mut runner = runner(16, false)?;
    advance(
        &mut runner,
        STEP,
        &[
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
            mouse(MouseButton::Right, ButtonState::Pressed),
        ],
    )?;
    advance(&mut runner, STEP, &[InputEvent::PointerLeft])?;
    let frame = last_frame(&runner)?;
    assert!(frame.held);
    assert_eq!(frame.releases.len(), 2);
    assert!(
        frame
            .releases
            .iter()
            .all(|edge| edge.cancellation_reason() == Some(InputCancellationReason::PointerLeft))
    );
    advance(&mut runner, STEP, &[InputEvent::FocusLost])?;
    let frame = last_frame(&runner)?;
    assert!(!frame.held);
    assert_eq!(frame.releases.len(), 1);
    assert_eq!(
        frame.releases[0].control(),
        InputControl::Key(PhysicalKeyCode::Space)
    );
    assert_eq!(
        frame.releases[0].cancellation_reason(),
        Some(InputCancellationReason::FocusLost)
    );
    Ok(())
}

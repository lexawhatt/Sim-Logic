use std::time::Duration;

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn one_focus_loss_expansion_is_rejected_atomically_even_while_paused() -> LogicResult {
    for paused in [false, true] {
        let mut runner = runner(27, true)?;
        runner.set_paused(true);
        let position = sample(40.0)?;
        advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::pointer_moved(position)],
        )?;
        let presses = all_presses();
        advance(&mut runner, Duration::ZERO, &presses[..27])?;
        advance(&mut runner, Duration::ZERO, &presses[27..])?;
        runner.set_paused(paused);
        let before = last_frame(&runner)?.clone();
        let frame_count = probe(&runner)?.frames.len();
        let generation = runner.world_generation();

        assert!(matches!(
            runner.advance_frame(request(STEP * 3, &[InputEvent::FocusLost])?),
            FrameOutcome::Rejected(BeginFrameRejection::Input(
                InputCollectionError::FrameEdgeLimitExceeded {
                    limit: 27,
                    incoming: 28
                }
            ))
        ));
        assert_eq!(probe(&runner)?.frames.len(), frame_count);
        assert_eq!(last_frame(&runner)?, &before);
        assert_eq!(runner.world_generation(), generation);
        let empty = advance(&mut runner, Duration::ZERO, &[])?;
        assert_eq!(
            empty.fixed_ticks_attempted(),
            0,
            "rejected wall time must not accumulate"
        );
        assert!(last_frame(&runner)?.held);
        assert_eq!(last_frame(&runner)?.pointer, Some(position));

        // A physical release makes the complete cancellation fit. The rejected
        // attempt must not have silently cleared the other 27 held controls.
        advance(
            &mut runner,
            STEP,
            &[mouse(MouseButton::Middle, ButtonState::Released)],
        )?;
        assert_eq!(last_frame(&runner)?.releases.len(), 1);
        advance(&mut runner, STEP, &[InputEvent::FocusLost])?;
        let frame = last_frame(&runner)?;
        assert_eq!(frame.releases.len(), 27);
        assert!(frame.releases.iter().all(|edge| edge.is_cancelled()));
        assert!(!frame.held);
        assert_eq!(frame.pointer, None);
    }
    Ok(())
}

#[test]
fn retained_fixed_limit_preserves_pointer_pending_edges_and_time_on_rejection() -> LogicResult {
    let mut runner = runner(4, false)?;
    let old_pointer = sample(10.0)?;
    let new_pointer = sample(90.0)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::pointer_moved(old_pointer),
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[key(PhysicalKeyCode::KeyW, ButtonState::Pressed)],
    )?;
    let before = last_frame(&runner)?.clone();
    let frame_count = probe(&runner)?.frames.len();
    assert!(probe(&runner)?.fixed.is_empty());

    assert!(matches!(
        runner.advance_frame(request(
            STEP * 3,
            &[
                InputEvent::pointer_moved(new_pointer),
                InputEvent::FocusLost,
            ]
        )?),
        FrameOutcome::Rejected(BeginFrameRejection::Input(
            InputCollectionError::RetainedFixedEdgeLimitExceeded {
                limit: 4,
                retained: 3,
                incoming: 3,
            }
        ))
    ));
    assert_eq!(probe(&runner)?.frames.len(), frame_count);
    assert_eq!(last_frame(&runner)?, &before);
    let empty = advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(empty.fixed_ticks_attempted(), 0);
    assert!(last_frame(&runner)?.held);
    assert_eq!(last_frame(&runner)?.pointer, Some(old_pointer));

    advance(&mut runner, STEP, &[])?;
    let fixed = probe(&runner)?.fixed.last().ok_or("fixed observation")?;
    assert_eq!(fixed.presses.len(), 3);
    assert!(fixed.releases.is_empty());
    assert_eq!(fixed.pointer, Some(old_pointer));
    assert!(fixed.held);

    advance(&mut runner, STEP, &[InputEvent::FocusLost])?;
    let frame = last_frame(&runner)?;
    assert_eq!(frame.releases.len(), 3);
    assert!(frame.releases.iter().all(|edge| edge.is_cancelled()));
    assert!(!frame.held);
    assert_eq!(frame.pointer, None);
    Ok(())
}

#[test]
fn fresh_press_after_loss_counts_toward_generated_edge_budget() -> LogicResult {
    let mut runner = runner(2, false)?;
    runner.set_paused(true);
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
    )?;
    let before = last_frame(&runner)?.clone();
    assert!(matches!(
        runner.advance_frame(request(
            Duration::ZERO,
            &[
                InputEvent::FocusLost,
                key(PhysicalKeyCode::Space, ButtonState::Pressed),
            ]
        )?),
        FrameOutcome::Rejected(BeginFrameRejection::Input(
            InputCollectionError::FrameEdgeLimitExceeded {
                limit: 2,
                incoming: 3
            }
        ))
    ));
    assert_eq!(last_frame(&runner)?, &before);
    advance(&mut runner, Duration::ZERO, &[InputEvent::FocusLost])?;
    assert_eq!(last_frame(&runner)?.releases.len(), 2);
    assert!(!last_frame(&runner)?.held);
    Ok(())
}

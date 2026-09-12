//! Input-to-camera acceptance: navigation changes presentation, never game rules.

use super::*;
use territory_wars::navigation::MapView;

fn center() -> (f32, f32) {
    (
        layout::MAP.x + layout::MAP.width * 0.5,
        layout::MAP.y + layout::MAP.height * 0.5,
    )
}

fn unzoomed_cell(x: f32, y: f32) -> Option<usize> {
    if !layout::MAP.contains(x, y) {
        return None;
    }
    let column = ((x - layout::MAP_X) / layout::CELL).floor() as usize;
    let row = ((y - layout::MAP_Y) / layout::CELL).floor() as usize;
    Some(row * WIDTH + column)
}

fn wheel(delta: ScrollDelta) -> InputEvent {
    InputEvent::mouse_wheel(delta)
}

fn middle(state: ButtonState) -> InputEvent {
    InputEvent::mouse_button(MouseButton::Middle, state)
}

fn send(runner: &mut HeadlessRunner<Action>, events: &[InputEvent]) -> LogicResult {
    advance(runner, Duration::ZERO, events, viewport())?;
    Ok(())
}

fn zoom(runner: &mut HeadlessRunner<Action>, steps: f64) -> LogicResult {
    let (x, y) = center();
    send(
        runner,
        &[
            pointer(x, y, viewport()),
            wheel(ScrollDelta::lines(0.0, steps)?),
        ],
    )
}

fn near(actual: (f32, f32), expected: (f32, f32)) {
    assert!(
        (actual.0 - expected.0).abs() < 0.002,
        "horizontal: {actual:?} != {expected:?}"
    );
    assert!(
        (actual.1 - expected.1).abs() < 0.002,
        "vertical: {actual:?} != {expected:?}"
    );
}

#[test]
fn zoom_keeps_cursor_anchor_and_v_resets_without_restarting_held_drag() -> LogicResult {
    let mut runner = build()?;
    let before = snapshot(&session(&runner).game);
    let anchor = (layout::MAP.x + 250.0, layout::MAP.y + 220.0);
    send(
        &mut runner,
        &[
            pointer(anchor.0, anchor.1, viewport()),
            wheel(ScrollDelta::lines(0.0, 4.0)?),
        ],
    )?;
    let view = session(&runner).map_view;
    assert!((view.zoom() - 1.2_f32.powi(4)).abs() < 0.000_01);
    near(view.project(anchor.0, anchor.1), anchor);
    assert_eq!(
        view.cell_at(anchor.0, anchor.1),
        unzoomed_cell(anchor.0, anchor.1)
    );
    send(&mut runner, &[middle(ButtonState::Pressed)])?;
    press(&mut runner, PhysicalKeyCode::KeyV)?;
    assert_eq!(session(&runner).map_view, MapView::default());
    send(
        &mut runner,
        &[pointer(anchor.0 + 90.0, anchor.1 + 60.0, viewport())],
    )?;
    assert_eq!(
        session(&runner).map_view,
        MapView::default(),
        "reset must cancel the existing gesture"
    );
    // At 1x, bounds alone would hide an accidentally retained drag. Zoom back
    // in without a new MMB press before checking another pointer displacement.
    zoom(&mut runner, 4.0)?;
    let zoomed_again = session(&runner).map_view;
    assert!(zoomed_again.zoom() > 2.0);
    let (x, y) = center();
    send(&mut runner, &[pointer(x + 45.0, y + 30.0, viewport())])?;
    assert_eq!(
        session(&runner).map_view,
        zoomed_again,
        "zooming again must not revive the pre-reset held drag"
    );
    assert_eq!(snapshot(&session(&runner).game), before);
    Ok(())
}

#[test]
fn fractional_lines_and_logical_pixels_match_without_integer_rounding() -> LogicResult {
    let mut lines = build()?;
    let mut pixels = build()?;
    let (x, y) = center();
    for amount in [0.25, 1.5, -0.125] {
        send(
            &mut lines,
            &[
                pointer(x, y, viewport()),
                wheel(ScrollDelta::lines(0.0, amount)?),
            ],
        )?;
        send(
            &mut pixels,
            &[
                pointer(x, y, viewport()),
                wheel(ScrollDelta::pixels(0.0, amount * 48.0)?),
            ],
        )?;
        assert_eq!(session(&lines).map_view, session(&pixels).map_view);
    }
    assert!(session(&lines).map_view.zoom() > 1.0);
    let before = session(&lines).map_view;
    send(&mut lines, &[wheel(ScrollDelta::lines(3.0, 0.0)?)])?;
    assert_eq!(
        session(&lines).map_view,
        before,
        "horizontal wheel is not vertical zoom"
    );
    Ok(())
}

#[test]
fn wheel_before_click_and_click_before_wheel_choose_different_correct_cells() -> LogicResult {
    let initial = Game::new(SEED);
    let (cx, cy) = center();
    // Independent 8x centered inverse, not the MapView picking implementation.
    let (unzoomed, zoomed) = initial
        .owners()
        .iter()
        .enumerate()
        .find_map(|(cell, &owner)| {
            let (x, y) = layout::cell_center(cell);
            let transformed = unzoomed_cell(cx + (x - cx) / 8.0, cy + (y - cy) / 8.0)?;
            (owner == NEUTRAL && initial.owners()[transformed] == NEUTRAL && cell != transformed)
                .then_some((cell, transformed))
        })
        .ok_or("expected distinct visible land under centered zoom")?;
    let (x, y) = layout::cell_center(unzoomed);
    for wheel_first in [false, true] {
        let mut runner = build()?;
        let scrolling = [
            pointer(cx, cy, viewport()),
            wheel(ScrollDelta::lines(0.0, 32.0)?),
        ];
        let clicking = click(x, y, viewport());
        let events: Vec<_> = if wheel_first {
            scrolling.into_iter().chain(clicking).collect()
        } else {
            clicking.into_iter().chain(scrolling).collect()
        };
        send(&mut runner, &events)?;
        let expected_home = if wheel_first { zoomed } else { unzoomed };
        let mut expected = Game::new(SEED);
        assert!(expected.start(expected_home));
        assert_eq!(session(&runner).game.factions()[0].capital, expected_home);
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    }
    Ok(())
}

#[test]
fn complete_single_frame_middle_drag_uses_release_not_later_pointer() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, 4.0)?;
    let (x, y) = center();
    send(
        &mut runner,
        &[
            pointer(x, y, viewport()),
            middle(ButtonState::Pressed),
            pointer(x + 80.0, y - 50.0, viewport()),
            middle(ButtonState::Released),
            pointer(x - 130.0, y + 120.0, viewport()),
        ],
    )?;
    near(
        session(&runner).map_view.project(x, y),
        (x + 80.0, y - 50.0),
    );
    assert_eq!(session(&runner).game.phase(), Phase::Choosing);
    Ok(())
}

#[test]
fn cross_frame_middle_drag_retains_anchor_and_stops_after_release() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, 4.0)?;
    let (x, y) = center();
    send(
        &mut runner,
        &[pointer(x, y, viewport()), middle(ButtonState::Pressed)],
    )?;
    for (dx, dy) in [(20.0, 10.0), (55.0, 25.0), (90.0, 40.0)] {
        send(&mut runner, &[pointer(x + dx, y + dy, viewport())])?;
        near(session(&runner).map_view.project(x, y), (x + dx, y + dy));
        send(&mut runner, &[])?;
        near(session(&runner).map_view.project(x, y), (x + dx, y + dy));
    }
    send(&mut runner, &[middle(ButtonState::Released)])?;
    let released = session(&runner).map_view;
    send(&mut runner, &[pointer(x, y, viewport())])?;
    assert_eq!(session(&runner).map_view, released);
    Ok(())
}

#[test]
fn wheel_inside_middle_drag_keeps_motion_before_and_after_zoom_ordered() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, 4.0)?;
    let (x, y) = center();
    send(
        &mut runner,
        &[
            pointer(x, y, viewport()),
            middle(ButtonState::Pressed),
            pointer(x + 40.0, y, viewport()),
            wheel(ScrollDelta::lines(0.0, 2.0)?),
            pointer(x + 70.0, y - 20.0, viewport()),
            middle(ButtonState::Released),
        ],
    )?;
    // Original center stays under the grabbed cursor throughout the wheel.
    near(
        session(&runner).map_view.project(x, y),
        (x + 70.0, y - 20.0),
    );
    assert!((session(&runner).map_view.zoom() - 1.2_f32.powi(6)).abs() < 0.000_01);
    Ok(())
}

#[test]
fn held_middle_drag_blocks_start_attack_and_all_hud_clicks() -> LogicResult {
    for running in [false, true] {
        let mut runner = if running { started()?.0 } else { build()? };
        zoom(&mut runner, 4.0)?;
        let before = snapshot(&session(&runner).game);
        let before_percent = session(&runner).percent;
        let (x, y) = center();
        send(
            &mut runner,
            &[pointer(x, y, viewport()), middle(ButtonState::Pressed)],
        )?;
        for area in [
            layout::MAP,
            layout::SLIDER,
            layout::EXPAND,
            layout::NEW_MAP,
            layout::PAUSE,
            layout::DEBUG,
        ] {
            send(
                &mut runner,
                &click(
                    area.x + area.width * 0.5,
                    area.y + area.height * 0.5,
                    viewport(),
                ),
            )?;
            assert_eq!(snapshot(&session(&runner).game), before);
            assert_eq!(session(&runner).percent, before_percent);
            assert!(!session(&runner).paused);
            assert!(!session(&runner).debug);
        }
        send(&mut runner, &[middle(ButtonState::Released)])?;
    }
    Ok(())
}

#[test]
fn middle_gesture_started_on_hud_cannot_capture_map_later() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, 4.0)?;
    let view = session(&runner).map_view;
    let (x, y) = center();
    send(
        &mut runner,
        &[
            pointer(layout::SLIDER.x + 10.0, layout::SLIDER.y + 10.0, viewport()),
            middle(ButtonState::Pressed),
            pointer(x, y, viewport()),
        ],
    )?;
    send(
        &mut runner,
        &[
            pointer(x + 100.0, y + 80.0, viewport()),
            middle(ButtonState::Released),
        ],
    )?;
    assert_eq!(session(&runner).map_view, view);
    assert_eq!(session(&runner).percent, 25);
    Ok(())
}

#[test]
fn wheels_over_hud_letterbox_or_missing_pointer_do_not_zoom() -> LogicResult {
    let mut runner = build()?;
    let scrolling = wheel(ScrollDelta::lines(0.0, 3.0)?);
    send(&mut runner, &[scrolling])?;
    for area in [
        layout::SLIDER,
        layout::SIDEBAR,
        layout::NEW_MAP,
        layout::PAUSE,
    ] {
        send(
            &mut runner,
            &[pointer(area.x + 5.0, area.y + 5.0, viewport()), scrolling],
        )?;
        assert_eq!(session(&runner).map_view, MapView::default());
    }
    let portrait = LogicalViewport::new(400.0, 1000.0)?;
    let letterbox = PointerSample::new(LogicalScreenPosition::new(200.0, 2.0), portrait)?;
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::pointer_moved(letterbox), scrolling],
        portrait,
    )?;
    assert_eq!(session(&runner).map_view, MapView::default());
    assert_eq!(session(&runner).game.phase(), Phase::Choosing);
    Ok(())
}

#[test]
fn pointer_leave_focus_loss_and_refreshed_resize_cancel_middle_drag() -> LogicResult {
    for boundary in 0..3 {
        let mut runner = build()?;
        zoom(&mut runner, 4.0)?;
        let view = session(&runner).map_view;
        let (x, y) = center();
        send(
            &mut runner,
            &[pointer(x, y, viewport()), middle(ButtonState::Pressed)],
        )?;
        match boundary {
            0 => send(&mut runner, &[InputEvent::PointerLeft])?,
            1 => send(&mut runner, &[InputEvent::FocusLost])?,
            _ => {
                let changed = LogicalViewport::new(720.0, 1000.0)?;
                advance(
                    &mut runner,
                    Duration::ZERO,
                    &[pointer(x + 90.0, y + 40.0, changed)],
                    changed,
                )?;
            }
        }
        send(&mut runner, &[pointer(x + 130.0, y + 70.0, viewport())])?;
        assert_eq!(
            session(&runner).map_view,
            view,
            "cancelled drag resumed across boundary {boundary}"
        );
    }
    Ok(())
}

#[test]
fn fixed_hud_controls_keep_their_coordinates_after_zoom_and_pan() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, 4.0)?;
    let (x, y) = center();
    send(
        &mut runner,
        &[
            pointer(x, y, viewport()),
            middle(ButtonState::Pressed),
            pointer(x + 70.0, y + 60.0, viewport()),
            middle(ButtonState::Released),
        ],
    )?;
    let moved = session(&runner).map_view;
    send(
        &mut runner,
        &click(
            layout::SLIDER.x + layout::SLIDER.width - 1.0,
            layout::SLIDER.y + 10.0,
            viewport(),
        ),
    )?;
    assert_eq!(session(&runner).percent, 100);
    assert_eq!(session(&runner).map_view, moved);
    send(
        &mut runner,
        &click(layout::PAUSE.x + 10.0, layout::PAUSE.y + 10.0, viewport()),
    )?;
    assert!(session(&runner).paused);
    send(
        &mut runner,
        &click(
            layout::NEW_MAP.x + 10.0,
            layout::NEW_MAP.y + 10.0,
            viewport(),
        ),
    )?;
    assert_eq!(session(&runner).game.seed(), SEED + 1);
    assert_eq!(session(&runner).map_view, MapView::default());
    assert!(!session(&runner).paused);
    Ok(())
}

#[test]
fn navigation_preserves_canonical_state_and_future_simulation_sequence() -> LogicResult {
    let (mut runner, mut expected) = started()?;
    let (x, y) = center();
    for _ in 0..4 {
        let events = [
            pointer(x, y, viewport()),
            wheel(ScrollDelta::lines(0.0, 2.0)?),
            middle(ButtonState::Pressed),
            pointer(x + 45.0, y - 30.0, viewport()),
            middle(ButtonState::Released),
        ];
        advance(&mut runner, TICK * 8, &events, viewport())?;
        for _ in 0..8 {
            expected.tick();
        }
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
        assert!(!session(&runner).assisted && !session(&runner).cheats);
    }
    press(&mut runner, PhysicalKeyCode::Space)?;
    expected.expand(25)?;
    for _ in 0..3 {
        advance(&mut runner, TICK * 8, &[], viewport())?;
        for _ in 0..8 {
            expected.tick();
        }
        assert_eq!(snapshot(&session(&runner).game), snapshot(&expected));
    }
    Ok(())
}

#[test]
fn extreme_scroll_and_pointer_values_leave_bounded_finite_rendering() -> LogicResult {
    let mut runner = build()?;
    zoom(&mut runner, ScrollDelta::MAX_DISPLACEMENT)?;
    assert_eq!(session(&runner).map_view.zoom(), 8.0);
    let (x, y) = center();
    for outside in [f32::MAX, -f32::MAX] {
        let sample = PointerSample::new(LogicalScreenPosition::new(outside, outside), viewport())?;
        send(
            &mut runner,
            &[
                pointer(x, y, viewport()),
                middle(ButtonState::Pressed),
                InputEvent::pointer_moved(sample),
                middle(ButtonState::Released),
            ],
        )?;
        let view = session(&runner).map_view;
        assert!((1.0..=8.0).contains(&view.zoom()));
        for cell in [0, WIDTH / 2 + HEIGHT / 2 * WIDTH, WIDTH * HEIGHT - 1] {
            let (px, py) = layout::cell_center(cell);
            let projected = view.project(px, py);
            assert!(projected.0.is_finite() && projected.1.is_finite());
        }
        let visible = view
            .rectangle(layout::MAP)
            .ok_or("map should still fill its viewport")?;
        assert!(visible.x >= layout::MAP.x && visible.y >= layout::MAP.y);
        assert!(visible.x + visible.width <= layout::MAP.x + layout::MAP.width);
        assert!(visible.y + visible.height <= layout::MAP.y + layout::MAP.height);
        let frame = runner.extracted_frame().ok_or("frame")?;
        for rect in frame.resolved_screen_rectangles() {
            assert!(rect.position().is_finite() && rect.size().is_finite());
        }
    }
    zoom(&mut runner, -ScrollDelta::MAX_DISPLACEMENT)?;
    assert_eq!(session(&runner).map_view, MapView::default());
    Ok(())
}

#[test]
fn overflowing_letterbox_unprojection_cancels_across_held_drag_frames() -> LogicResult {
    let mut runner = build()?;
    let portrait = LogicalViewport::new(720.0, 1000.0)?;
    let (x, y) = center();
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            pointer(x, y, portrait),
            wheel(ScrollDelta::lines(0.0, 4.0)?),
            middle(ButtonState::Pressed),
        ],
        portrait,
    )?;
    let before = session(&runner).map_view;
    // Both logical positions are finite, but the 0.5x letterbox scale turns
    // each unprojected coordinate infinite. Separate held-drag frames expose
    // an inf-minus-inf delta if the first invalid sample is retained.
    for coordinate in [f32::MAX, f32::MAX / 1.5] {
        let sample =
            PointerSample::new(LogicalScreenPosition::new(coordinate, coordinate), portrait)?;
        advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::pointer_moved(sample)],
            portrait,
        )?;
        assert_eq!(session(&runner).map_view, before);
        let projected = session(&runner).map_view.project(x, y);
        assert!(projected.0.is_finite() && projected.1.is_finite());
        let frame = runner.extracted_frame().ok_or("frame")?;
        for rectangle in frame.resolved_screen_rectangles() {
            assert!(rectangle.position().is_finite() && rectangle.size().is_finite());
        }
    }
    advance(
        &mut runner,
        Duration::ZERO,
        &[pointer(x + 50.0, y + 30.0, portrait)],
        portrait,
    )?;
    assert_eq!(
        session(&runner).map_view,
        before,
        "invalid sample cancelled the gesture"
    );
    advance(
        &mut runner,
        Duration::ZERO,
        &[middle(ButtonState::Released)],
        portrait,
    )?;
    Ok(())
}

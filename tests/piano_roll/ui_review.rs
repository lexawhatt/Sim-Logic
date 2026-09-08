//! Independent editor/view acceptance checks using ordered synthetic input only.

use std::time::Duration;

use sim_logic::prelude::*;

use super::piano::{
    self, Action, Session, View,
    control::{self, Settings},
    layout::{self, Button, GRID, Layout},
    music::{MAX_NOTES, MAX_PITCH, MIN_PITCH, Note, Sequence},
};

fn viewport() -> LogicalViewport {
    LogicalViewport::new(1440.0, 900.0).unwrap()
}

fn advance(
    runner: &mut HeadlessRunner<Action>,
    events: &[InputEvent],
    viewport: LogicalViewport,
) -> LogicResult {
    match runner.advance_frame(FrameRequest::new(
        Duration::from_millis(16),
        events,
        viewport,
    )) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(())
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

fn pointer(point: Vec2, viewport: LogicalViewport) -> InputEvent {
    let position = Layout::new(viewport).position(point.x(), point.y());
    InputEvent::pointer_moved(PointerSample::new(position, viewport).unwrap())
}

fn cell(pitch: u8, step: u8) -> Vec2 {
    Vec2::new(
        GRID.x + (f32::from(step) + 0.5) * GRID.width / 32.0,
        GRID.y + (f32::from(MAX_PITCH - pitch) + 0.5) * GRID.height / 36.0,
    )
}

fn click_button(runner: &mut HeadlessRunner<Action>, button: Button) -> LogicResult {
    let area = button.area();
    advance(
        runner,
        &[
            pointer(
                Vec2::new(area.x + area.width * 0.5, area.y + area.height * 0.5),
                viewport(),
            ),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )
}

fn empty_editor() -> LogicResult<HeadlessRunner<Action>> {
    let (mut control, stream) = control::channel(48_000)?;
    control.use_offline(stream);
    let (application, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = application.build_headless(initial)?;
    click_button(&mut runner, Button::Clear)?;
    Ok(runner)
}

fn only_note(runner: &HeadlessRunner<Action>) -> Note {
    let session = runner.app_resource::<Session>().unwrap();
    let mut notes = session.settings.sequence.notes().iter().flatten();
    let note = *notes.next().unwrap();
    assert!(notes.next().is_none());
    note
}

#[test]
fn drag_can_return_to_its_original_cell_after_extending_a_note() -> LogicResult {
    let mut runner = empty_editor()?;
    advance(
        &mut runner,
        &[
            pointer(cell(60, 4), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        viewport(),
    )?;
    advance(&mut runner, &[pointer(cell(60, 8), viewport())], viewport())?;
    assert_eq!(only_note(&runner).duration(), 5);
    advance(&mut runner, &[pointer(cell(60, 4), viewport())], viewport())?;
    assert_eq!(only_note(&runner).duration(), 1);
    Ok(())
}

#[test]
fn drag_uses_the_release_endpoint_even_when_motion_and_release_share_a_frame() -> LogicResult {
    let mut runner = empty_editor()?;
    advance(
        &mut runner,
        &[
            pointer(cell(60, 4), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        viewport(),
    )?;
    advance(
        &mut runner,
        &[
            pointer(cell(60, 8), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            // The release edge, not later pointer motion, is the drag endpoint.
            pointer(cell(60, 12), viewport()),
        ],
        viewport(),
    )?;
    assert_eq!(only_note(&runner).duration(), 5);
    Ok(())
}

#[test]
fn complete_drag_between_display_frames_is_not_lost() -> LogicResult {
    let mut runner = empty_editor()?;
    advance(
        &mut runner,
        &[
            pointer(cell(60, 4), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            pointer(cell(60, 8), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    assert_eq!(only_note(&runner).duration(), 5);
    Ok(())
}

#[test]
fn two_complete_drags_in_one_frame_keep_their_separate_endpoints() -> LogicResult {
    let mut runner = empty_editor()?;
    advance(
        &mut runner,
        &[
            pointer(cell(60, 4), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            pointer(cell(60, 8), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            pointer(cell(64, 12), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            pointer(cell(64, 14), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    let session = runner.app_resource::<Session>().ok_or("session")?;
    let notes: Vec<_> = session
        .settings
        .sequence
        .notes()
        .iter()
        .flatten()
        .copied()
        .collect();
    assert_eq!(notes.len(), 2);
    assert_eq!(
        (notes[0].pitch(), notes[0].start(), notes[0].duration()),
        (60, 4, 5)
    );
    assert_eq!(
        (notes[1].pitch(), notes[1].start(), notes[1].duration()),
        (64, 12, 3)
    );
    Ok(())
}

#[test]
fn pause_and_stop_silence_held_keys_until_a_new_physical_press() -> LogicResult {
    for stopping in [false, true] {
        let mut runner = empty_editor()?;
        click_button(&mut runner, Button::Play)?;
        advance(
            &mut runner,
            &[InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed)],
            viewport(),
        )?;
        assert_eq!(
            runner
                .app_resource::<control::Control>()
                .ok_or("control")?
                .snapshot()
                .active_keys,
            1
        );
        click_button(
            &mut runner,
            if stopping { Button::Stop } else { Button::Play },
        )?;
        let stopped = runner
            .app_resource::<control::Control>()
            .ok_or("control")?
            .snapshot();
        assert!(!stopped.playing);
        assert_eq!(stopped.active_keys, 0);
        advance(&mut runner, &[], viewport())?;
        assert_eq!(
            runner
                .app_resource::<control::Control>()
                .ok_or("control")?
                .snapshot()
                .active_keys,
            0
        );
        // This is a new key strike even though held-state is true at both
        // frame boundaries. The intervening release must unblock audition.
        advance(
            &mut runner,
            &[
                InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed),
            ],
            viewport(),
        )?;
        assert_eq!(
            runner
                .app_resource::<control::Control>()
                .ok_or("control")?
                .snapshot()
                .active_keys,
            1
        );
        advance(
            &mut runner,
            &[InputEvent::key(
                PhysicalKeyCode::KeyA,
                ButtonState::Released,
            )],
            viewport(),
        )?;
        assert_eq!(
            runner
                .app_resource::<control::Control>()
                .ok_or("control")?
                .snapshot()
                .active_keys,
            0
        );
    }
    Ok(())
}

#[test]
fn longer_grows_the_selected_note_from_its_actual_duration() -> LogicResult {
    let (mut control, stream) = control::channel(48_000)?;
    control.use_offline(stream);
    let (application, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = application.build_headless(initial)?;
    advance(
        &mut runner,
        &[
            pointer(cell(48, 1), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    let selected = runner
        .app_resource::<Session>()
        .ok_or("session")?
        .selected
        .ok_or("selection")?;
    assert_eq!(
        runner
            .app_resource::<Session>()
            .ok_or("session")?
            .settings
            .sequence
            .notes()[selected]
            .ok_or("note")?
            .duration(),
        7
    );
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::ArrowRight,
            ButtonState::Pressed,
        )],
        viewport(),
    )?;
    let session = runner.app_resource::<Session>().ok_or("session")?;
    assert_eq!(
        session.settings.sequence.notes()[selected]
            .ok_or("note")?
            .duration(),
        8
    );
    assert_eq!(session.length, 8);
    Ok(())
}

#[test]
fn selected_length_stays_at_the_loop_end_limit_so_shorter_still_works() -> LogicResult {
    let (mut control, stream) = control::channel(48_000)?;
    control.use_offline(stream);
    let (application, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = application.build_headless(initial)?;
    advance(
        &mut runner,
        &[
            pointer(cell(48, 25), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    for _ in 0..2 {
        advance(
            &mut runner,
            &[
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
                InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Released),
            ],
            viewport(),
        )?;
    }
    let session = runner.app_resource::<Session>().ok_or("session")?;
    let selected = session.selected.ok_or("selection")?;
    assert_eq!(session.length, 8);
    assert_eq!(
        session.settings.sequence.notes()[selected]
            .ok_or("note")?
            .duration(),
        8
    );
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::ArrowLeft,
            ButtonState::Pressed,
        )],
        viewport(),
    )?;
    let session = runner.app_resource::<Session>().ok_or("session")?;
    assert_eq!(session.length, 7);
    assert_eq!(
        session.settings.sequence.notes()[selected]
            .ok_or("note")?
            .duration(),
        7
    );
    Ok(())
}

#[test]
fn event_time_viewport_controls_click_hit_testing_after_a_resize() -> LogicResult {
    let mut runner = empty_editor()?;
    let clicked_viewport = LogicalViewport::new(600.0, 1000.0)?;
    advance(
        &mut runner,
        &[
            pointer(cell(83, 31), clicked_viewport),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        LogicalViewport::new(1920.0, 1080.0)?,
    )?;
    assert_eq!(only_note(&runner).pitch(), 83);
    assert_eq!(only_note(&runner).start(), 31);
    assert_eq!(only_note(&runner).duration(), 1);
    Ok(())
}

#[test]
fn layout_and_hit_testing_agree_for_every_grid_cell_key_and_button() -> LogicResult {
    for (width, height) in [(1440.0, 900.0), (600.0, 1000.0), (320.0, 240.0), (1.0, 1.0)] {
        let viewport = LogicalViewport::new(width, height)?;
        let layout = Layout::new(viewport);
        for pitch in MIN_PITCH..=MAX_PITCH {
            for step in 0..32 {
                let point = cell(pitch, step);
                let transformed = layout.unproject(layout.position(point.x(), point.y()));
                assert_eq!(layout::grid_cell(transformed), Some((pitch, step)));
            }
            let area = layout::key_area(pitch);
            let point = Vec2::new(area.x + area.width * 0.5, area.y + area.height * 0.9);
            assert_eq!(layout::keyboard_pitch(point), Some(pitch));
        }
        for button in layout::BUTTONS {
            let area = button.area();
            let point = layout
                .unproject(layout.position(area.x + area.width * 0.5, area.y + area.height * 0.5));
            let hit = layout::BUTTONS
                .into_iter()
                .find(|button| button.area().contains(point));
            assert_eq!(hit, Some(button));
        }
    }
    assert_eq!(layout::grid_cell(Vec2::new(GRID.x - 0.01, GRID.y)), None);
    assert_eq!(
        layout::grid_cell(Vec2::new(GRID.x + GRID.width, GRID.y)),
        None
    );
    assert_eq!(
        layout::grid_cell(Vec2::new(GRID.x, GRID.y + GRID.height)),
        None
    );
    Ok(())
}

#[test]
fn a_full_score_fits_both_fixed_visual_pools_at_tiny_and_portrait_sizes() -> LogicResult {
    let mut full = Sequence::new();
    for index in 0..MAX_NOTES {
        full.add(Note::new(
            MIN_PITCH + (index % 36) as u8,
            (index % 32) as u8,
            1,
            100,
        )?)?;
    }
    let (mut control, stream) = control::channel(48_000)?;
    control.use_offline(stream);
    let (mut application, initial) = piano::build_application(control, View::TwoD)?;
    // A test-only frame seeder keeps production code read-only. Drawing sees
    // this score on the following frame; no audio command is needed to test it.
    application.add_frame_system(move |mut session: AppResMut<Session>| {
        session.settings.sequence = full;
    });
    let mut runner = application.build_headless(initial)?;
    advance(&mut runner, &[], viewport())?;
    for (width, height) in [
        (1440.0, 900.0),
        (600.0, 1000.0),
        (1.0, 1.0),
        (1.0, 900.0),
        (900.0, 1.0),
    ] {
        let viewport = LogicalViewport::new(width, height)?;
        advance(&mut runner, &[], viewport)?;
        let frame = runner.extracted_frame().ok_or("frame")?;
        assert_eq!(frame.resolved_screen_rectangles().len(), 384);
        assert_eq!(frame.resolved_screen_images().len(), 96);
        assert_eq!(
            frame
                .resolved_screen_rectangles()
                .iter()
                .filter(|visual| visual.color().alpha() != 0.0)
                .count(),
            250
        );
        assert_eq!(
            frame
                .resolved_screen_images()
                .iter()
                .filter(|visual| visual.tint().alpha() != 0.0)
                .count(),
            93
        );
        for rectangle in frame.resolved_screen_rectangles() {
            assert!(rectangle.position().to_vec2().is_finite());
            assert!(rectangle.size().to_vec2().is_finite());
        }
    }
    click_button(&mut runner, Button::View3d)?;
    for (width, height) in [
        (1440.0, 900.0),
        (600.0, 1000.0),
        (1.0, 1.0),
        (1.0, 900.0),
        (900.0, 1.0),
    ] {
        advance(&mut runner, &[], LogicalViewport::new(width, height)?)?;
        let frame = runner.extracted_frame().ok_or("frame")?;
        assert!(frame.three_d().is_some());
        assert_eq!(frame.resolved_cuboids().len(), 53);
        assert_eq!(
            runner
                .app_resource::<Session>()
                .ok_or("session")?
                .settings
                .sequence,
            full
        );
    }
    Ok(())
}

#[test]
fn switching_views_does_not_send_music_commands_or_interrupt_a_held_key() -> LogicResult {
    let (control, mut stream) = control::channel(48_000)?;
    let (application, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = application.build_headless(initial)?;
    let (mut reference_control, mut reference_stream) = control::channel(48_000)?;
    assert!(reference_control.submit(Settings::default(), control::Action::Play));
    reference_control.set_held_keys(1);
    advance(
        &mut runner,
        &[
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed),
        ],
        viewport(),
    )?;
    for _ in 0..640 {
        assert_eq!(stream.next(), reference_stream.next());
    }
    let sequence = runner
        .app_resource::<Session>()
        .ok_or("session")?
        .settings
        .sequence;
    for button in [Button::View3d, Button::View2d, Button::View3d] {
        click_button(&mut runner, button)?;
        for _ in 0..640 {
            assert_eq!(stream.next(), reference_stream.next());
        }
        let session = runner.app_resource::<Session>().ok_or("session")?;
        assert_eq!(session.settings.sequence, sequence);
        assert!(session.playing);
        let snapshot = runner
            .app_resource::<control::Control>()
            .ok_or("control")?
            .snapshot();
        assert!(snapshot.playing);
        assert_ne!(snapshot.active_keys & 1, 0);
    }
    Ok(())
}

#[test]
fn rejected_editor_change_preserves_the_score_and_view_switch_needs_no_queue_slot() -> LogicResult {
    let (mut control, _stream) = control::channel(48_000)?;
    for _ in 0..control::CONTROL_CAPACITY {
        assert!(control.submit(Settings::default(), control::Action::Update));
    }
    let (application, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = application.build_headless(initial)?;
    let before = runner.app_resource::<Session>().ok_or("session")?.settings;
    advance(
        &mut runner,
        &[
            pointer(cell(83, 4), viewport()),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        ],
        viewport(),
    )?;
    let session = runner.app_resource::<Session>().ok_or("session")?;
    assert_eq!(session.settings, before);
    assert!(session.notice > 0.0);
    click_button(&mut runner, Button::View3d)?;
    let session = runner.app_resource::<Session>().ok_or("session")?;
    assert_eq!(session.view, View::ThreeD);
    assert_eq!(session.settings, before);
    Ok(())
}

use super::piano::{self, Action, Session, View};
use sim_logic::prelude::*;
use std::time::Duration;

fn advance(
    runner: &mut HeadlessRunner<Action>,
    events: &[InputEvent],
    viewport: LogicalViewport,
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        Duration::from_millis(16),
        events,
        viewport,
    )) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

#[test]
fn shared_piano_renders_both_views_without_restarting_music() -> LogicResult {
    let (mut control, stream) = piano::control::channel(48_000)?;
    control.use_offline(stream);
    let (app, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = app.build_headless(initial)?;
    let viewport = LogicalViewport::new(1440.0, 900.0)?;
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
        viewport,
    )?;
    let sequence = runner
        .app_resource::<Session>()
        .ok_or("session")?
        .settings
        .sequence;
    let before = runner
        .app_resource::<piano::control::Control>()
        .ok_or("control")?
        .snapshot();
    assert!(before.playing);
    assert!(runner.extracted_frame().ok_or("frame")?.three_d().is_none());
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
        viewport,
    )?;
    let after = runner
        .app_resource::<piano::control::Control>()
        .ok_or("control")?
        .snapshot();
    assert!(
        after.playing
            && after.frames_generated > before.frames_generated
            && after.step > before.step
    );
    assert_eq!(
        runner
            .app_resource::<Session>()
            .ok_or("session")?
            .settings
            .sequence,
        sequence
    );
    let frame = runner.extracted_frame().ok_or("frame")?;
    assert!(frame.three_d().is_some());
    assert!(frame.resolved_cuboids().len() >= 36);
    assert_eq!(runner.image_asset_count(), 66);
    for (width, height) in [(960.0, 540.0), (600.0, 1000.0), (1.0, 1.0)] {
        advance(&mut runner, &[], LogicalViewport::new(width, height)?)?;
    }
    let exit = advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Escape,
            ButtonState::Pressed,
        )],
        viewport,
    )?;
    assert!(exit.exit_requested());
    Ok(())
}

#[test]
fn backgrounds_precede_score_keys_and_labels_regardless_of_entity_ids() -> LogicResult {
    let (mut control, stream) = piano::control::channel(48_000)?;
    control.use_offline(stream);
    let (app, initial) = piano::build_application(control, View::TwoD)?;
    let mut runner = app.build_headless(initial)?;
    advance(&mut runner, &[], LogicalViewport::new(1440.0, 900.0)?)?;
    let frame = runner.extracted_frame().ok_or("frame")?;
    let panels: Vec<_> = frame
        .resolved_screen_rectangles()
        .iter()
        .filter(|panel| panel.color().alpha() != 0.0)
        .collect();
    assert!(
        panels
            .windows(2)
            .all(|pair| pair[0].draw_order_depth() < pair[1].draw_order_depth())
    );
    assert_eq!(panels[0].position(), LogicalScreenPosition::new(20.0, 16.0));
    let last_panel = panels.last().ok_or("panels")?.draw_order_depth();
    assert!(
        frame
            .resolved_screen_images()
            .iter()
            .filter(|image| image.tint().alpha() != 0.0)
            .all(|image| image.draw_order_depth() > last_panel)
    );
    Ok(())
}

#[test]
fn live_note_depresses_and_restores_the_matching_geometric_key() -> LogicResult {
    let (mut control, stream) = piano::control::channel(48_000)?;
    control.use_offline(stream);
    let (app, initial) = piano::build_application(control, View::ThreeD)?;
    let mut runner = app.build_headless(initial)?;
    let viewport = LogicalViewport::new(1440.0, 900.0)?;
    advance(&mut runner, &[], viewport)?;
    let rest = piano::model3d::parts()?
        .into_iter()
        .find(|(part, _)| part.pitch == Some(48))
        .ok_or("C3 model")?
        .1;
    let source = runner
        .extracted_frame()
        .ok_or("frame")?
        .resolved_cuboids()
        .iter()
        .find(|cuboid| cuboid.visual() == rest)
        .ok_or("C3 record")?
        .source();
    advance(
        &mut runner,
        &[InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed)],
        viewport,
    )?;
    let pressed = runner
        .extracted_frame()
        .ok_or("frame")?
        .resolved_cuboids()
        .iter()
        .find(|cuboid| cuboid.source() == source)
        .ok_or("C3 pressed")?
        .visual();
    assert!(
        (rest.transform().translation().y() - pressed.transform().translation().y() - 0.07).abs()
            < 0.0001
    );
    assert_ne!(pressed.color(), rest.color());
    advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::KeyA,
            ButtonState::Released,
        )],
        viewport,
    )?;
    let released = runner
        .extracted_frame()
        .ok_or("frame")?
        .resolved_cuboids()
        .iter()
        .find(|cuboid| cuboid.source() == source)
        .ok_or("C3 released")?
        .visual();
    assert_eq!(released, rest);
    Ok(())
}

use std::time::Duration;

use sim_logic::prelude::*;

#[path = "../../examples/image_board/game.rs"]
mod board;

fn advance(
    runner: &mut HeadlessRunner<board::DemoAction>,
    delta: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        delta,
        events,
        LogicalViewport::new(1280.0, 720.0)?,
    )) {
        FrameOutcome::Advanced(report) => {
            assert!(report.failure().is_none(), "{:?}", report.failure());
            Ok(report)
        }
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

#[test]
fn image_board_moves_crops_replaces_world_and_exits_through_public_runtime() -> LogicResult {
    let (app, initial) = board::build_application()?;
    let mut runner = app.build_headless(initial)?;
    let (movable, _) = runner
        .components::<board::MovableImage>()
        .next()
        .ok_or("movable image")?;
    let before = *runner.component::<ScreenImageVisual>(movable)?;
    assert!(before.position().to_vec2().x() < 0.0);
    let asset = before.image();
    let pixels = runner.image_asset(asset).ok_or("asset")?.pixels().as_ptr();
    assert_eq!(runner.image_asset_count(), 1);
    assert_eq!(runner.image_pixel_bytes(), 256);
    let frame = runner.extracted_frame().ok_or("snapshot")?;
    assert_eq!(frame.resolved_screen_rectangles().len(), 7);
    assert_eq!(frame.resolved_screen_images().len(), 5);
    assert!(
        frame
            .resolved_screen_images()
            .iter()
            .all(|row| row.image() == asset)
    );

    advance(
        &mut runner,
        Duration::from_millis(100),
        &[
            InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
        ],
    )?;
    let moved = runner.component::<ScreenImageVisual>(movable)?;
    assert!(moved.position().to_vec2().x() > before.position().to_vec2().x());
    assert_eq!(moved.source_region(), Some(ImageRegion::new(0, 0, 8, 4)?));
    advance(
        &mut runner,
        Duration::ZERO,
        &[
            InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
        ],
    )?;

    let generation = runner.world_generation();
    let transition = advance(
        &mut runner,
        Duration::from_millis(100),
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        transition.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), generation);
    assert_eq!(
        runner.image_asset(asset).ok_or("asset")?.pixels().as_ptr(),
        pixels
    );
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("snapshot")?
            .resolved_screen_images()
            .len(),
        5
    );
    let (replacement, _) = runner
        .components::<board::MovableImage>()
        .next()
        .ok_or("new movable")?;
    assert_eq!(
        runner
            .component::<ScreenImageVisual>(replacement)?
            .position()
            .to_vec2()
            .x(),
        900.0
    );

    let exit = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Escape,
            ButtonState::Pressed,
        )],
    )?;
    assert!(exit.exit_requested());
    Ok(())
}

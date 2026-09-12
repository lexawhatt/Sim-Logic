use super::*;
use std::time::Duration;

#[test]
fn zoomed_map_and_badge_ink_stay_inside_map_and_reuse_pools() -> LogicResult {
    use crate::territory_wars::{DEFAULT_SEED, app, drawing};
    let (app, initial) = app::build_demo(DEFAULT_SEED, true)?;
    let mut runner = app.build_headless(initial)?;
    let viewport = LogicalViewport::new(1440.0, 900.0)?;
    let pointer = |x, y| -> LogicResult<InputEvent> {
        Ok(InputEvent::pointer_moved(PointerSample::new(
            LogicalScreenPosition::new(x, y),
            viewport,
        )?))
    };
    let mut canvas = Canvas::new()?;
    let rect_storage = (canvas.rects.as_ptr(), canvas.rects.capacity());
    let glyph_storage = (canvas.glyphs.as_ptr(), canvas.glyphs.capacity());
    for (x, y, steps) in [
        (504.0, 432.0, 32.0),
        (24.01, 112.01, -3.0),
        (983.99, 751.99, 5.0),
        (700.0, 200.0, -1.5),
        (200.0, 600.0, 0.25),
        (504.0, 432.0, -32.0),
    ] {
        let events = [
            pointer(x, y)?,
            InputEvent::mouse_wheel(ScrollDelta::lines(0.0, steps)?),
            InputEvent::mouse_button(MouseButton::Middle, ButtonState::Pressed),
            pointer(504.0, 432.0)?,
            InputEvent::mouse_button(MouseButton::Middle, ButtonState::Released),
        ];
        let FrameOutcome::Advanced(report) =
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport))
        else {
            panic!("valid navigation frame rejected")
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
        canvas.clear();
        draw(&mut canvas, runner.app_resource::<Session>().unwrap())?;
        // First rectangle is the deliberate one-pixel outer frame.
        for rect in canvas.rects.iter().skip(1) {
            within_map(rect.x, rect.y, rect.width, rect.height);
        }
        for glyph in &canvas.glyphs {
            within_map(glyph.x, glyph.y, glyph.width, glyph.height);
        }
        assert_eq!(
            (canvas.rects.as_ptr(), canvas.rects.capacity()),
            rect_storage
        );
        assert_eq!(
            (canvas.glyphs.as_ptr(), canvas.glyphs.capacity()),
            glyph_storage
        );
        assert!(canvas.rect_count() <= drawing::MAX_RECTS);
        assert!(canvas.glyph_count() <= drawing::MAX_GLYPHS);
    }
    Ok(())
}

fn within_map(x: f32, y: f32, width: f32, height: f32) {
    assert!([x, y, width, height].into_iter().all(f32::is_finite));
    assert!(width > 0.0 && height > 0.0);
    assert!(x >= MAP.x && y >= MAP.y, "outside map: {x}, {y}");
    assert!(x + width <= MAP.x + MAP.width + 0.001);
    assert!(y + height <= MAP.y + MAP.height + 0.001);
}

use std::error::Error;

use sim_engine::Layer;
use sim_logic::{prelude::*, screen::ScreenVisualError};

use super::support::screen_rectangle;

#[test]
fn invalid_geometry_style_and_rounded_away_bounds_preserve_the_prior_component()
-> Result<(), Box<dyn Error>> {
    let mut visual = screen_rectangle(-50.0)?;
    visual.set_layer(Layer::new(-10));
    visual.set_draw_order_depth(2.0)?;
    let original = visual;

    assert!(matches!(
        visual.set_position(LogicalScreenPosition::new(f32::NAN, 0.0)),
        Err(ScreenVisualError::InvalidPosition { .. })
    ));
    assert_eq!(visual, original);
    assert!(matches!(
        visual.set_size(LogicalScreenVector::new(0.0, 10.0)),
        Err(ScreenVisualError::InvalidSize { .. })
    ));
    assert_eq!(visual, original);
    for (position, size) in [
        (
            LogicalScreenPosition::new(f32::MAX, 0.0),
            LogicalScreenVector::new(f32::MAX, 1.0),
        ),
        (
            LogicalScreenPosition::new(16_777_216.0, 0.0),
            LogicalScreenVector::new(1.0, 1.0),
        ),
        (
            LogicalScreenPosition::new(0.0, 16_777_216.0),
            LogicalScreenVector::new(1.0, 1.0),
        ),
    ] {
        assert!(matches!(
            ScreenRectangleVisual::new(position, size, Color::WHITE),
            Err(ScreenVisualError::InvalidBounds { .. })
        ));
        assert!(matches!(
            visual.set_geometry(position, size),
            Err(ScreenVisualError::InvalidBounds { .. })
        ));
        assert_eq!(visual, original);
    }
    assert!(matches!(
        visual.set_color(Color::rgba(1.0, 1.0, 1.0, 1.1)),
        Err(ScreenVisualError::InvalidColor { .. })
    ));
    assert_eq!(visual, original);
    assert!(matches!(
        visual.set_draw_order_depth(f32::INFINITY),
        Err(ScreenVisualError::InvalidDrawOrderDepth { .. })
    ));
    assert_eq!(visual, original);

    visual.set_geometry(
        LogicalScreenPosition::new(-100.0, 900.0),
        LogicalScreenVector::new(40.0, 8.0),
    )?;
    visual.set_position(LogicalScreenPosition::new(-80.0, 910.0))?;
    visual.set_size(LogicalScreenVector::new(60.0, 12.0))?;
    visual.set_color(Color::BLACK)?;
    assert_eq!(visual.position(), LogicalScreenPosition::new(-80.0, 910.0));
    assert_eq!(visual.size(), LogicalScreenVector::new(60.0, 12.0));
    assert_eq!(visual.color(), Color::BLACK);
    assert_eq!(visual.layer(), Layer::new(-10));
    assert_eq!(visual.draw_order_depth(), 2.0);
    Ok(())
}

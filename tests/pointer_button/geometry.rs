use sim_logic::prelude::*;

use super::support::*;

#[test]
fn rectangle_uses_half_open_logical_bounds_and_has_no_alpha_policy() -> LogicResult {
    let mut rectangle = rectangle(10.0, 20.0, 30.0, 40.0)?;
    for (x, y, expected) in [
        (10.0, 20.0, true),
        (39.0, 59.0, true),
        (9.0, 20.0, false),
        (10.0, 19.0, false),
        (40.0, 20.0, false),
        (10.0, 60.0, false),
        (40.0, 60.0, false),
    ] {
        assert_eq!(
            rectangle.contains_pointer(sample(x, y, 800.0, 600.0)?),
            expected,
            "point ({x}, {y})"
        );
    }
    rectangle.set_color(Color::rgba(1.0, 1.0, 1.0, 0.0))?;
    assert!(rectangle.contains_pointer(sample(20.0, 30.0, 800.0, 600.0)?));
    Ok(())
}

#[test]
fn offscreen_rectangles_clip_hits_to_the_samples_half_open_viewport() -> LogicResult {
    let rectangle = rectangle(-20.0, -20.0, 1_000.0, 1_000.0)?;
    for (x, y, expected) in [
        (-1.0, 0.0, false),
        (0.0, -1.0, false),
        (0.0, 0.0, true),
        (-0.0, -0.0, true),
        (99.0, 49.0, true),
        (100.0, 0.0, false),
        (0.0, 50.0, false),
    ] {
        assert_eq!(
            rectangle.contains_pointer(sample(x, y, 100.0, 50.0)?),
            expected,
            "point ({x}, {y})"
        );
    }
    let entirely_offscreen = super::support::rectangle(-100.0, -100.0, 10.0, 10.0)?;
    assert!(!entirely_offscreen.contains_pointer(sample(-95.0, -95.0, 800.0, 600.0)?));
    Ok(())
}

#[test]
fn hit_testing_uses_the_stored_viewport_and_the_rectangles_current_geometry() -> LogicResult {
    let mut rectangle = rectangle(10.0, 10.0, 100.0, 100.0)?;
    let old_small_viewport = sample(90.0, 30.0, 80.0, 60.0)?;
    let larger_viewport = sample(90.0, 30.0, 800.0, 600.0)?;
    assert!(!rectangle.contains_pointer(old_small_viewport));
    assert!(rectangle.contains_pointer(larger_viewport));
    rectangle.set_position(LogicalScreenPosition::new(150.0, 10.0))?;
    assert!(!rectangle.contains_pointer(larger_viewport));
    assert!(rectangle.contains_pointer(sample(180.0, 30.0, 800.0, 600.0)?));
    Ok(())
}

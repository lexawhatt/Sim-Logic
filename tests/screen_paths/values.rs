use super::*;

#[test]
fn path_metadata_does_not_widen_every_simple_primitive_record() {
    assert!(
        size_of::<ScreenPolylineVisual>() <= size_of::<ScreenRectangleVisual>(),
        "path metadata {} exceeds rectangle metadata {}; avoid charging every simple primitive for an inline path style",
        size_of::<ScreenPolylineVisual>(),
        size_of::<ScreenRectangleVisual>()
    );
}

#[test]
fn points_are_shared_and_rejected_edits_preserve_the_snapshot() -> LogicResult {
    let mut path = path()?;
    let original = path.clone();
    assert_eq!(path.points().as_ptr(), original.points().as_ptr());
    path.set_points(&points())?;
    assert_eq!(path.points().as_ptr(), original.points().as_ptr());
    assert!(path.point_allocation_bytes() >= 3 * size_of::<LogicalScreenPosition>());
    for bad in [
        vec![],
        vec![points()[0]],
        vec![points()[0], points()[1], points()[1]],
        vec![points()[0], LogicalScreenPosition::new(f32::NAN, 0.0)],
        vec![points()[0], points()[1], points()[0]],
    ] {
        assert!(path.set_points(&bad).is_err());
        assert_eq!(path, original);
        assert_eq!(path.points().as_ptr(), original.points().as_ptr());
    }
    let world_width = StrokeStyle2d::world(WorldLength::new(1.0)?, Color::WHITE);
    assert!(path.set_style(world_width).is_err());
    assert_eq!(path, original);
    assert!(path.set_draw_order_depth(f32::NAN).is_err());
    assert_eq!(path, original);
    path.set_style(path.style().with_cap(StrokeCap2d::Square))?;
    assert_eq!(path.points().as_ptr(), original.points().as_ptr());
    assert_eq!(original.style().cap(), StrokeCap2d::Round);
    assert_eq!(path.style().cap(), StrokeCap2d::Square);
    let mut changed = points();
    changed[2] = LogicalScreenPosition::new(80.25, 45.5);
    path.set_points(&changed)?;
    assert_ne!(path.points().as_ptr(), original.points().as_ptr());
    assert_eq!(original.points(), points());
    Ok(())
}

#[test]
fn full_stroke_styles_use_engine_validation_and_closed_paths_are_explicit() -> LogicResult {
    for cap in [StrokeCap2d::Butt, StrokeCap2d::Square, StrokeCap2d::Round] {
        for join in [
            StrokeJoin2d::Bevel,
            StrokeJoin2d::Round,
            StrokeJoin2d::Miter,
        ] {
            let style = StrokeStyle2d::new(2.0, Color::WHITE)
                .with_cap(cap)
                .with_join(join);
            let path = ScreenPolylineVisual::new(&points(), style)?;
            assert_eq!(path.style(), style);
        }
    }
    let style = path()?
        .style()
        .with_dash_pattern(StrokeDashPattern2d::new(&[8.0, 4.0], 1.0, 64)?);
    let dashed = ScreenPolylineVisual::new(&points(), style)?;
    assert_eq!(dashed.style(), style);
    // Dash gaps are intentionally eligible for centerline selection.
    assert!(dashed.hit_test_centerline(pointer(20.25, 20.5)?, 0.0)?);
    let arrow = StrokeMarker2d::arrow(LogicalPixels::new(5.0)?, LogicalPixels::new(3.0)?);
    let style = path()?.style().with_end_marker(arrow);
    assert_eq!(ScreenPolylineVisual::new(&points(), style)?.style(), style);
    // Engine's exact-tip route only accepts a two-point, undashed shaft.
    let exact_tip = style.with_end_marker(arrow.with_anchor(StrokeMarkerAnchor2d::TipAtEndpoint));
    assert!(ScreenPolylineVisual::new(&points(), exact_tip).is_err());
    assert!(ScreenPolylineVisual::new(&points()[..2], exact_tip).is_ok());
    assert!(matches!(
        ScreenPolylineVisual::new(&[points()[0], points()[1], points()[0]], path()?.style()),
        Err(ScreenPolylineError::ClosedPathUnsupported)
    ));
    for limit in [0, 1, 2] {
        assert!(matches!(
            ScreenPolylineVisual::with_point_limit(&points(), path()?.style(), limit),
            Err(ScreenPolylineError::PointLimit { .. })
        ));
    }
    let mut bounded = ScreenPolylineVisual::with_point_limit(&points(), path()?.style(), 3)?;
    let before = bounded.clone();
    assert!(bounded.set_points(&[points()[0]; 4]).is_err());
    assert_eq!(bounded, before);
    Ok(())
}

#[test]
fn centerline_picking_has_explicit_logical_radius_and_obeys_clip() -> LogicResult {
    let mut path = path()?;
    assert!(path.hit_test_centerline(pointer(30.25, 20.5)?, 0.0)?);
    assert!(path.hit_test_centerline(pointer(30.25, 21.0)?, 0.5)?);
    assert!(!path.hit_test_centerline(pointer(30.25, 21.0)?, 0.25)?);
    assert!(path.hit_test_centerline(pointer(9.75, 20.5)?, 0.5)?);
    assert!(!path.hit_test_centerline(pointer(9.75, 20.5)?, 0.0)?);
    for radius in [-1.0, f32::NAN, f32::INFINITY] {
        assert!(matches!(
            path.hit_test_centerline(pointer(30.25, 20.5)?, radius),
            Err(ScreenPolylineError::InvalidPickRadius)
        ));
    }
    path.set_clip(ScreenClip::new(
        LogicalScreenPosition::new(20.0, 10.0),
        LogicalScreenVector::new(20.0, 20.0),
    )?);
    assert!(path.hit_test_centerline(pointer(30.25, 20.5)?, 1.0)?);
    assert!(!path.hit_test_centerline(pointer(50.25, 20.5)?, 1.0)?);
    path.set_clip(ScreenClip::Empty);
    assert!(!path.hit_test_centerline(pointer(30.25, 20.5)?, 100.0)?);
    Ok(())
}

#[test]
fn outline_circle_has_no_filled_disk_and_rejects_removing_its_only_paint() -> LogicResult {
    let mut circle = ScreenCircleVisual::outlined(
        LogicalScreenPosition::new(50.25, 50.5),
        10.0,
        2.0,
        Color::WHITE,
    )?;
    assert_eq!(circle.fill_color(), None);
    assert!(!circle.contains_pointer(pointer(50.25, 50.5)?));
    assert!(circle.contains_pointer(pointer(59.25, 50.5)?));
    assert!(circle.contains_pointer(pointer(61.25, 50.5)?));
    assert!(!circle.contains_pointer(pointer(61.5, 50.5)?));
    let before = circle;
    assert!(matches!(
        circle.set_stroke(None),
        Err(ScreenVisualError::MissingPaint)
    ));
    assert_eq!(circle, before);
    assert!(
        circle
            .set_stroke(Some(Stroke::new(0.0, Color::WHITE)))
            .is_err()
    );
    assert_eq!(circle, before);
    circle.set_color(Color::BLACK)?;
    circle.set_stroke(None)?;
    assert_eq!(circle.fill_color(), Some(Color::BLACK));
    assert!(circle.contains_pointer(pointer(50.25, 50.5)?));
    Ok(())
}

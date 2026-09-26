use super::*;

#[test]
fn ten_thousand_points_are_one_managed_primitive_and_one_draw_run() -> LogicResult {
    // Straight exactly representable samples isolate source scaling, not tolerance.
    let points: Vec<_> = (0..10_000)
        .map(|i| LogicalScreenPosition::new(i as f32 * 0.25, 40.5))
        .collect();
    let path = ScreenPolylineVisual::new(&points, StrokeStyle2d::new(1.5, Color::WHITE))?;
    let source_ptr = path.points().as_ptr();
    let limits = RenderLimits::default()
        .with_max_screen_polyline_points(10_000)
        .with_screen_scene_budget(SceneBudget::new(
            256,
            10_000,
            2_000_000,
            8 * 1024 * 1024,
            16 * 1024 * 1024,
            128 * 1024 * 1024,
            256,
        ));
    let runner = runner_with_paths(vec![path.clone()], limits)?;
    let snapshot = runner.extracted_frame().ok_or("snapshot")?;
    let records: Vec<_> = snapshot.screen_primitives().collect();
    assert_eq!(records.len(), 1);
    let ResolvedScreenPrimitive::Polyline { visual, .. } = &records[0] else {
        return Err("path was split into unrelated primitives".into());
    };
    assert_eq!(visual.points().len(), 10_000);
    assert_eq!(visual.points().as_ptr(), source_ptr);
    assert_eq!(
        snapshot.screen_draws(),
        &[ScreenDraw::Primitives { run: 0 }]
    );
    assert!(runner_with_paths(vec![path], limits.with_max_screen_polyline_points(9_999)).is_err());
    Ok(())
}

#[test]
fn hidden_paths_and_shared_points_still_count_toward_both_source_limits() -> LogicResult {
    let mut path = path()?;
    path.set_clip(ScreenClip::Empty);
    let limits = RenderLimits::default()
        .with_max_screen_polylines(2)
        .with_max_screen_polyline_points(6);
    assert!(runner_with_paths(vec![path.clone(), path.clone()], limits).is_ok());
    assert!(
        runner_with_paths(
            vec![path.clone(), path.clone()],
            limits.with_max_screen_polylines(1)
        )
        .is_err()
    );
    assert!(
        runner_with_paths(
            vec![path.clone(), path],
            limits.with_max_screen_polyline_points(5)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn paths_update_in_the_same_paused_frame_and_disabled_sources_are_excluded() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_fallible_frame_system(
        |viewport: FrameViewport, mut paths: Query<&mut ScreenPolylineVisual>| -> LogicResult {
            for mut path in &mut paths {
                let mut changed = points();
                changed[2] = LogicalScreenPosition::new(viewport.logical().width() - 10.25, 60.5);
                path.set_points(&changed)?;
            }
            Ok(())
        },
    );
    let path = path()?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("paused", move |world| {
        world.spawn(camera)?;
        world.spawn(path.clone())?;
        world.spawn((Disabled, path.clone()))?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    runner.set_paused(true);
    for width in [800.0, 1000.0, 400.0] {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(150),
            &[],
            LogicalViewport::new(width, 600.0)?,
        )) else {
            return Err("frame rejected".into());
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
        assert_eq!(report.fixed_ticks_attempted(), 0);
        let snapshot = runner.extracted_frame().ok_or("snapshot")?;
        assert_eq!(snapshot.screen_primitives().count(), 1);
        let Some(ResolvedScreenPrimitive::Polyline { visual, .. }) =
            snapshot.screen_primitives().next()
        else {
            return Err("missing path".into());
        };
        assert_eq!(visual.points()[2].to_vec2().x(), width - 10.25);
    }
    Ok(())
}

#[derive(Resource, Default)]
struct Step(bool);

#[test]
fn failed_extraction_preserves_published_points_and_later_valid_edit_recovers() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_screen_polyline_points(3));
    let mut app = Application::<Action>::new(config)?;
    app.add_fallible_frame_system(
        |mut step: ResMut<Step>, mut paths: Query<&mut ScreenPolylineVisual>| -> LogicResult {
            for mut path in &mut paths {
                if step.0 {
                    path.set_points(&points())?;
                } else {
                    let mut longer = points().to_vec();
                    longer.push(LogicalScreenPosition::new(100.25, 40.5));
                    path.set_points(&longer)?;
                }
            }
            step.0 = true;
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let path = path()?;
    let initial = app.register_world("atomic", move |world| {
        world.spawn(camera)?;
        world.spawn(path.clone())?;
        world.insert_resource(Step::default())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let before = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .screen_primitives()
        .next()
        .ok_or("path")?;
    for step in 0..2 {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::ZERO,
            &[],
            LogicalViewport::new(800.0, 600.0)?,
        )) else {
            return Err("frame rejected".into());
        };
        if step == 0 {
            assert!(matches!(
                report.failure(),
                Some(FrameFailure::Extraction(
                    ExtractionError::ScreenPolylinePointsLimitExceeded {
                        limit: 3,
                        requested: 4,
                        ..
                    }
                ))
            ));
        } else {
            assert!(report.failure().is_none(), "{:?}", report.failure());
        }
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("snapshot")?
                .screen_primitives()
                .next(),
            Some(before.clone())
        );
    }
    Ok(())
}

#[test]
fn paths_share_mixed_order_with_images_and_other_geometry() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_screen_images(1));
    let mut app = Application::<Action>::new(config)?;
    let image = app.register_image_rgba8(1, 1, &[255; 4])?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut lower = path()?;
    lower.set_layer(Layer::new(-1));
    let mut upper = path()?;
    upper.set_layer(Layer::new(1));
    let image = ScreenImageVisual::new(
        image,
        LogicalScreenPosition::new(5.0, 5.0),
        LogicalScreenVector::new(8.0, 8.0),
    )?;
    let circle = ScreenCircleVisual::outlined(
        LogicalScreenPosition::new(20.0, 20.0),
        5.0,
        1.0,
        Color::WHITE,
    )?;
    let initial = app.register_world("mixed", move |world| {
        world.spawn(camera)?;
        // Same-entity geometry precedes its image; the upper path follows both.
        world.spawn((lower.clone(), circle, image))?;
        world.spawn(upper.clone())?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    let frame = runner.extracted_frame().ok_or("snapshot")?;
    assert_eq!(
        frame.screen_draws(),
        &[
            ScreenDraw::Primitives { run: 0 },
            ScreenDraw::Image { index: 0 },
            ScreenDraw::Primitives { run: 1 },
        ]
    );
    let lower: Vec<_> = frame
        .screen_primitive_run_records(0)
        .ok_or("lower")?
        .collect();
    assert!(matches!(
        &lower[..],
        [
            ResolvedScreenPrimitive::Polyline { .. },
            ResolvedScreenPrimitive::Circle { .. }
        ]
    ));
    assert!(matches!(
        frame.screen_primitive_run_records(1).ok_or("upper")?.next(),
        Some(ResolvedScreenPrimitive::Polyline { .. })
    ));
    Ok(())
}

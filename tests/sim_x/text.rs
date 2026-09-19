use sim_logic::prelude::*;
const FONT: &[u8] = include_bytes!("../assets/text/DejaVuSans.ttf");
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

#[test]
fn size_styles_share_one_face_charge_but_keep_separate_cache_identity() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_text_limits(TextLimits::new(3, FONT.len()));
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_texts(3)
            .with_max_screen_text_bytes(1024)
            .with_max_screen_text_glyphs(256),
    );
    let mut app = Application::<Action>::new(config)?;
    let ui = app.register_font(FONT.to_vec(), TextSettings::new(16.0)?)?;
    let heading = app.register_font_style(&ui, TextSettings::new(32.0)?)?;
    let logo = app.register_font_style(&heading, TextSettings::new(48.0)?)?;
    assert!(ui.shares_face_with(&heading));
    assert!(ui.shares_face_with(&logo));
    assert_ne!(ui, heading);
    assert!(
        app.register_font_style(&ui, TextSettings::new(64.0)?)
            .is_err()
    );
    let mut other = Application::<Action>::new(AppConfig::default())?;
    assert!(matches!(
        other.register_font_style(&ui, TextSettings::default()),
        Err(TextError::ForeignFont)
    ));
    let small = ScreenTextVisual::new(ui, "Sim;X", LogicalScreenPosition::new(40.0, 50.0))?;
    let large = ScreenTextVisual::new(heading, "Sim;X", LogicalScreenPosition::new(40.0, 100.0))?;
    assert!((large.metrics().advance() - small.metrics().advance() * 2.0).abs() < 0.001);
    let initial = app.register_world("typography", move |world| {
        world.spawn(
            ActiveCamera2d::centered(1.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        world.spawn(small.clone())?;
        world.spawn(large.clone())?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    assert_eq!(runner.text_font_count(), 3);
    assert_eq!(runner.text_font_bytes(), FONT.len());
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("snapshot")?
            .resolved_screen_texts()
            .len(),
        2
    );
    Ok(())
}

#[test]
fn shared_style_checks_limits_and_clip_does_not_reshape() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let ui = app.register_font(FONT.to_vec(), TextSettings::default())?;
    assert!(
        app.register_font_style(
            &ui,
            TextSettings::default().with_font_budget(sim_engine::FontBudget::new(1, 1))
        )
        .is_err()
    );
    let mut label = ScreenTextVisual::new(ui, "Hello", LogicalScreenPosition::new(20.0, 40.0))?;
    let metrics = label.metrics();
    let clip = ScreenClip::new(
        LogicalScreenPosition::new(20.0, 20.0),
        LogicalScreenVector::new(15.0, 20.0),
    )?;
    label.set_clip(clip);
    assert_eq!(label.metrics(), metrics);
    assert_eq!(label.clip(), clip);
    label.set_clip(ScreenClip::Empty);
    assert_eq!(label.metrics(), metrics);
    Ok(())
}

#[test]
fn same_entity_mixed_kind_ties_keep_geometry_then_image_then_text() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(1)
            .with_max_screen_texts(1)
            .with_max_screen_text_bytes(128)
            .with_max_screen_text_glyphs(64),
    );
    let mut app = Application::<Action>::new(config)?;
    let image = app.register_image_rgba8(1, 1, &[255; 4])?;
    let font = app.register_font(FONT.to_vec(), TextSettings::default())?;
    let position = LogicalScreenPosition::new(20.0, 20.0);
    let size = LogicalScreenVector::new(50.0, 30.0);
    let rectangle = ScreenRectangleVisual::rounded(position, size, Color::WHITE, 8.0)?;
    let line = ScreenLineVisual::new(
        position,
        LogicalScreenPosition::new(70.0, 40.0),
        2.0,
        Color::BLACK,
    )?;
    let circle = ScreenCircleVisual::new(position, 8.0, Color::WHITE)?;
    let image = ScreenImageVisual::new(image, position, size)?;
    let text = ScreenTextVisual::new(font, "A", position)?;
    let initial = app.register_world("same-source", move |world| {
        world.spawn(
            ActiveCamera2d::centered(1.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        world.spawn((text.clone(), image, circle, line, rectangle))?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    let snapshot = runner.extracted_frame().ok_or("snapshot")?;
    assert_eq!(
        snapshot.screen_draws(),
        &[
            ScreenDraw::Primitives { run: 0 },
            ScreenDraw::Image { index: 0 },
            ScreenDraw::Text { index: 0 },
        ]
    );
    let run: Vec<_> = snapshot
        .screen_primitive_run_records(0)
        .ok_or("run")?
        .collect();
    assert!(matches!(
        run.as_slice(),
        [
            ResolvedScreenPrimitive::Rectangle(_),
            ResolvedScreenPrimitive::Line { .. },
            ResolvedScreenPrimitive::Circle { .. },
        ]
    ));
    assert_eq!(
        snapshot
            .screen_rectangle_run_records(0)
            .ok_or("rectangles")?
            .len(),
        1
    );
    assert!(
        run.iter()
            .all(|primitive| primitive.source() == snapshot.resolved_screen_images()[0].source())
    );
    Ok(())
}

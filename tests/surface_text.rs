#![cfg(feature = "headless-text")]
use sim_logic::{
    fonts::TextDirection,
    prelude::*,
    text::{SurfaceTextError, layout_surface_text},
};
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}
#[test]
fn cyrillic_page_wraps_and_raster_survives_session_changes() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let font = app.register_font(
        include_bytes!("assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(20.0)?.with_direction(TextDirection::Ltr),
    )?;
    let mut session = font.shaping_session()?;
    let width = session.shape_line("Привет ")?.advance() + 0.1;
    let lines = layout_surface_text(&mut session, "Привет мир", width)?;
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0].source(), 0..13);
    assert_eq!(lines[1].source(), 13..19);
    assert!(lines[0].caret_x(1).is_err());
    let bounds = lines[0].selection_bounds(0..13)?;
    assert!(bounds[2] > 0.0 && bounds[3] > 0.0);
    let glyph = lines[0].shaped().glyphs()[0].glyph_id();
    let raster = font.rasterize_glyph(glyph, 1.0)?;
    assert!(!raster.coverage().is_empty());
    session.clear_scratch();
    assert_eq!(lines[0].shaped().text(), "Привет ");
    assert!(matches!(
        layout_surface_text(&mut session, "longword", 1.0),
        Err(SurfaceTextError::WordTooWide)
    ));
    assert!(matches!(
        layout_surface_text(&mut session, &"a".repeat(4097), 100.0),
        Err(SurfaceTextError::Limit)
    ));
    Ok(())
}

#[test]
fn pen_carets_preserve_cluster_errors_and_document_offsets() -> LogicResult {
    use sim_logic::fonts::ShapedGeometryError;
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let font = app.register_font(
        include_bytes!("assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(32.0)?.with_direction(TextDirection::Ltr),
    )?;
    let mut session = font.shaping_session()?;
    let lines = layout_surface_text(&mut session, "Я\nq\u{301}X\noffice\nAV\n", 1000.0)?;
    let marks = &lines[1];
    assert_eq!(marks.source(), 3..7);
    let shaped = marks.shaped();
    assert_ne!(
        shaped.glyphs()[1].logical_x(),
        shaped.glyphs()[1].pen_start().logical_x()
    );
    assert_eq!(marks.caret_x(3)?, 0.0);
    assert_eq!(marks.caret_x(6)?, shaped.glyphs()[1].pen_end().logical_x());
    assert!(std::ptr::eq(marks.caret_at(6)?.line(), shaped));
    assert_eq!(marks.caret_at(6)?.byte_offset(), 3);
    assert!(matches!(marks.caret_at(4), Err(SurfaceTextError::Geometry {
        source_start: 3, error: ShapedGeometryError::InsideCluster { byte_offset: 1, cluster_range }
    }) if cluster_range == (0..3)));
    assert!(matches!(
        marks.caret_at(5),
        Err(SurfaceTextError::Geometry {
            error: ShapedGeometryError::InvalidByteOffset { byte_offset: 2 },
            ..
        })
    ));
    assert!(matches!(
        marks.caret_at(2),
        Err(SurfaceTextError::SourceOffset { .. })
    ));
    assert!(matches!(
        marks.caret_at(8),
        Err(SurfaceTextError::SourceOffset { .. })
    ));
    assert!(matches!(
        marks.selection(std::ops::Range { start: 6, end: 3 }),
        Err(SurfaceTextError::Geometry {
            error: ShapedGeometryError::ReversedRange,
            ..
        })
    ));
    assert_eq!(marks.selection(3..6)?.count(), 1);
    assert_eq!(marks.selection_bounds(6..6)?[2], 0.0);
    let ligature = &lines[2];
    let cluster = ligature
        .shaped()
        .clusters()
        .find(|c| c.byte_range().len() > 1)
        .unwrap();
    let inside = ligature.source().start + cluster.byte_range().start + 1;
    assert!(matches!(
        ligature.caret_at(inside),
        Err(SurfaceTextError::Geometry {
            error: ShapedGeometryError::InsideCluster { .. },
            ..
        })
    ));
    let av = &lines[3];
    let a = session.shape_line("A")?;
    assert!(av.caret_x(av.source().start + 1)? < a.advance());
    let empty = &lines[4];
    assert_eq!(empty.caret_x(empty.source().start)?, 0.0);
    assert_eq!(empty.selection(empty.source())?.count(), 0);
    Ok(())
}

#[test]
fn managed_rtl_geometry_keeps_visual_order_and_provenance() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let font = app.register_font(
        include_bytes!("assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(24.0)?.with_direction(TextDirection::Rtl),
    )?;
    let mut session = font.shaping_session()?;
    let line = session.shape_line("אבג")?;
    assert_eq!(line.resolved_direction(), TextDirection::Rtl);
    let clusters: Vec<_> = line.clusters().collect();
    assert_eq!(
        clusters.iter().map(|c| c.byte_range()).collect::<Vec<_>>(),
        [4..6, 2..4, 0..2]
    );
    assert!(clusters.iter().all(|c| std::ptr::eq(c.line(), &line)));
    assert_eq!(line.caret_at(0)?.position().logical_x(), line.advance());
    assert_eq!(line.caret_at(6)?.position().logical_x(), 0.0);
    assert_eq!(
        line.selection(0..4)?
            .map(|c| c.byte_range())
            .collect::<Vec<_>>(),
        [2..4, 0..2]
    );
    session.clear_scratch();
    assert_eq!(line.selection(2..2)?.count(), 0);
    // Paragraph bidi/wrapping remains a separate application policy.
    assert!(matches!(
        layout_surface_text(&mut session, "אבג", 100.0),
        Err(SurfaceTextError::Direction)
    ));
    Ok(())
}

#![cfg(feature = "text")]

//! Public prepared-label ownership and reusable-session contracts; no GPU needed.

use sim_engine::{FontError, ShapedLineError, TextLayoutBudget};
use sim_logic::prelude::*;

const FONT: &[u8] = include_bytes!("assets/text/DejaVuSans.ttf");

fn font(settings: TextSettings) -> LogicResult<TextFont> {
    let mut app = Application::<u8>::new(AppConfig::default())?;
    Ok(app.register_font(FONT.to_vec(), settings)?)
}

#[test]
fn label_retains_exact_engine_layout_and_clones_share_it() -> LogicResult {
    let font = font(TextSettings::new(20.0)?)?;
    let label = ScreenTextVisual::new(
        font.clone(),
        "AV ffi Привет",
        LogicalScreenPosition::new(20.0, 40.0),
    )?;
    let clone = label.clone();
    assert!(std::ptr::eq(label.shaped_line(), clone.shaped_line()));
    assert_eq!(label.text().as_ptr(), label.shaped_line().text().as_ptr());
    assert_eq!(label.shaped_line().style(), &font.settings().style(1.0)?);
    assert_eq!(label.metrics().advance(), label.shaped_line().advance());
    assert_eq!(label.glyph_count(), label.shaped_line().glyphs().len());
    assert_eq!(
        label.retained_layout_bytes(),
        label.shaped_line().allocation_bytes()
    );
    assert_eq!(label.retained_text_bytes(), label.retained_layout_bytes());
    assert!(label.retained_layout_bytes() >= label.text().len());
    Ok(())
}

#[test]
fn borrowed_session_reuses_one_plan_without_borrowing_returned_labels() -> LogicResult {
    let font = font(TextSettings::default())?;
    let mut session = font.shaping_session()?;
    assert_eq!(session.cached_plan_count(), 0);
    let mut label = ScreenTextVisual::new_with_session(
        &mut session,
        "Count: 100",
        LogicalScreenPosition::new(20.0, 40.0),
    )?;
    let before = label.clone();
    assert_eq!(session.cached_plan_count(), 1);
    label.set_text_with_session(&mut session, "Count: 101")?;
    assert_eq!(session.cached_plan_count(), 1);
    assert_eq!(before.text(), "Count: 100");
    assert_eq!(label.text(), "Count: 101");
    assert!(!std::ptr::eq(label.shaped_line(), before.shaped_line()));
    session.clear_scratch();
    assert_eq!(session.cached_plan_count(), 0);
    assert_eq!(session.allocation_bytes(), 0);
    drop(session);
    drop(font);
    assert_eq!(label.text(), "Count: 101");
    assert_eq!(before.text(), "Count: 100");
    Ok(())
}

#[test]
fn equal_text_with_matching_session_does_not_warm_shaping_state() -> LogicResult {
    let font = font(TextSettings::default())?;
    let mut label =
        ScreenTextVisual::new(font.clone(), "Same", LogicalScreenPosition::new(0.0, 40.0))?;
    let before = label.clone();
    let mut session = font.shaping_session()?;
    label.set_text_with_session(&mut session, "Same")?;
    assert!(std::ptr::eq(label.shaped_line(), before.shaped_line()));
    assert_eq!(session.cached_plan_count(), 0);
    assert_eq!(session.allocation_bytes(), 0);
    Ok(())
}

#[test]
fn foreign_session_is_rejected_even_with_equal_text_or_equal_font_bytes() -> LogicResult {
    let local = font(TextSettings::default())?;
    let foreign = font(TextSettings::default())?;
    let mut label = ScreenTextVisual::new(local, "Same", LogicalScreenPosition::new(0.0, 40.0))?;
    let before = label.clone();
    let mut session = foreign.shaping_session()?;
    for text in ["Same", "Changed"] {
        assert!(matches!(
            label.set_text_with_session(&mut session, text),
            Err(TextError::Prepared(ShapedLineError::FontMismatch))
        ));
        assert_eq!(label, before);
        assert!(std::ptr::eq(label.shaped_line(), before.shaped_line()));
        assert_eq!(session.cached_plan_count(), 0);
    }
    Ok(())
}

#[test]
fn failed_session_changes_preserve_text_metrics_and_presentation() -> LogicResult {
    let settings = TextSettings::new(20.0)?.with_layout_budget(TextLayoutBudget::new(
        16,
        16,
        1024 * 1024,
        16 * 1024,
    ));
    let font = font(settings)?;
    let mut session = font.shaping_session()?;
    let mut label = ScreenTextVisual::new_with_session(
        &mut session,
        "Good",
        LogicalScreenPosition::new(60.0, 40.0),
    )?;
    label.set_alignment(TextAlignment::Right)?;
    label.set_tint(Color::rgba(0.2, 0.4, 0.6, 0.5))?;
    let before = label.clone();
    for text in [
        "bad\nline",
        "\u{10ffff}",
        "A line longer than the configured budget",
    ] {
        assert!(label.set_text_with_session(&mut session, text).is_err());
        assert_eq!(label, before);
        assert!(std::ptr::eq(label.shaped_line(), before.shaped_line()));
    }
    label.set_text_with_session(&mut session, "Recovered")?;
    assert_eq!(label.text(), "Recovered");
    assert_eq!(before.text(), "Good");
    Ok(())
}

#[test]
fn canonical_prepared_line_rejects_a_different_dpi_instead_of_relabelling_it() -> LogicResult {
    let font = font(TextSettings::default())?;
    let label =
        ScreenTextVisual::new(font.clone(), "Scale", LogicalScreenPosition::new(0.0, 40.0))?;
    let line = label.shaped_line();
    line.validate_for(
        line.font(),
        &font.settings().style(1.0)?,
        &font.settings().layout_budget(),
    )?;
    assert!(matches!(
        line.validate_for(
            line.font(),
            &font.settings().style(1.25)?,
            &font.settings().layout_budget()
        ),
        Err(ShapedLineError::StyleMismatch)
    ));
    assert!(matches!(
        line.validate_for(
            line.font(),
            line.style(),
            &TextLayoutBudget::new(1, 1, 0, 0)
        ),
        Err(ShapedLineError::Font(FontError::BudgetExceeded { .. }))
    ));
    Ok(())
}

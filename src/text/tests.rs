use std::{collections::HashSet, error::Error};

use sim_engine::{
    Color, FontBudget, FontBudgetResource, FontError, GlyphRunBudget, Layer, LogicalScreenPosition,
    TextAtlasBudget, TextDirection, TextLayoutBudget,
};

use crate::identity::ApplicationId;

use super::*;

type TestResult = Result<(), Box<dyn Error>>;

const FONT: &[u8] = include_bytes!("../../tests/assets/text/DejaVuSans.ttf");

fn registry(application: u64, limits: TextLimits) -> TextRegistry {
    TextRegistry::new(ApplicationId::from_raw(application), limits)
}

fn font(settings: TextSettings) -> Result<TextFont, TextError> {
    registry(1, TextLimits::default()).register(FONT.to_vec(), settings)
}

fn label(text: &str) -> Result<ScreenTextVisual, TextError> {
    ScreenTextVisual::new(
        font(TextSettings::default())?,
        text,
        LogicalScreenPosition::new(100.0, 60.0),
    )
}

#[test]
fn settings_validate_size_and_scale_without_gpu() -> TestResult {
    let defaults = TextSettings::default();
    assert_eq!(defaults.logical_em_size(), 24.0);
    assert_eq!(defaults.direction(), TextDirection::Auto);
    assert_eq!(defaults.layout_budget().max_text_bytes(), 4096);
    assert_eq!(defaults.layout_budget().max_shaped_glyphs(), 1024);
    assert_eq!(TextLimits::default(), TextLimits::new(8, 32 * 1024 * 1024));
    for value in [0.0, -1.0, f32::from_bits(1), f32::INFINITY, f32::NAN] {
        assert!(matches!(
            TextSettings::new(value),
            Err(TextError::Font(FontError::InvalidScale))
        ));
    }
    for scale in [0.0, -1.0, f32::INFINITY, f32::NAN] {
        assert!(defaults.style(scale).is_err());
    }
    let settings = TextSettings::new(18.0)?.with_direction(TextDirection::Rtl);
    assert_eq!(settings.style(1.25)?.physical_em_size(), 22.5);
    assert_eq!(settings.style(1.25)?.direction(), TextDirection::Rtl);
    assert!(TextSettings::new(f32::MAX)?.style(2.0).is_err());
    Ok(())
}

#[test]
fn registration_owns_source_and_clones_share_identity_and_bytes() -> TestResult {
    let mut fonts = registry(1, TextLimits::default());
    let first = fonts.register(FONT.to_vec(), TextSettings::default())?;
    let clone = first.clone();
    assert_eq!(first, clone);
    assert!(fonts.contains(&clone));
    assert_eq!(
        first.face().font_data().as_ptr(),
        clone.face().font_data().as_ptr()
    );
    assert_eq!(first.face().font_data(), FONT);
    assert_eq!(fonts.len(), 1);
    assert_eq!(fonts.font_bytes(), first.font_bytes());
    assert_eq!(first.slot(), 0);
    assert!(format!("{first:?}").len() < 256);
    Ok(())
}

#[test]
fn equal_registrations_and_foreign_registrations_are_not_equal() -> TestResult {
    let mut local = registry(1, TextLimits::default());
    let first = local.register(FONT.to_vec(), TextSettings::default())?;
    let second = local.register(FONT.to_vec(), TextSettings::default())?;
    let mut other = registry(2, TextLimits::default());
    let foreign = other.register(FONT.to_vec(), TextSettings::default())?;
    assert_ne!(first, second);
    assert_ne!(first, foreign);
    assert!(!local.contains(&foreign));
    assert!(!other.contains(&first));
    assert!(local.contains(&second));
    assert_eq!(
        HashSet::from([first.clone(), first, second, foreign]).len(),
        3
    );
    Ok(())
}

#[test]
fn failed_registration_preserves_count_and_actual_capacity_accounting() -> TestResult {
    let source = FONT.to_vec();
    let capacity = source.capacity();
    let mut fonts = registry(1, TextLimits::new(2, capacity));
    let first = fonts.register(source, TextSettings::default())?;
    assert!(matches!(
        fonts.register(FONT.to_vec(), TextSettings::default()),
        Err(TextError::FontByteLimitExceeded { retained, .. }) if retained == capacity
    ));
    assert_eq!(fonts.len(), 1);
    assert_eq!(fonts.font_bytes(), capacity);
    assert!(fonts.contains(&first));

    let mut spare = Vec::with_capacity(FONT.len() + 256);
    spare.extend_from_slice(FONT);
    let actual = spare.capacity();
    let mut too_small = registry(1, TextLimits::new(1, actual - 1));
    assert!(matches!(
        too_small.register(spare, TextSettings::default()),
        Err(TextError::FontByteLimitExceeded { incoming, .. }) if incoming == actual
    ));
    assert_eq!(too_small.len(), 0);
    assert_eq!(too_small.font_bytes(), 0);
    Ok(())
}

#[test]
fn font_count_and_parse_limits_reject_before_publication() -> TestResult {
    let mut disabled = registry(1, TextLimits::new(0, usize::MAX));
    assert!(matches!(
        disabled.register(Vec::new(), TextSettings::default()),
        Err(TextError::FontLimitExceeded { limit: 0 })
    ));
    let mut fonts = registry(1, TextLimits::default());
    let first = fonts.register(FONT.to_vec(), TextSettings::default())?;
    let before = fonts.font_bytes();
    assert!(matches!(
        fonts.register(vec![0; 12], TextSettings::default()),
        Err(TextError::Font(FontError::InvalidFont))
    ));
    assert!(matches!(
        fonts.register(
            FONT.to_vec(),
            TextSettings::default().with_font_budget(FontBudget::new(1, 65_535))
        ),
        Err(TextError::Font(FontError::BudgetExceeded {
            resource: FontBudgetResource::FontBytes,
            ..
        }))
    ));
    assert_eq!(fonts.len(), 1);
    assert_eq!(fonts.font_bytes(), before);
    assert!(fonts.contains(&first));
    Ok(())
}

#[test]
fn labels_use_engine_metrics_for_latin_cyrillic_and_combining_marks() -> TestResult {
    let font = font(TextSettings::default())?;
    for text in ["Hello, world!", "Привет, мир!", "AV ffi", "e\u{301}"] {
        let visual =
            ScreenTextVisual::new(font.clone(), text, LogicalScreenPosition::new(-10.0, 30.0))?;
        let expected = font.face().shape_line(
            text,
            &font.settings().style(1.0)?,
            &font.settings().layout_budget(),
        )?;
        assert_eq!(visual.text(), text);
        assert_eq!(visual.metrics().advance(), expected.advance());
        assert_eq!(visual.metrics().ascent(), expected.ascent());
        assert_eq!(visual.metrics().descent(), expected.descent());
        assert_eq!(visual.metrics().line_height(), expected.line_height());
        assert_eq!(visual.glyph_count(), expected.glyphs().len());
        assert!(visual.retained_text_bytes() >= text.len());
        assert_eq!(visual.tint(), Color::WHITE);
        assert_eq!(visual.layer(), Layer::DEFAULT);
        assert_eq!(visual.draw_order_depth(), 0.0);
    }
    Ok(())
}

#[test]
fn whitespace_and_empty_labels_retain_their_engine_metrics() -> TestResult {
    let empty = label("")?;
    assert_eq!(empty.glyph_count(), 0);
    assert_eq!(empty.metrics().advance(), 0.0);
    assert_eq!(empty.retained_text_bytes(), 0);
    let spaces = label("   ")?;
    assert!(spaces.metrics().advance() > 0.0);
    assert_eq!(spaces.glyph_count(), 3);
    assert!(spaces.metrics().line_height() > 0.0);
    Ok(())
}

#[test]
fn invalid_input_and_missing_glyphs_are_typed_errors() -> TestResult {
    let mut visual = label("Good")?;
    let before = visual.clone();
    for text in ["a\nb", "a\rb", "a\tb", "a\u{2028}b", "a\u{2029}b"] {
        assert!(matches!(
            visual.set_text(text),
            Err(TextError::Font(FontError::UnsupportedText { .. }))
        ));
        assert_eq!(visual, before);
    }
    assert!(matches!(
        visual.set_text("\u{10ffff}"),
        Err(TextError::Font(FontError::MissingGlyph { .. }))
    ));
    assert_eq!(visual, before);
    Ok(())
}

#[test]
fn equal_text_and_presentation_changes_share_prepared_storage() -> TestResult {
    let mut visual = label("Score: 123")?;
    let pointer = visual.text().as_ptr();
    let metrics = visual.metrics();
    let clone = visual.clone();
    assert_eq!(clone.text().as_ptr(), pointer);
    visual.set_text("Score: 123")?;
    visual.set_font(visual.font().clone())?;
    visual.set_position(LogicalScreenPosition::new(-20.0, 800.0))?;
    visual.set_alignment(TextAlignment::Center)?;
    visual.set_tint(Color::rgba(0.1, 0.2, 0.3, 0.5))?;
    visual.set_layer(Layer::new(7));
    visual.set_draw_order_depth(-3.0)?;
    assert_eq!(visual.text().as_ptr(), pointer);
    assert_eq!(visual.metrics(), metrics);
    assert_eq!(clone.position(), LogicalScreenPosition::new(100.0, 60.0));
    assert_eq!(clone.tint(), Color::WHITE);
    Ok(())
}

#[test]
fn changed_text_is_clone_independent_and_published_only_after_validation() -> TestResult {
    let mut visual = label("One")?;
    let before = visual.clone();
    let pointer = before.text().as_ptr();
    visual.set_text("Two much longer words")?;
    assert_eq!(before.text(), "One");
    assert_eq!(before.text().as_ptr(), pointer);
    assert_ne!(visual.text().as_ptr(), pointer);
    assert!(visual.metrics().advance() > before.metrics().advance());
    assert_eq!(visual.position(), before.position());
    Ok(())
}

#[test]
fn alignment_uses_advance_including_trailing_spaces() -> TestResult {
    let mut visual = label("AV ")?;
    let anchor = visual.position();
    let width = visual.metrics().advance();
    assert_eq!(visual.baseline_origin(), anchor);
    visual.set_alignment(TextAlignment::Center)?;
    assert_eq!(
        visual.baseline_origin(),
        LogicalScreenPosition::new(anchor.to_vec2().x() - width * 0.5, anchor.to_vec2().y())
    );
    visual.set_alignment(TextAlignment::Right)?;
    assert_eq!(
        visual.baseline_origin(),
        LogicalScreenPosition::new(anchor.to_vec2().x() - width, anchor.to_vec2().y())
    );
    assert_eq!(visual.position(), anchor);
    Ok(())
}

#[test]
fn invalid_presentation_setters_preserve_all_prior_fields() -> TestResult {
    let mut visual = label("Good")?;
    visual.set_alignment(TextAlignment::Right)?;
    visual.set_layer(Layer::new(4));
    let before = visual.clone();
    let pointer = visual.text().as_ptr();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        assert!(
            visual
                .set_position(LogicalScreenPosition::new(value, 30.0))
                .is_err()
        );
        assert_eq!(visual, before);
        assert!(
            visual
                .set_position(LogicalScreenPosition::new(30.0, value))
                .is_err()
        );
        assert_eq!(visual, before);
    }
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(visual.set_tint(Color::WHITE.with_alpha(value)).is_err());
        assert_eq!(visual, before);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(visual.set_draw_order_depth(value).is_err());
        assert_eq!(visual, before);
    }
    assert_eq!(visual.text().as_ptr(), pointer);
    Ok(())
}

#[test]
fn per_label_utf8_and_shaped_glyph_limits_are_independent() -> TestResult {
    let settings =
        TextSettings::default().with_layout_budget(TextLayoutBudget::new(6, 4, 100_000, 16_384));
    let font = font(settings)?;
    let mut visual = ScreenTextVisual::new(font, "okay", LogicalScreenPosition::new(0.0, 40.0))?;
    let before = visual.clone();
    assert!(matches!(
        visual.set_text("Привет"),
        Err(TextError::Font(FontError::BudgetExceeded {
            resource: FontBudgetResource::TextBytes,
            ..
        }))
    ));
    assert_eq!(visual, before);
    assert!(matches!(
        visual.set_text("abcde"),
        Err(TextError::Font(FontError::BudgetExceeded {
            resource: FontBudgetResource::ShapedGlyphs,
            ..
        }))
    ));
    assert_eq!(visual, before);
    Ok(())
}

#[test]
fn desktop_run_limits_are_checked_headlessly_including_spaces() -> TestResult {
    let run = GlyphRunBudget::new(1, GlyphRunBudget::RETAINED_BYTES_PER_GLYPH)?;
    let atlas = TextAtlasBudget::new(64, 64, 16, run)?;
    let font = font(TextSettings::default().with_atlas_budget(atlas))?;
    let mut visual = ScreenTextVisual::new(font, "a", LogicalScreenPosition::new(0.0, 40.0))?;
    let before = visual.clone();
    assert!(matches!(
        visual.set_text("  "),
        Err(TextError::RunBudgetExceeded { glyphs: 2, .. })
    ));
    assert_eq!(visual, before);
    visual.set_text("")?;
    assert_eq!(visual.glyph_count(), 0);
    Ok(())
}

#[test]
fn replacing_font_is_atomic_and_changes_metrics_only_on_success() -> TestResult {
    let mut fonts = registry(1, TextLimits::default());
    let original = fonts.register(FONT.to_vec(), TextSettings::new(24.0)?)?;
    let small = fonts.register(FONT.to_vec(), TextSettings::new(12.0)?)?;
    let restrictive = fonts.register(
        FONT.to_vec(),
        TextSettings::default().with_layout_budget(TextLayoutBudget::new(1, 1, 100_000, 16_384)),
    )?;
    let mut visual =
        ScreenTextVisual::new(original, "Hello", LogicalScreenPosition::new(30.0, 40.0))?;
    let before = visual.clone();
    assert!(visual.set_font(restrictive).is_err());
    assert_eq!(visual, before);
    visual.set_font(small.clone())?;
    assert_eq!(visual.font(), &small);
    assert_eq!(visual.text(), before.text());
    assert_eq!(visual.metrics().advance(), before.metrics().advance() * 0.5);
    assert_eq!(visual.position(), before.position());
    Ok(())
}

#[test]
fn logical_metrics_do_not_change_with_raster_dpi() -> TestResult {
    let font = font(TextSettings::default())?;
    let one = font.face().shape_line(
        "Привет AV",
        &font.settings().style(1.0)?,
        &font.settings().layout_budget(),
    )?;
    let fractional = font.face().shape_line(
        "Привет AV",
        &font.settings().style(1.25)?,
        &font.settings().layout_budget(),
    )?;
    assert_eq!(one.glyphs(), fractional.glyphs());
    assert_eq!(one.advance(), fractional.advance());
    assert_eq!(one.ascent(), fractional.ascent());
    assert_eq!(one.descent(), fractional.descent());
    Ok(())
}

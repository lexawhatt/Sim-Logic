use std::sync::Arc;

use bevy_ecs::prelude::Component;
use sim_engine::{
    Color, FontBudgetResource, FontError, GlyphRunBudget, Layer, LogicalScreenPosition, ShapedLine,
};

use crate::screen::ScreenVisualError;

use super::{TextError, TextFont};

/// Horizontal alignment of a line's typographic advance around its baseline anchor.
///
/// This aligns the entire line, not each glyph or its ink bounds. Trailing
/// whitespace contributes to advance. Alignment never changes shaping direction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextAlignment {
    /// The baseline origin starts at the anchor's x coordinate.
    #[default]
    Left,
    /// Half the line's advance lies on each side of the anchor.
    Center,
    /// The line's advance ends at the anchor's x coordinate.
    Right,
}

/// Headless single-line metrics returned by Sim;Engine's font shaper.
///
/// Every distance is in logical pixels and remains independent of display DPI.
/// These are typographic metrics, not exact rasterized ink or clipping bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    advance: f32,
    ascent: f32,
    descent: f32,
    line_height: f32,
    glyph_count: usize,
}

impl TextMetrics {
    /// Returns the horizontal pen advance, including whitespace.
    pub const fn advance(self) -> f32 {
        self.advance
    }

    /// Returns the font ascender above the baseline in logical pixels.
    pub const fn ascent(self) -> f32 {
        self.ascent
    }

    /// Returns the signed font descender, normally negative below the baseline.
    pub const fn descent(self) -> f32 {
        self.descent
    }

    /// Returns the font-recommended baseline spacing, not automatic wrapping.
    pub const fn line_height(self) -> f32 {
        self.line_height
    }

    /// Returns shaped output count including non-drawing whitespace glyphs.
    /// It may differ from the number of UTF-8 bytes or Unicode characters.
    pub const fn glyph_count(self) -> usize {
        self.glyph_count
    }

    fn from_line(line: &ShapedLine) -> Self {
        Self {
            advance: line.advance(),
            ascent: line.ascent(),
            descent: line.descent(),
            line_height: line.line_height(),
            glyph_count: line.glyphs().len(),
        }
    }
}

#[derive(Debug, PartialEq)]
struct PreparedText {
    text: String,
    metrics: TextMetrics,
    minimum_x: f32,
    maximum_x: f32,
    minimum_y: f32,
    maximum_y: f32,
}

impl PreparedText {
    fn new(font: &TextFont, text: &str) -> Result<Self, TextError> {
        let settings = font.settings();
        let line =
            font.face()
                .shape_line(text, &settings.style(1.0)?, &settings.layout_budget())?;
        let metrics = TextMetrics::from_line(&line);
        let run_budget = settings.atlas_budget().run_budget();
        let required = metrics
            .glyph_count
            .checked_mul(GlyphRunBudget::RETAINED_BYTES_PER_GLYPH);
        if metrics.glyph_count > run_budget.max_glyphs()
            || required.is_none_or(|bytes| bytes > run_budget.max_retained_bytes())
        {
            return Err(TextError::RunBudgetExceeded {
                glyphs: metrics.glyph_count,
                max_glyphs: run_budget.max_glyphs(),
                required_bytes: required.unwrap_or(usize::MAX),
                max_retained_bytes: run_budget.max_retained_bytes(),
            });
        }
        let mut owned = String::new();
        owned
            .try_reserve_exact(text.len())
            .map_err(|source| TextError::AllocationFailed { source })?;
        let limit = settings.layout_budget().max_text_bytes();
        if owned.capacity() > limit {
            return Err(TextError::Font(FontError::BudgetExceeded {
                resource: FontBudgetResource::TextBytes,
                required: owned.capacity(),
                limit,
            }));
        }
        owned.push_str(text);
        let mut result = Self {
            text: owned,
            metrics,
            minimum_x: metrics.advance.min(0.0),
            maximum_x: metrics.advance.max(0.0),
            minimum_y: -metrics.ascent,
            maximum_y: -metrics.descent,
        };
        for glyph in line.glyphs() {
            result.minimum_x = result.minimum_x.min(glyph.logical_x());
            result.maximum_x = result.maximum_x.max(glyph.logical_x());
            result.minimum_y = result.minimum_y.min(glyph.logical_y());
            result.maximum_y = result.maximum_y.max(glyph.logical_y());
        }
        Ok(result)
    }
}

/// One validated single-line label anchored to a logical-screen baseline.
///
/// Screen coordinates start at the content area's top-left and increase down
/// and right. The y coordinate is the baseline, not the upper edge of the
/// letters. World cameras and fixed-step interpolation do not move this label.
/// It requires no world Transform and may coexist with other visual components.
///
/// Construction and changed text/font setters use Engine's CPU font shaper.
/// Cloning shares immutable text and font data; moving, tinting, ordering, and
/// aligning do not shape, rasterize, or upload glyphs. A failed setter leaves
/// every prior field intact. No old string history is retained by this value.
/// Clones held elsewhere can of course retain earlier immutable strings.
///
/// Extraction excludes disabled entities and validates font provenance before
/// publishing a complete snapshot. Screen layer, depth, and entity identity
/// determine painter order; a same-entity rectangle/image/text tie uses that
/// order. No wrapping, font fallback, hit testing, input capture, or text editing
/// is implied. Missing glyphs and line separators are errors, not replacements.
///
/// Raster size, atlas space, and GPU allocation are checked later by desktop
/// presentation. CPU validation alone cannot guarantee a glyph fits every DPI
/// or device. Engine currently reshapes changed text when preparing its GPU run;
/// unchanged desktop runs reuse their layout and buffers.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct ScreenTextVisual {
    font: TextFont,
    prepared: Arc<PreparedText>,
    position: LogicalScreenPosition,
    alignment: TextAlignment,
    tint: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl ScreenTextVisual {
    /// Prepares a left-aligned white label on the default layer at depth zero.
    ///
    /// Position is a finite logical-pixel baseline anchor. Negative/offscreen
    /// positions are valid when aligned bounds remain representable. Empty
    /// text is valid; whitespace retains advance even if it draws no glyphs.
    /// No window, rasterization, or GPU operation occurs here.
    pub fn new(
        font: TextFont,
        text: &str,
        position: LogicalScreenPosition,
    ) -> Result<Self, TextError> {
        validate_position(position)?;
        let prepared = Arc::new(PreparedText::new(&font, text)?);
        let result = Self {
            font,
            prepared,
            position,
            alignment: TextAlignment::Left,
            tint: Color::WHITE,
            layer: Layer::DEFAULT,
            draw_order_depth: 0.0,
        };
        result.validate()?;
        Ok(result)
    }

    /// Returns the opaque application font registration shared by this label.
    pub const fn font(&self) -> &TextFont {
        &self.font
    }

    /// Returns the original UTF-8 single line without allocating or reshaping.
    pub fn text(&self) -> &str {
        &self.prepared.text
    }

    /// Returns DPI-independent typographic metrics from the latest successful preparation.
    pub fn metrics(&self) -> TextMetrics {
        self.prepared.metrics
    }

    /// Returns shaped glyph count, conservatively including whitespace.
    pub fn glyph_count(&self) -> usize {
        self.metrics().glyph_count()
    }

    /// Returns the immutable retained UTF-8 String capacity in bytes.
    ///
    /// A clone shares these bytes. Extraction may conservatively count each
    /// active label separately when enforcing its aggregate text-byte limit.
    /// Font bytes and fixed-sized prepared metadata are excluded.
    pub fn retained_text_bytes(&self) -> usize {
        self.prepared.text.capacity()
    }

    /// Returns the logical-pixel baseline anchor before horizontal alignment.
    pub const fn position(&self) -> LogicalScreenPosition {
        self.position
    }

    /// Returns the actual logical baseline origin after horizontal alignment.
    /// Typographic advance, including trailing spaces, determines the offset.
    pub fn baseline_origin(&self) -> LogicalScreenPosition {
        let offset = match self.alignment {
            TextAlignment::Left => 0.0,
            TextAlignment::Center => self.metrics().advance() * 0.5,
            TextAlignment::Right => self.metrics().advance(),
        };
        let position = self.position.to_vec2();
        LogicalScreenPosition::new(position.x() - offset, position.y())
    }

    /// Returns how the line's advance is aligned around its baseline anchor.
    pub const fn alignment(&self) -> TextAlignment {
        self.alignment
    }

    /// Returns normalized straight-linear RGBA multiplied by glyph coverage.
    pub const fn tint(&self) -> Color {
        self.tint
    }

    /// Returns the primary painter-order layer within screen content.
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Returns finite within-layer ordering depth, not camera projection depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.draw_order_depth
    }

    /// Replaces the line only after complete bounded shaping and geometry validation.
    ///
    /// Exact equal UTF-8 is a no-op. Any failure preserves text, metrics,
    /// shared storage, and presentation fields. Old storage is released when
    /// no earlier clone or published snapshot references it.
    pub fn set_text(&mut self, text: &str) -> Result<(), TextError> {
        if self.text() == text {
            return Ok(());
        }
        let mut candidate = self.clone();
        candidate.prepared = Arc::new(PreparedText::new(&self.font, text)?);
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Selects a registered font and reparses the current line before swapping.
    ///
    /// The same registration is a no-op. Missing glyphs, budgets, or invalid
    /// aligned geometry preserve the old font and every other field. Issuing
    /// application identity is checked by extraction, not by this standalone value.
    pub fn set_font(&mut self, font: TextFont) -> Result<(), TextError> {
        if self.font == font {
            return Ok(());
        }
        let mut candidate = self.clone();
        candidate.prepared = Arc::new(PreparedText::new(&font, self.text())?);
        candidate.font = font;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Moves the logical baseline anchor without shaping or changing retained text.
    /// Non-finite or unrepresentable aligned bounds leave the value unchanged.
    pub fn set_position(&mut self, position: LogicalScreenPosition) -> Result<(), TextError> {
        let mut candidate = self.clone();
        candidate.position = position;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Changes advance-based alignment without shaping or rasterization.
    /// An unrepresentable adjusted baseline leaves every field unchanged.
    pub fn set_alignment(&mut self, alignment: TextAlignment) -> Result<(), TextError> {
        let mut candidate = self.clone();
        candidate.alignment = alignment;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }

    /// Changes normalized straight-linear RGBA tint atomically, without reshaping.
    pub fn set_tint(&mut self, tint: Color) -> Result<(), TextError> {
        if !tint.is_normalized() {
            return Err(ScreenVisualError::InvalidColor { value: tint }.into());
        }
        self.tint = tint;
        Ok(())
    }

    /// Replaces the primary screen painter-order layer without preparing text again.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Replaces finite within-layer painter depth; negative values are allowed.
    /// Non-finite input preserves the current depth and every other field.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), TextError> {
        if !depth.is_finite() {
            return Err(ScreenVisualError::InvalidDrawOrderDepth { value: depth }.into());
        }
        self.draw_order_depth = depth;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), TextError> {
        validate_position(self.position)?;
        if !self.tint.is_normalized() {
            return Err(ScreenVisualError::InvalidColor { value: self.tint }.into());
        }
        if !self.draw_order_depth.is_finite() {
            return Err(ScreenVisualError::InvalidDrawOrderDepth {
                value: self.draw_order_depth,
            }
            .into());
        }
        let baseline = self.baseline_origin().to_vec2();
        let p = &self.prepared;
        if !baseline.is_finite()
            || !offset_is_representable(baseline.x(), p.minimum_x)
            || !offset_is_representable(baseline.x(), p.maximum_x)
            || !offset_is_representable(baseline.y(), p.minimum_y)
            || !offset_is_representable(baseline.y(), p.maximum_y)
        {
            return Err(TextError::InvalidBounds {
                position: self.position,
            });
        }
        Ok(())
    }
}

fn validate_position(position: LogicalScreenPosition) -> Result<(), TextError> {
    if !position.is_finite() {
        return Err(ScreenVisualError::InvalidPosition { value: position }.into());
    }
    Ok(())
}

fn offset_is_representable(origin: f32, offset: f32) -> bool {
    let result = origin + offset;
    result.is_finite() && (offset == 0.0 || result != origin)
}

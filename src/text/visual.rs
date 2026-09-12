use std::{fmt, sync::Arc};

use bevy_ecs::prelude::Component;
use sim_engine::{Color, GlyphRunBudget, Layer, LogicalScreenPosition, ShapedLine};

use crate::screen::ScreenVisualError;

use super::{TextError, TextFont, TextPreparationSession};

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

struct PreparedText {
    line: ShapedLine,
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
        Self::from_line(font, line)
    }

    fn from_line(font: &TextFont, line: ShapedLine) -> Result<Self, TextError> {
        let settings = font.settings();
        line.validate_for(
            font.face(),
            &settings.style(1.0)?,
            &settings.layout_budget(),
        )?;
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
        let mut result = Self {
            line,
            metrics,
            minimum_x: metrics.advance.min(0.0),
            maximum_x: metrics.advance.max(0.0),
            minimum_y: -metrics.ascent,
            maximum_y: -metrics.descent,
        };
        for glyph in result.line.glyphs() {
            result.minimum_x = result.minimum_x.min(glyph.logical_x());
            result.maximum_x = result.maximum_x.max(glyph.logical_x());
            result.minimum_y = result.minimum_y.min(glyph.logical_y());
            result.maximum_y = result.maximum_y.max(glyph.logical_y());
        }
        Ok(result)
    }
}

impl fmt::Debug for PreparedText {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // ShapedLine also retains its font; avoid dumping that shared source.
        formatter
            .debug_struct("PreparedText")
            .field("text", &self.line.text())
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl PartialEq for PreparedText {
    fn eq(&self, other: &Self) -> bool {
        self.line.text() == other.line.text()
            && self.line.style() == other.line.style()
            && self.line.glyphs() == other.line.glyphs()
            && self.metrics == other.metrics
            && self.minimum_x == other.minimum_x
            && self.maximum_x == other.maximum_x
            && self.minimum_y == other.minimum_y
            && self.maximum_y == other.maximum_y
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
/// or device. Desktop preparation passes this retained CPU line directly to
/// Engine at scale 1.0. Other display scales require a DPI-specific line because
/// Engine's prepared-line provenance includes exact DPI; that line is shaped
/// only on a changed label or resource rebuild, never for an unchanged run.
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
        let prepared = PreparedText::new(&font, text)?;
        Self::from_prepared(font, prepared, position)
    }

    /// Creates a label using a caller-owned reusable shaping session.
    ///
    /// The session fixes one registered font and logical size at scale 1.0.
    /// It reuses parsed shaping state, not mutable storage held by older labels.
    /// The returned value owns its line and does not borrow the session.
    pub fn new_with_session(
        session: &mut TextPreparationSession<'_>,
        text: &str,
        position: LogicalScreenPosition,
    ) -> Result<Self, TextError> {
        validate_position(position)?;
        let font = session.font().clone();
        let prepared = PreparedText::from_line(&font, session.shape_line(text)?)?;
        Self::from_prepared(font, prepared, position)
    }

    fn from_prepared(
        font: TextFont,
        prepared: PreparedText,
        position: LogicalScreenPosition,
    ) -> Result<Self, TextError> {
        let prepared = Arc::new(prepared);
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
        self.prepared.line.text()
    }

    /// Returns immutable Engine shaping output, including exact font/style provenance.
    ///
    /// Logical labels prepare at scale 1.0. A different-DPI atlas must not consume
    /// this line as though its style matched; desktop handles that rebuild itself.
    /// The UTF-8 and glyph metadata are shared with every clone and snapshot.
    pub fn shaped_line(&self) -> &ShapedLine {
        &self.prepared.line
    }

    /// Returns DPI-independent typographic metrics from the latest successful preparation.
    pub fn metrics(&self) -> TextMetrics {
        self.prepared.metrics
    }

    /// Returns shaped glyph count, conservatively including whitespace.
    pub fn glyph_count(&self) -> usize {
        self.metrics().glyph_count()
    }

    /// Returns a conservative upper bound on retained UTF-8 storage in bytes.
    ///
    /// Since Engine 0.4 owns the UTF-8 together with glyph metadata, this includes
    /// both capacities, exactly as [`Self::retained_layout_bytes`]. It no longer
    /// measures the String allocation alone. Font and fixed metadata are excluded.
    /// Extraction's text-byte allowance independently counts `text().len()`.
    pub fn retained_text_bytes(&self) -> usize {
        self.retained_layout_bytes()
    }

    /// Returns retained UTF-8 plus shaped-glyph Vec capacity bytes.
    ///
    /// This is Engine's actual capacity accounting, shared by clones, excluding
    /// font storage, allocator overhead and this value's fixed metadata. It is
    /// not a measurement of desktop atlas or GPU memory.
    pub fn retained_layout_bytes(&self) -> usize {
        self.prepared.line.allocation_bytes()
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

    /// Changes text using reusable shaping state for this exact font registration.
    ///
    /// A foreign registration is rejected even when UTF-8 is unchanged. An exact
    /// text match with the correct session performs no shaping. Any error preserves
    /// all visual fields and earlier snapshots; session scratch may have changed.
    /// Successful changes own fresh line storage, so this is not a zero-allocation
    /// update promise. The borrowed session never becomes part of the component.
    pub fn set_text_with_session(
        &mut self,
        session: &mut TextPreparationSession<'_>,
        text: &str,
    ) -> Result<(), TextError> {
        if session.font() != &self.font {
            return Err(sim_engine::ShapedLineError::FontMismatch.into());
        }
        if self.text() == text {
            return Ok(());
        }
        let prepared = PreparedText::from_line(&self.font, session.shape_line(text)?)?;
        let mut candidate = self.clone();
        candidate.prepared = Arc::new(prepared);
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

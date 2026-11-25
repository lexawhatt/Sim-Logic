//! CPU layout for caller-owned page surfaces; no widget or atlas ownership.
use super::{TextError, TextPreparationSession};
use sim_engine::{ShapedCaret, ShapedGeometryError, ShapedLine, ShapedSelection, TextDirection};
use std::{error::Error, fmt, ops::Range};

/// One immutable wrapped line, with source UTF-8 offsets and logical baseline.
#[derive(Debug)]
pub struct SurfaceTextLine {
    /// Source range, excluding an explicit newline. Spaces remain in the range.
    source: Range<usize>,
    /// Baseline y in logical pixels, positive downward from the page top.
    baseline: f32,
    /// Owned shaping snapshot; shares immutable font bytes.
    shaped: ShapedLine,
}
/// Rejected page layout or caret lookup; no previous layout is modified.
#[derive(Debug)]
pub enum SurfaceTextError {
    /// Invalid width, more than 4096 UTF-8 bytes, more than 128 lines,
    /// or a page rectangle not representable as finite logical f32 coordinates.
    Limit,
    /// The bounded helper supports explicitly left-to-right text only.
    Direction,
    /// A word cannot fit the requested width; it is never silently clipped.
    WordTooWide,
    /// The requested source byte is outside this wrapped line.
    SourceOffset {
        /// Requested document-relative byte offset.
        byte_offset: usize,
        /// Valid document-relative range, including the end boundary.
        source: Range<usize>,
    },
    /// Engine rejected a cluster-interior, invalid UTF-8 or reversed selection.
    Geometry {
        /// Add this document offset to Engine's line-relative error offsets.
        source_start: usize,
        /// Exact structured Engine error, including indivisible cluster ranges.
        error: ShapedGeometryError,
    },
    /// Font shaping rejected the text.
    Text(TextError),
}
impl fmt::Display for SurfaceTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "surface text layout rejected: {self:?}")
    }
}
impl Error for SurfaceTextError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry { error, .. } => Some(error),
            Self::Text(error) => Some(error),
            _ => None,
        }
    }
}
impl From<TextError> for SurfaceTextError {
    fn from(error: TextError) -> Self {
        Self::Text(error)
    }
}

/// Bounded word wrapping for Latin/Cyrillic left-to-right pages.
/// Wraps at ASCII spaces and explicit newlines; no bidi paragraph algorithm,
/// hyphenation or grapheme editor is implied. At most 4096 input bytes and 128
/// lines are accepted. Work is bounded quadratic in input bytes because candidate
/// lines are reshaped for exact advances. Returned snapshots retain font storage;
/// atlas rebuilds do not invalidate them. Replace a prior layout only on success.
pub fn layout_surface_text(
    session: &mut TextPreparationSession<'_>,
    text: &str,
    width: f32,
) -> Result<Vec<SurfaceTextLine>, SurfaceTextError> {
    if !width.is_finite() || width <= 0.0 || text.len() > 4096 {
        return Err(SurfaceTextError::Limit);
    }
    if session.font().settings().direction() != TextDirection::Ltr {
        return Err(SurfaceTextError::Direction);
    }
    let mut lines = Vec::new();
    let mut base = 0;
    for paragraph in text.split('\n') {
        let mut start = 0;
        let mut end = 0;
        let mut accepted: Option<ShapedLine> = None;
        for word in paragraph.split_inclusive(' ') {
            let next = end + word.len();
            let candidate = session.shape_line(&paragraph[start..next])?;
            if candidate.advance() > width {
                let Some(previous) = accepted.take() else {
                    return Err(SurfaceTextError::WordTooWide);
                };
                push_line(&mut lines, base + start..base + end, previous)?;
                start = end;
                let candidate = session.shape_line(&paragraph[start..next])?;
                if candidate.advance() > width {
                    return Err(SurfaceTextError::WordTooWide);
                }
                accepted = Some(candidate);
            } else {
                accepted = Some(candidate);
            }
            end = next;
        }
        let shaped = match accepted {
            Some(line) => line,
            None => session.shape_line("")?,
        };
        push_line(&mut lines, base + start..base + end, shaped)?;
        base += paragraph.len() + 1;
    }
    Ok(lines)
}
fn push_line(
    lines: &mut Vec<SurfaceTextLine>,
    source: Range<usize>,
    shaped: ShapedLine,
) -> Result<(), SurfaceTextError> {
    if lines.len() == 128 {
        return Err(SurfaceTextError::Limit);
    }
    let top = lines.last().map_or(0.0, |line| {
        line.baseline - line.shaped.ascent() + line.shaped.line_height()
    });
    let baseline = top + shaped.ascent();
    if !baseline.is_finite() {
        return Err(SurfaceTextError::Limit);
    }
    lines.push(SurfaceTextLine {
        source,
        baseline,
        shaped,
    });
    Ok(())
}
impl SurfaceTextLine {
    /// Source UTF-8 range in the complete document, excluding an explicit newline.
    pub fn source(&self) -> Range<usize> {
        self.source.clone()
    }

    /// Baseline y in logical page pixels, positive downward.
    pub const fn baseline(&self) -> f32 {
        self.baseline
    }

    /// Immutable Engine line with font/style provenance and allocation-free
    /// cluster traversal in visual order. Its offsets are relative to this line.
    pub const fn shaped(&self) -> &ShapedLine {
        &self.shaped
    }

    fn local_offset(&self, byte_offset: usize) -> Result<usize, SurfaceTextError> {
        if byte_offset < self.source.start || byte_offset > self.source.end {
            return Err(SurfaceTextError::SourceOffset {
                byte_offset,
                source: self.source.clone(),
            });
        }
        Ok(byte_offset - self.source.start)
    }
    fn geometry_error(&self, error: ShapedGeometryError) -> SurfaceTextError {
        SurfaceTextError::Geometry {
            source_start: self.source.start,
            error,
        }
    }

    /// Looks up an exact pen caret from a document-relative UTF-8 byte offset.
    /// The borrowed Engine result retains line/font/style provenance; its own
    /// byte_offset is line-relative. Position is baseline-relative logical pixels.
    /// Only whole shaped-cluster boundaries are accepted, including line end.
    /// Ligature/combining interiors reject without snapping. No allocation occurs.
    pub fn caret_at(&self, source_byte: usize) -> Result<ShapedCaret<'_>, SurfaceTextError> {
        self.shaped
            .caret_at(self.local_offset(source_byte)?)
            .map_err(|error| self.geometry_error(error))
    }

    /// Exact horizontal insertion position, excluding glyph placement offsets.
    /// Input uses document bytes; output uses baseline-relative logical pixels.
    pub fn caret_x(&self, source_byte: usize) -> Result<f32, SurfaceTextError> {
        Ok(self.caret_at(source_byte)?.position().logical_x())
    }

    /// Borrows whole-cluster selection spans in visual order without allocation.
    /// Input uses ascending document offsets; returned ranges are line-relative.
    /// Empty ranges yield no spans. Invalid boundaries and reversed ranges reject.
    pub fn selection(&self, source: Range<usize>) -> Result<ShapedSelection<'_>, SurfaceTextError> {
        self.shaped
            .selection(self.local_offset(source.start)?..self.local_offset(source.end)?)
            .map_err(|error| self.geometry_error(error))
    }

    /// Returns [left, top, width, height] of the exact horizontal pen selection.
    /// Top/height use this line's typographic metrics in logical page pixels,
    /// not glyph ink. Empty selection has zero width at its valid caret.
    /// This wrapped-line helper is LTR; it does not combine bidi paragraph runs.
    pub fn selection_bounds(&self, source: Range<usize>) -> Result<[f32; 4], SurfaceTextError> {
        let spans = self.selection(source.clone())?;
        let x = self.caret_x(source.start)?;
        let mut left = x;
        let mut right = x;
        for span in spans {
            left = left
                .min(span.pen_start().logical_x())
                .min(span.pen_end().logical_x());
            right = right
                .max(span.pen_start().logical_x())
                .max(span.pen_end().logical_x());
        }
        let width = (f64::from(right) - f64::from(left)) as f32;
        let top = (f64::from(self.baseline) - f64::from(self.shaped.ascent())) as f32;
        let height = self.shaped.line_height();
        if !left.is_finite()
            || !top.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width < 0.0
            || height < 0.0
        {
            return Err(SurfaceTextError::Limit);
        }
        Ok([left, top, width, height])
    }
}

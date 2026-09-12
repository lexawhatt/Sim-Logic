//! Caller-owned shaping state; labels remain independent immutable snapshots.

use sim_engine::{ShapedLine, TextShapingSession};

use super::{TextError, TextFont};

/// Reusable CPU shaping state borrowed from one application font registration.
///
/// Create with [`TextFont::shaping_session`], then use
/// [`super::ScreenTextVisual::new_with_session`] or
/// [`super::ScreenTextVisual::set_text_with_session`]. Each successful changed
/// label owns new UTF-8/glyph storage so old clones and published frames remain
/// immutable. Only parsed shaping state and one property-matched plan are reused;
/// this does not promise allocation-free changing labels.
///
/// The font, logical size, direction and work limits cannot change in a session.
/// Raster DPI is 1.0 for canonical labels. Desktop DPI rebuilds are separate.
/// This object does not own the font, create a GPU resource, or use global state.
pub struct TextPreparationSession<'font> {
    font: &'font TextFont,
    engine: TextShapingSession<'font>,
}

impl<'font> TextPreparationSession<'font> {
    pub(super) fn new(font: &'font TextFont) -> Result<Self, TextError> {
        let settings = font.settings();
        Ok(Self {
            font,
            engine: TextShapingSession::new(
                font.face(),
                settings.style(1.0)?,
                settings.layout_budget(),
            )?,
        })
    }

    /// Returns the exact borrowed application registration, not just its source bytes.
    pub const fn font(&self) -> &TextFont {
        self.font
    }

    /// Returns Engine-owned scratch capacity bytes, excluding returned labels,
    /// source font, and dependency-owned parsed face/plan/buffer allocations.
    pub fn allocation_bytes(&self) -> usize {
        self.engine.allocation_bytes()
    }

    /// Returns the number of retained property-matched plans, always zero or one.
    pub fn cached_plan_count(&self) -> usize {
        self.engine.cached_plan_count()
    }

    /// Releases shaping scratch and the cached plan without invalidating labels.
    /// The borrowed parsed font face remains available for subsequent work.
    pub fn clear_scratch(&mut self) {
        self.engine.clear_scratch();
    }

    pub(super) fn shape_line(&mut self, text: &str) -> Result<ShapedLine, TextError> {
        Ok(self.engine.shape_line(text)?)
    }
}

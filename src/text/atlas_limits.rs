//! GPU-independent configuration values for the retained text bridge.

use super::TextError;

/// Count and nominal retained-byte limits for one prepared glyph run.
/// These are configuration values, not allocated CPU/GPU storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRunLimits {
    glyphs: usize,
    bytes: usize,
}

impl TextRunLimits {
    /// Engine 0.4.1's positioned-glyph + sprite + conversion record accounting.
    /// A parity test against Engine's public constant guards dependency upgrades.
    pub const RETAINED_BYTES_PER_GLYPH: usize = 132;
    /// Requires at least one glyph and one complete retained record.
    pub fn new(max_glyphs: usize, max_retained_bytes: usize) -> Result<Self, TextError> {
        if max_glyphs == 0 || max_retained_bytes < Self::RETAINED_BYTES_PER_GLYPH {
            return Err(TextError::InvalidAtlasLimits);
        }
        Ok(Self {
            glyphs: max_glyphs,
            bytes: max_retained_bytes,
        })
    }
    /// Returns maximum glyph count, conservatively including whitespace.
    pub const fn max_glyphs(self) -> usize {
        self.glyphs
    }
    /// Returns the retained run-byte ceiling, excluding atlas texels.
    pub const fn max_retained_bytes(self) -> usize {
        self.bytes
    }
}
impl Default for TextRunLimits {
    fn default() -> Self {
        Self {
            glyphs: 100_000,
            bytes: 32 * 1024 * 1024,
        }
    }
}

/// Headless configuration for a per-style desktop atlas and its glyph runs.
/// Shared font source storage is separate. Creating this value allocates nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextAtlasLimits {
    width: u32,
    height: u32,
    glyphs: usize,
    run: TextRunLimits,
}

impl TextAtlasLimits {
    /// Requires dimensions at least 5, a 1..=65535 glyph cache, and representable
    /// complete RGBA texel storage. Actual device limits are checked on desktop.
    pub fn new(
        width: u32,
        height: u32,
        max_cached_glyphs: usize,
        run: TextRunLimits,
    ) -> Result<Self, TextError> {
        if width < 5
            || height < 5
            || !(1..=65535).contains(&max_cached_glyphs)
            || (width as usize)
                .checked_mul(height as usize)
                .and_then(|area| area.checked_mul(4))
                .is_none()
        {
            return Err(TextError::InvalidAtlasLimits);
        }
        Ok(Self {
            width,
            height,
            glyphs: max_cached_glyphs,
            run,
        })
    }
    /// Returns physical atlas width.
    pub const fn width(self) -> u32 {
        self.width
    }
    /// Returns physical atlas height.
    pub const fn height(self) -> u32 {
        self.height
    }
    /// Returns cached glyph count limit (including spacing glyphs).
    pub const fn max_cached_glyphs(self) -> usize {
        self.glyphs
    }
    /// Returns independent per-run limits.
    pub const fn run_budget(self) -> TextRunLimits {
        self.run
    }

    #[cfg(feature = "text")]
    pub(crate) const fn from_engine(budget: sim_engine::TextAtlasBudget) -> Self {
        Self {
            width: budget.width(),
            height: budget.height(),
            glyphs: budget.max_cached_glyphs(),
            run: TextRunLimits {
                glyphs: budget.run_budget().max_glyphs(),
                bytes: budget.run_budget().max_retained_bytes(),
            },
        }
    }
    #[cfg(feature = "text")]
    pub(crate) fn engine_budget(self) -> Result<sim_engine::TextAtlasBudget, TextError> {
        let run = sim_engine::GlyphRunBudget::new(self.run.glyphs, self.run.bytes)
            .map_err(|_| TextError::InvalidAtlasLimits)?;
        sim_engine::TextAtlasBudget::new(self.width, self.height, self.glyphs, run)
            .map_err(|_| TextError::InvalidAtlasLimits)
    }
}
impl Default for TextAtlasLimits {
    fn default() -> Self {
        Self {
            width: 1024,
            height: 1024,
            glyphs: 4096,
            run: TextRunLimits::default(),
        }
    }
}

#[cfg(all(test, feature = "text"))]
mod tests {
    use super::*;
    #[test]
    fn cpu_accounting_and_defaults_match_the_pinned_engine() -> crate::LogicResult {
        assert_eq!(
            TextRunLimits::RETAINED_BYTES_PER_GLYPH,
            sim_engine::GlyphRunBudget::RETAINED_BYTES_PER_GLYPH
        );
        assert_eq!(
            TextAtlasLimits::default().engine_budget()?,
            sim_engine::TextAtlasBudget::default()
        );
        for (glyphs, bytes) in [(0, 132), (1, 131), (1, 132), (10, 132), (3, usize::MAX)] {
            assert_eq!(
                TextRunLimits::new(glyphs, bytes).is_ok(),
                sim_engine::GlyphRunBudget::new(glyphs, bytes).is_ok()
            );
        }
        Ok(())
    }
}

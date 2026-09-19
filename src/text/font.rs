use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

use sim_engine::FontFace;

use crate::identity::ApplicationId;

use super::{TextError, TextLimits, TextPreparationSession, TextSettings};

/// Opaque handle to one application-owned font and immutable size configuration.
///
/// Cloning shares parsed font bytes and settings without duplicating them.
/// Equal registrations still issue different identities. A handle survives
/// World replacement within its issuing application, but is not a file path,
/// persistent key, or GPU resource. Foreign handles are rejected by extraction.
/// Keeping a clone after application shutdown retains its shared font bytes.
#[derive(Clone)]
pub struct TextFont {
    application: ApplicationId,
    slot: usize,
    data: Arc<FontData>,
}

#[derive(Debug)]
struct FontData {
    face: FontFace,
    face_slot: usize,
    settings: TextSettings,
}

impl TextFont {
    /// Reports shared parsed face/source storage, independent of size/style identity.
    /// Equal file contents loaded separately are not treated as shared storage.
    pub fn shares_face_with(&self, other: &Self) -> bool {
        self.application == other.application && self.data.face_slot == other.data.face_slot
    }
    /// Returns immutable logical size, shaping policy, and preparation budgets.
    pub fn settings(&self) -> TextSettings {
        self.data.settings
    }

    /// Returns the referenced source-font Vec capacity in bytes. Shared styles
    /// report the same source; application accounting charges that source once.
    /// Parsed dependency metadata and allocator overhead are excluded.
    pub fn font_bytes(&self) -> usize {
        self.data.face.allocation_bytes()
    }

    /// Borrows this registration for reusable CPU label shaping at logical scale 1.0.
    ///
    /// The caller owns the session lifetime; no global cache, interior lock or
    /// self-referencing component is created. This parses shaping state but does
    /// not allocate an atlas or GPU resource. Font/layout limits stay fixed.
    pub fn shaping_session(&self) -> Result<TextPreparationSession<'_>, TextError> {
        TextPreparationSession::new(self)
    }

    pub(crate) fn face(&self) -> &FontFace {
        &self.data.face
    }

    #[cfg(any(all(feature = "desktop", feature = "text"), test))]
    pub(crate) const fn slot(&self) -> usize {
        self.slot
    }
}

impl fmt::Debug for TextFont {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never dump parsed font internals or hundreds of kilobytes of source
        // data into a visual's error report.
        formatter
            .debug_struct("TextFont")
            .field("application", &self.application)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

impl PartialEq for TextFont {
    fn eq(&self, other: &Self) -> bool {
        self.application == other.application && self.slot == other.slot
    }
}

impl Eq for TextFont {}

impl Hash for TextFont {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.application.hash(state);
        self.slot.hash(state);
    }
}

#[derive(Debug)]
pub(crate) struct TextRegistry {
    application: ApplicationId,
    limits: TextLimits,
    fonts: Vec<TextFont>,
    font_bytes: usize,
}

impl TextRegistry {
    pub(crate) const fn new(application: ApplicationId, limits: TextLimits) -> Self {
        Self {
            application,
            limits,
            fonts: Vec::new(),
            font_bytes: 0,
        }
    }

    pub(crate) fn register(
        &mut self,
        bytes: Vec<u8>,
        settings: TextSettings,
    ) -> Result<TextFont, TextError> {
        settings.style(1.0)?;
        if self.fonts.len() >= self.limits.max_fonts() {
            return Err(TextError::FontLimitExceeded {
                limit: self.limits.max_fonts(),
            });
        }
        let incoming = bytes.capacity();
        if incoming > self.limits.max_font_bytes().saturating_sub(self.font_bytes) {
            return Err(TextError::FontByteLimitExceeded {
                limit: self.limits.max_font_bytes(),
                retained: self.font_bytes,
                incoming,
            });
        }
        let face = FontFace::from_bytes(bytes, settings.font_budget())?;
        self.fonts
            .try_reserve_exact(1)
            .map_err(|source| TextError::AllocationFailed { source })?;
        let font = TextFont {
            application: self.application,
            slot: self.fonts.len(),
            data: Arc::new(FontData {
                face,
                settings,
                face_slot: self.fonts.len(),
            }),
        };
        self.font_bytes += incoming;
        self.fonts.push(font.clone());
        Ok(font)
    }

    pub(crate) fn register_style(
        &mut self,
        source: &TextFont,
        settings: TextSettings,
    ) -> Result<TextFont, TextError> {
        if !self.contains(source) {
            return Err(TextError::ForeignFont);
        }
        settings.style(1.0)?;
        if self.fonts.len() >= self.limits.max_fonts() {
            return Err(TextError::FontLimitExceeded {
                limit: self.limits.max_fonts(),
            });
        }
        let face = source.face();
        for (resource, requested, limit) in [
            (
                sim_engine::FontBudgetResource::FontBytes,
                face.allocation_bytes(),
                settings.font_budget().max_font_bytes(),
            ),
            (
                sim_engine::FontBudgetResource::FontGlyphs,
                face.glyph_count(),
                settings.font_budget().max_font_glyphs(),
            ),
        ] {
            if requested > limit {
                return Err(sim_engine::FontError::BudgetExceeded {
                    resource,
                    required: requested,
                    limit,
                }
                .into());
            }
        }
        self.fonts
            .try_reserve_exact(1)
            .map_err(|source| TextError::AllocationFailed { source })?;
        let font = TextFont {
            application: self.application,
            slot: self.fonts.len(),
            data: Arc::new(FontData {
                face: face.clone(),
                settings,
                face_slot: source.data.face_slot,
            }),
        };
        self.fonts.push(font.clone());
        // Engine's FontFace clone shares its parsed face and original byte Vec.
        // Charge neither source bytes nor parsing a second time; atlases remain per style.
        Ok(font)
    }

    pub(crate) fn contains(&self, font: &TextFont) -> bool {
        font.application == self.application
            && self
                .fonts
                .get(font.slot)
                .is_some_and(|stored| stored == font && Arc::ptr_eq(&stored.data, &font.data))
    }

    pub(crate) fn len(&self) -> usize {
        self.fonts.len()
    }

    pub(crate) const fn font_bytes(&self) -> usize {
        self.font_bytes
    }
}

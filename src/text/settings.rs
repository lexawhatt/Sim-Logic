use sim_engine::{
    FontBudget, FontError, LogicalPixels, PhysicalPerLogical, TextAtlasBudget, TextDirection,
    TextLayoutBudget, TextStyle,
};

use super::TextError;

/// Frozen limits for application-owned font registrations.
///
/// Source byte Vec capacities are counted, not just initialized lengths.
/// Parsed dependency metadata, shared-handle control blocks, label strings, and
/// desktop atlases are separate. Zero values may disable registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextLimits {
    max_fonts: usize,
    max_font_bytes: usize,
}

impl TextLimits {
    /// Creates exact registration-count and aggregate source-capacity limits.
    /// No allocation, parsing, or GPU operation occurs.
    pub const fn new(max_fonts: usize, max_font_bytes: usize) -> Self {
        Self {
            max_fonts,
            max_font_bytes,
        }
    }

    /// Returns the maximum number of distinct immutable font registrations.
    pub const fn max_fonts(self) -> usize {
        self.max_fonts
    }

    /// Returns the maximum sum of registered font source Vec capacities.
    pub const fn max_font_bytes(self) -> usize {
        self.max_font_bytes
    }
}

impl Default for TextLimits {
    /// Allows eight font registrations sharing an aggregate 32 MiB source cap.
    fn default() -> Self {
        Self::new(8, 32 * 1024 * 1024)
    }
}

/// Immutable size, shaping policy, and budgets of one registered font.
///
/// Logical size is pixels per em, not a promise that every glyph has that
/// height. Labels using a registration share this size and direction. Register
/// another font configuration when a second size is needed. Display DPI is
/// selected by the desktop runner; it never changes logical text metrics.
///
/// Engine budget types here are configuration values, not renderer resources.
/// Font bytes must come from a trusted application asset. Dependency parsing
/// and shaping are not an untrusted-font sandbox or an OS OOM guarantee.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextSettings {
    logical_em_size: f32,
    direction: TextDirection,
    font_budget: FontBudget,
    layout_budget: TextLayoutBudget,
    atlas_budget: TextAtlasBudget,
}

impl TextSettings {
    /// Creates a positive normal logical em size with the remaining defaults.
    ///
    /// Non-finite, non-positive, or subnormal sizes return an error. Creation
    /// is headless and does not rasterize or allocate an atlas.
    pub fn new(logical_em_size: f32) -> Result<Self, TextError> {
        let settings = Self {
            logical_em_size,
            ..Self::default()
        };
        settings.style(1.0)?;
        Ok(settings)
    }

    /// Selects a single-run shaping direction; this is not bidi paragraph layout.
    pub const fn with_direction(mut self, direction: TextDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Replaces per-font source-capacity and declared-glyph limits.
    /// Application-wide source limits still apply independently.
    pub const fn with_font_budget(mut self, budget: FontBudget) -> Self {
        self.font_budget = budget;
        self
    }

    /// Replaces per-label input/output and per-glyph rasterization work limits.
    /// Zero limits are valid. A label exceeding a limit is rejected atomically.
    pub const fn with_layout_budget(mut self, budget: TextLayoutBudget) -> Self {
        self.layout_budget = budget;
        self
    }

    /// Replaces fixed desktop atlas and per-run capacities.
    ///
    /// This stores an already validated configuration without allocating GPU
    /// resources. Device dimensions and raster fit are checked on desktop use.
    pub const fn with_atlas_budget(mut self, budget: TextAtlasBudget) -> Self {
        self.atlas_budget = budget;
        self
    }

    /// Returns the fixed logical-pixel em size shared by this registration.
    pub const fn logical_em_size(self) -> f32 {
        self.logical_em_size
    }

    /// Returns the requested direction for one horizontal shaping run.
    pub const fn direction(self) -> TextDirection {
        self.direction
    }

    /// Returns limits applied when taking ownership of font source bytes.
    pub const fn font_budget(self) -> FontBudget {
        self.font_budget
    }

    /// Returns limits applied to each changed label and rasterized glyph.
    pub const fn layout_budget(self) -> TextLayoutBudget {
        self.layout_budget
    }

    /// Returns the fixed physical atlas dimensions and per-run capacities.
    pub const fn atlas_budget(self) -> TextAtlasBudget {
        self.atlas_budget
    }

    /// Creates Engine's style for an explicit physical-pixels-per-logical scale.
    ///
    /// A headless caller can use `1.0`; desktop presentation uses actual DPI.
    /// Invalid or unrepresentable physical size returns a typed font error.
    /// This creates a value only and performs no GPU operation.
    pub fn style(self, scale: f32) -> Result<TextStyle, TextError> {
        let size = LogicalPixels::new(self.logical_em_size)
            .map_err(|_| TextError::Font(FontError::InvalidScale))?;
        let scale =
            PhysicalPerLogical::new(scale).map_err(|_| TextError::Font(FontError::InvalidScale))?;
        Ok(TextStyle::new(size, scale)?.with_direction(self.direction))
    }
}

impl Default for TextSettings {
    /// Uses 24 logical pixels per em, automatic direction, an 8 MiB font cap,
    /// 4096 UTF-8 bytes and 1024 shaped glyphs per label, and Engine's fixed
    /// 1024 by 1024 RGBA atlas. Raster work is capped per glyph at one million
    /// coverage pixels and 16384 outline commands.
    fn default() -> Self {
        Self {
            logical_em_size: 24.0,
            direction: TextDirection::Auto,
            font_budget: FontBudget::default(),
            layout_budget: TextLayoutBudget::new(4096, 1024, 1024 * 1024, 16 * 1024),
            atlas_budget: TextAtlasBudget::default(),
        }
    }
}

//! Bounded example-owned drawing lists and a single immutable bitmap atlas.
//!
//! The application reuses ECS visual pools; this module only prepares design-
//! space rectangles and glyph placements. It is not a new framework text API.

use std::{error::Error, fmt};

use sim_logic::prelude::{
    Action, Application, Color, ImageAssetId, ImageRegion, LogicResult, Resource,
};

#[path = "../support/bitmap_font.rs"]
mod bitmap_font;

pub(super) const WIDTH: f32 = 1440.0;
pub(super) const HEIGHT: f32 = 900.0;
pub(super) const MAX_RECTS: usize = 6600;
pub(super) const MAX_GLYPHS: usize = 1800;
pub(super) const FONT_WIDTH: u32 = 512;
pub(super) const FONT_HEIGHT: u32 = 256;
pub(super) const FONT_BYTES: usize = FONT_WIDTH as usize * FONT_HEIGHT as usize * 4;

// Fixed example captions are retained as whole image regions. The current
// Engine image bridge submits each image separately, so this avoids hundreds
// of draws without inventing a mutable text cache or new framework API.
const WORDS: &[&str] = &[
    "FRONTIER",
    "SIM;LOGIC / TERRITORY CONQUEST",
    "YOUR RESERVE",
    "LAND CONTROL",
    "IN THE FIELD",
    "NEW MAP [N]",
    "PAUSE [P]",
    "RESUME [P]",
    "ATTACK STRENGTH",
    "EXPAND [SPACE]",
    "DEBUG [F3]",
    "BOTS / SINGLE PLAYER",
    "CHOOSE YOUR HOME",
    "CLICK ANY LAND TILE TO BEGIN",
    "GROW. EXPAND. HOLD YOUR BORDERS.",
    "RIVALS",
    "YOU",
    "EMBER",
    "AZURE",
    "MOSS",
    "VIOLET",
    "GOLD",
    "CORAL",
    "NEUTRAL",
    "WATER",
    "TROOPS",
    "LAND",
    "INCOME / SEC",
    "RESERVE CAP",
    "TARGET",
    "NO SHARED BORDER",
    "UNCLAIMED LAND",
    "YOUR TERRITORY",
    "BORDERING RIVAL",
    "NEED A LAND BORDER",
    "VICTORY",
    "DEFEAT",
    "PRESS R TO TRY AGAIN",
    "PRESS R TO PLAY AGAIN",
    "PAUSED",
    "P TO RESUME / F9 SINGLE TICK",
    "ASSISTED RUN",
    "DEBUG INSPECTOR",
    "ECONOMY / NEXT SECOND",
    "TERRITORY",
    "INTEREST",
    "CREDITED",
    "CAPACITY",
    "EXPEDITION",
    "LAST TICK",
    "CAPTURED",
    "SPENT",
    "AI DECISIONS",
    "WAITING",
    "EXPANDING",
    "ATTACKING",
    "HOLDING",
    "ELIMINATED",
    "CHEATS ARMED",
    "CHEATS OFF",
    "F4 ARM / DISARM CHEATS",
    "F5 TROOPS / F6 AI ORDERS",
    "F8 WIN / F9 STEP WHEN PAUSED",
    "FRAME DT MS",
    "CELLS",
    "DRAW RECTS",
    "DRAW GLYPHS",
    "CLICK A BORDERING COLOR TO ATTACK",
    "1-5 PRESETS / ARROWS ADJUST / R RESTART",
    "VIEW ONLY - F4 TO ARM CHEATS",
];

const WORD_REGIONS: [[u32; 4]; WORDS.len()] = word_regions();

const fn word_regions() -> [[u32; 4]; WORDS.len()] {
    let mut regions = [[0; 4]; WORDS.len()];
    let mut x = 0;
    let mut y = 48;
    let mut index = 0;
    while index < WORDS.len() {
        let width = WORDS[index].len() as u32 * 6 - 1;
        assert!(width < FONT_WIDTH);
        if x + width > FONT_WIDTH {
            x = 0;
            y += 8;
        }
        assert!(y + 7 <= FONT_HEIGHT);
        regions[index] = [x, y, width, 7];
        x += width + 1;
        index += 1;
    }
    regions
}

/// One filled rectangle in the example's 1440-by-900 design coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Color,
}

/// One tinted atlas region, holding a single glyph or a fixed whole caption.
/// The caller draws these regions above the rectangle pool.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Glyph {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub color: Color,
    pub source: ImageRegion,
}

/// Reusable, explicitly bounded drawing storage; successful draws do not grow it.
#[derive(Debug, Resource)]
pub(super) struct Canvas {
    pub rects: Vec<Rect>,
    pub glyphs: Vec<Glyph>,
}

impl Canvas {
    /// Reserves both pools once, returning allocation failures to the caller.
    pub fn new() -> LogicResult<Self> {
        let mut rects = Vec::new();
        let mut glyphs = Vec::new();
        rects.try_reserve_exact(MAX_RECTS)?;
        glyphs.try_reserve_exact(MAX_GLYPHS)?;
        Ok(Self { rects, glyphs })
    }

    /// Starts another drawing without releasing the retained storage.
    pub fn clear(&mut self) {
        self.rects.clear();
        self.glyphs.clear();
    }

    /// Appends a rectangle clipped to the design canvas.
    ///
    /// Finite nonpositive extents represent empty bars and add nothing. Invalid
    /// numbers, colors, and budget exhaustion return errors without changing
    /// either list. Fully clipped rectangles do not consume the rectangle budget.
    pub fn rect(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) -> LogicResult {
        if ![x, y, width, height].into_iter().all(f32::is_finite) {
            return Err(DrawError::Geometry.into());
        }
        validate_color(color)?;
        if width <= 0.0 || height <= 0.0 {
            return Ok(());
        }
        // Widen before addition so finite, very large offscreen rectangles can
        // still be clipped without an intermediate f32 infinity.
        let left = f64::from(x).max(0.0);
        let top = f64::from(y).max(0.0);
        let right = (f64::from(x) + f64::from(width)).min(f64::from(WIDTH));
        let bottom = (f64::from(y) + f64::from(height)).min(f64::from(HEIGHT));
        if right <= left || bottom <= top {
            return Ok(());
        }
        let rect = Rect {
            x: left as f32,
            y: top as f32,
            width: (right - left) as f32,
            height: (bottom - top) as f32,
            color,
        };
        if rect.x + rect.width <= rect.x || rect.y + rect.height <= rect.y {
            return Err(DrawError::Geometry.into());
        }
        if self.rects.len() >= MAX_RECTS {
            return Err(DrawError::RectangleBudget.into());
        }
        self.rects.push(rect);
        Ok(())
    }

    /// Appends one ASCII line without formatting or allocating.
    ///
    /// Exact matches from the example's fixed caption list use one atlas image.
    /// Other text uses individual five-by-seven glyph images with six scaled
    /// pixels of advance. Both routes have identical ink positions. Lowercase
    /// letters display their uppercase shapes. Spaces advance without creating
    /// separate images. A line is limited to MAX_GLYPHS bytes even when mostly
    /// spaces, keeping validation work bounded. All input and capacity checks
    /// precede insertion, so a rejected line preserves both lists.
    ///
    /// As with ScreenImageVisual, finite offscreen placement is allowed; text
    /// does not receive the rectangle helper's design-canvas clipping.
    pub fn text(&mut self, x: f32, y: f32, scale: f32, text: &str, color: Color) -> LogicResult {
        self.text_bytes(x, y, scale, text.as_bytes(), color)
    }

    /// Appends an unsigned decimal number using at most ten stack bytes.
    pub fn number(
        &mut self,
        x: f32,
        y: f32,
        scale: f32,
        mut value: u32,
        color: Color,
    ) -> LogicResult {
        let mut digits = [b'0'; 10];
        let mut start = digits.len();
        loop {
            start -= 1;
            digits[start] = b'0' + (value % 10) as u8;
            value /= 10;
            if value == 0 {
                break;
            }
        }
        self.text_bytes(x, y, scale, &digits[start..], color)
    }

    /// Reports active rectangle slots, not reserved pool capacity.
    pub fn rect_count(&self) -> usize {
        self.rects.len()
    }

    /// Reports active image slots; one packed caption counts as one slot.
    pub fn glyph_count(&self) -> usize {
        self.glyphs.len()
    }

    fn text_bytes(&mut self, x: f32, y: f32, scale: f32, text: &[u8], color: Color) -> LogicResult {
        if ![x, y, scale].into_iter().all(f32::is_finite) || scale <= 0.0 {
            return Err(DrawError::Geometry.into());
        }
        validate_color(color)?;
        if text.len() > MAX_GLYPHS {
            return Err(DrawError::GlyphBudget.into());
        }
        if !text.iter().all(|byte| (b' '..=b'~').contains(byte)) {
            return Err(DrawError::Ascii.into());
        }
        let packed = WORDS.iter().position(|word| word.as_bytes() == text);
        let count = if packed.is_some() {
            1
        } else {
            text.iter().filter(|&&byte| byte != b' ').count()
        };
        if count > MAX_GLYPHS.saturating_sub(self.glyphs.len()) {
            return Err(DrawError::GlyphBudget.into());
        }
        let width = 5.0 * scale;
        let height = 7.0 * scale;
        let advance = 6.0 * scale;
        let last_x = x + text.len().saturating_sub(1) as f32 * advance;
        if ![width, height, advance, last_x, last_x + width, y + height]
            .into_iter()
            .all(f32::is_finite)
            || x + width <= x
            || last_x + width <= last_x
            || y + height <= y
        {
            return Err(DrawError::Geometry.into());
        }
        if let Some(index) = packed {
            let [source_x, source_y, source_width, source_height] = WORD_REGIONS[index];
            let packed_width = source_width as f32 * scale;
            if !packed_width.is_finite() || !(x + packed_width).is_finite() || x + packed_width <= x
            {
                return Err(DrawError::Geometry.into());
            }
            self.glyphs.push(Glyph {
                x,
                y,
                width: packed_width,
                height,
                color,
                source: ImageRegion::new(source_x, source_y, source_width, source_height)?,
            });
            return Ok(());
        }
        for (index, &byte) in text.iter().enumerate() {
            if byte == b' ' {
                continue;
            }
            let source = glyph_region(byte)?;
            self.glyphs.push(Glyph {
                x: x + index as f32 * advance,
                y,
                width,
                height,
                color,
                source,
            });
        }
        Ok(())
    }
}

/// Registers one 512-by-256 white-on-transparent RGBA8 atlas before startup.
///
/// ASCII bytes 32 through 127 occupy 16 columns of 6-by-8 cells at the top left.
/// Fixed whole captions are packed below the first 48 rows. This is one
/// application-owned immutable asset; rendering only changes image regions,
/// placement, and tint. ScreenImageVisual defaults to the required nearest
/// filtering. Pixel storage is allocated once and dropped after registration.
pub(super) fn register_font<A: Action>(
    application: &mut Application<A>,
) -> LogicResult<ImageAssetId> {
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(FONT_BYTES)?;
    pixels.resize(FONT_BYTES, 0);
    rasterize_font(&mut pixels);
    Ok(application.register_image_rgba8(FONT_WIDTH, FONT_HEIGHT, &pixels)?)
}

fn glyph_region(byte: u8) -> LogicResult<ImageRegion> {
    let cell = u32::from(byte - b' ');
    Ok(ImageRegion::new((cell % 16) * 6, (cell / 16) * 8, 5, 7)?)
}

fn rasterize_font(pixels: &mut [u8]) {
    for byte in b' '..=127 {
        let cell = usize::from(byte - b' ');
        let x = cell % 16 * 6;
        let y = cell / 16 * 8;
        for (row, bits) in glyph_shape(byte).into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let offset = ((y + row) * FONT_WIDTH as usize + x + column) * 4;
                    pixels[offset..offset + 4].fill(255);
                }
            }
        }
    }
    for (word, &[x, y, _, _]) in WORDS.iter().zip(&WORD_REGIONS) {
        for (index, byte) in word.bytes().enumerate() {
            for (row, bits) in glyph_shape(byte).into_iter().enumerate() {
                for column in 0..5 {
                    if bits & (1 << (4 - column)) != 0 {
                        let offset = ((y as usize + row) * FONT_WIDTH as usize
                            + x as usize
                            + index * 6
                            + column)
                            * 4;
                        pixels[offset..offset + 4].fill(255);
                    }
                }
            }
        }
    }
}

fn glyph_shape(byte: u8) -> [u8; 7] {
    match byte.to_ascii_uppercase() {
        b' ' => [0; 7],
        b'%' => [25, 25, 2, 4, 8, 19, 19],
        b'=' => [0, 31, 0, 0, 31, 0, 0],
        b'[' => [14, 8, 8, 8, 8, 8, 14],
        b']' => [14, 2, 2, 2, 2, 2, 14],
        b'(' => [2, 4, 8, 8, 8, 4, 2],
        b')' => [8, 4, 2, 2, 2, 4, 8],
        b'!' => [4, 4, 4, 4, 4, 0, 4],
        b'_' => [0, 0, 0, 0, 0, 0, 31],
        b',' => [0, 0, 0, 0, 0, 4, 8],
        b';' => [0, 4, 4, 0, 0, 4, 8],
        b'*' => [0, 21, 14, 31, 14, 21, 0],
        b'|' => [4; 7],
        byte => {
            let shape = bitmap_font::glyph(byte);
            if shape == [0; 7] {
                [14, 17, 1, 2, 4, 0, 4]
            } else {
                shape
            }
        }
    }
}

fn validate_color(color: Color) -> Result<(), DrawError> {
    if color.is_normalized() {
        Ok(())
    } else {
        Err(DrawError::Color)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawError {
    Geometry,
    Color,
    Ascii,
    RectangleBudget,
    GlyphBudget,
}

impl fmt::Display for DrawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Geometry => "territory drawing geometry is not finite or representable",
            Self::Color => "territory drawing color must be finite and normalized",
            Self::Ascii => "territory labels must be single-line printable ASCII",
            Self::RectangleBudget => "territory rectangle pool is full",
            Self::GlyphBudget => "territory glyph pool or line-length budget is full",
        })
    }
}

impl Error for DrawError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_clip_and_empty_bars_do_not_consume_slots() -> LogicResult {
        let mut canvas = Canvas::new()?;
        canvas.rect(-5.0, -8.0, 15.0, 18.0, Color::WHITE)?;
        assert_eq!(canvas.rects[0].x, 0.0);
        assert_eq!(canvas.rects[0].y, 0.0);
        assert_eq!(canvas.rects[0].width, 10.0);
        assert_eq!(canvas.rects[0].height, 10.0);
        canvas.rect(WIDTH - 2.0, HEIGHT - 3.0, 50.0, 50.0, Color::WHITE)?;
        assert_eq!(canvas.rects[1].width, 2.0);
        assert_eq!(canvas.rects[1].height, 3.0);
        canvas.rect(0.0, 0.0, 0.0, 1.0, Color::WHITE)?;
        canvas.rect(0.0, 0.0, 1.0, -1.0, Color::WHITE)?;
        canvas.rect(WIDTH, HEIGHT, 10.0, 10.0, Color::WHITE)?;
        assert_eq!(canvas.rect_count(), 2);
        Ok(())
    }

    #[test]
    fn pools_never_grow_and_rejections_preserve_prior_drawing() -> LogicResult {
        let mut canvas = Canvas::new()?;
        let capacities = (canvas.rects.capacity(), canvas.glyphs.capacity());
        for _ in 0..MAX_RECTS {
            canvas.rect(0.0, 0.0, 1.0, 1.0, Color::WHITE)?;
        }
        assert!(canvas.rect(0.0, 0.0, 1.0, 1.0, Color::WHITE).is_err());
        for _ in 0..MAX_GLYPHS - 1 {
            canvas.text(0.0, 0.0, 1.0, "A", Color::WHITE)?;
        }
        assert!(canvas.text(0.0, 0.0, 1.0, "BC", Color::WHITE).is_err());
        assert_eq!(canvas.glyph_count(), MAX_GLYPHS - 1);
        canvas.text(0.0, 0.0, 1.0, "D", Color::WHITE)?;
        assert_eq!(
            capacities,
            (canvas.rects.capacity(), canvas.glyphs.capacity())
        );
        canvas.clear();
        assert_eq!(canvas.rect_count(), 0);
        assert_eq!(canvas.glyph_count(), 0);
        assert_eq!(
            capacities,
            (canvas.rects.capacity(), canvas.glyphs.capacity())
        );
        Ok(())
    }

    #[test]
    fn text_spacing_numbers_and_source_regions_are_exact() -> LogicResult {
        let mut canvas = Canvas::new()?;
        canvas.text(10.0, 20.0, 2.0, "A B", Color::WHITE)?;
        assert_eq!(canvas.glyph_count(), 2);
        assert_eq!(canvas.glyphs[1].x, 34.0);
        assert_eq!(canvas.glyphs[0].width, 10.0);
        assert_eq!(canvas.glyphs[0].height, 14.0);
        assert_eq!(canvas.glyphs[0].source, glyph_region(b'A')?);
        canvas.clear();
        canvas.number(0.0, 0.0, 1.0, 0, Color::WHITE)?;
        assert_eq!(canvas.glyph_count(), 1);
        assert_eq!(canvas.glyphs[0].source, glyph_region(b'0')?);
        canvas.clear();
        canvas.number(0.0, 0.0, 1.0, u32::MAX, Color::WHITE)?;
        assert_eq!(canvas.glyph_count(), 10);
        for (image, &byte) in canvas.glyphs.iter().zip(b"4294967295") {
            assert_eq!(image.source, glyph_region(byte)?);
        }
        Ok(())
    }

    #[test]
    fn malformed_input_does_not_partially_append() -> LogicResult {
        let mut canvas = Canvas::new()?;
        assert!(canvas.text(0.0, 0.0, 1.0, "A\nB", Color::WHITE).is_err());
        assert!(canvas.text(0.0, 0.0, 1.0, "Aé", Color::WHITE).is_err());
        assert!(canvas.text(0.0, 0.0, f32::MAX, "A", Color::WHITE).is_err());
        assert!(canvas.rect(f32::NAN, 0.0, 1.0, 1.0, Color::WHITE).is_err());
        assert!(
            canvas
                .rect(0.0, 0.0, 1.0, 1.0, Color::rgba(0.0, 0.0, 0.0, f32::NAN))
                .is_err()
        );
        assert_eq!(canvas.rect_count(), 0);
        assert_eq!(canvas.glyph_count(), 0);
        Ok(())
    }

    #[test]
    fn atlas_cells_have_transparent_gutters_and_only_white_ink() -> LogicResult {
        let mut pixels = vec![0; FONT_BYTES];
        rasterize_font(&mut pixels);
        for y in 0..FONT_HEIGHT as usize {
            for x in 0..FONT_WIDTH as usize {
                let offset = (y * FONT_WIDTH as usize + x) * 4;
                let pixel = &pixels[offset..offset + 4];
                assert!(pixel == [0; 4] || pixel == [255; 4]);
                if x % 6 == 5 || y % 8 == 7 {
                    assert_eq!(pixel, [0; 4]);
                }
            }
        }
        assert_eq!(glyph_shape(b'a'), glyph_shape(b'A'));
        for byte in b' '..=127 {
            let region = glyph_region(byte)?;
            assert!(region.x() + region.width() <= FONT_WIDTH);
            assert!(region.y() + region.height() <= FONT_HEIGHT);
        }
        Ok(())
    }

    #[test]
    fn font_registration_fits_exactly_one_bounded_asset() -> LogicResult {
        use sim_logic::prelude::{AppConfig, ImageAssetLimits};

        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        enum TestAction {}

        let mut config = AppConfig::default();
        config.set_image_asset_limits(ImageAssetLimits::new(
            1,
            FONT_WIDTH,
            FONT_HEIGHT,
            FONT_BYTES,
        ));
        let mut application = Application::<TestAction>::new(config)?;
        let image = register_font(&mut application)?;
        assert_eq!(image.width(), FONT_WIDTH);
        assert_eq!(image.height(), FONT_HEIGHT);
        assert!(register_font(&mut application).is_err());
        Ok(())
    }

    #[test]
    fn exact_fixed_captions_use_one_slot_and_other_text_uses_glyphs() -> LogicResult {
        let mut canvas = Canvas::new()?;
        let color = Color::rgba8(100, 160, 200, 128);
        canvas.text(10.0, 20.0, 2.0, "FRONTIER", color)?;
        assert_eq!(canvas.glyph_count(), 1);
        let caption = canvas.glyphs[0];
        assert_eq!(caption.x, 10.0);
        assert_eq!(caption.y, 20.0);
        assert_eq!(caption.width, 94.0);
        assert_eq!(caption.height, 14.0);
        assert_eq!(caption.color, color);
        assert_eq!(caption.source.width(), 47);
        canvas.clear();
        canvas.text(10.0, 20.0, 2.0, "FRONTIER ", color)?;
        assert_eq!(canvas.glyph_count(), 8);
        assert_eq!(canvas.glyphs[7].x + canvas.glyphs[7].width, 104.0);
        Ok(())
    }

    #[test]
    fn every_packed_word_matches_individual_glyph_pixels_without_overlap() {
        let mut pixels = vec![0; FONT_BYTES];
        rasterize_font(&mut pixels);
        let mut unique = std::collections::HashSet::new();
        for (index, word) in WORDS.iter().enumerate() {
            assert!(unique.insert(word));
            let [x, y, width, height] = WORD_REGIONS[index];
            assert!(y >= 48);
            assert!(x + width <= FONT_WIDTH && y + height <= FONT_HEIGHT);
            for &[other_x, other_y, other_width, other_height] in &WORD_REGIONS[..index] {
                assert!(
                    x + width <= other_x
                        || other_x + other_width <= x
                        || y + height <= other_y
                        || other_y + other_height <= y
                );
            }
            for (character, byte) in word.bytes().enumerate() {
                let cell = u32::from(byte - b' ');
                let source_x = cell % 16 * 6;
                let source_y = cell / 16 * 8;
                for row in 0..7 {
                    for column in 0..5 {
                        let single =
                            ((source_y + row) * FONT_WIDTH + source_x + column) as usize * 4;
                        let packed = ((y + row) * FONT_WIDTH + x + character as u32 * 6 + column)
                            as usize
                            * 4;
                        assert_eq!(&pixels[single..single + 4], &pixels[packed..packed + 4]);
                    }
                }
            }
        }
    }
}

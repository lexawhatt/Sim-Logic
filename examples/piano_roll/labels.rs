//! Static bitmap labels, registered once before the piano application's startup.
//!
//! These are example-owned words and original 5-by-7 shapes, not a framework
//! text renderer. Rendering code positions/tints their immutable image handles;
//! no font files, pixel rebuilding, or text allocation is needed during frames.

use sim_logic::prelude::{Action, Application, ImageAssetId, LogicResult};

#[path = "../support/bitmap_font.rs"]
mod bitmap_font;

const NAMED_COUNT: usize = 20;
const KEY_COUNT: usize = 36;
const HEIGHT: usize = 7;
const MAX_WIDTH: usize = 317;
const MAX_PIXEL_BYTES: usize = MAX_WIDTH * HEIGHT * 4;

/// Registered entries: 20 named labels, ten digits, and 36 octave-qualified keys.
pub const ASSET_COUNT: usize = NAMED_COUNT + 10 + KEY_COUNT;
/// Exact RGBA8 payload of all labels, excluding registry metadata/GPU storage.
pub const PIXEL_BYTES: usize = 44_184;

/// The fixed words used by the piano controls; their layout belongs to the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// Application title.
    Title,
    /// Sample-clock subtitle.
    Subtitle,
    /// Start/resume button.
    Play,
    /// Pause button.
    Pause,
    /// Stop/rewind button.
    Stop,
    /// Restore the example phrase.
    Demo,
    /// Erase the score.
    Clear,
    /// Tempo value heading.
    Tempo,
    /// Note-length value heading.
    Length,
    /// Master-volume value heading.
    Volume,
    /// Flat piano-roll view selector.
    View2d,
    /// Volumetric piano view selector.
    View3d,
    /// Increment button symbol.
    Plus,
    /// Decrement button symbol.
    Minus,
    /// Pointer drawing/erasing reminder.
    PointerHelp,
    /// Live-key and transport/view shortcuts.
    KeyboardHelp,
    /// Active audio-output status.
    Audio,
    /// Silent/offline status.
    Silent,
    /// Failed audio-output status.
    AudioError,
    /// Bounded command-queue rejection status.
    Busy,
}

impl Label {
    const ALL: [Self; NAMED_COUNT] = [
        Self::Title,
        Self::Subtitle,
        Self::Play,
        Self::Pause,
        Self::Stop,
        Self::Demo,
        Self::Clear,
        Self::Tempo,
        Self::Length,
        Self::Volume,
        Self::View2d,
        Self::View3d,
        Self::Plus,
        Self::Minus,
        Self::PointerHelp,
        Self::KeyboardHelp,
        Self::Audio,
        Self::Silent,
        Self::AudioError,
        Self::Busy,
    ];

    /// Original ASCII text encoded into this label's immutable pixels.
    pub const fn text(self) -> &'static str {
        match self {
            Self::Title => "PIANO ROLL",
            Self::Subtitle => "SAMPLE CLOCK SYNTH",
            Self::Play => "PLAY",
            Self::Pause => "PAUSE",
            Self::Stop => "STOP",
            Self::Demo => "DEMO",
            Self::Clear => "CLEAR",
            Self::Tempo => "TEMPO",
            Self::Length => "LENGTH",
            Self::Volume => "VOLUME",
            Self::View2d => "2D",
            Self::View3d => "3D",
            Self::Plus => "+",
            Self::Minus => "-",
            Self::PointerHelp => "LEFT: DRAW / RIGHT: ERASE",
            Self::KeyboardHelp => "A S D W: KEYS / SPACE: PLAY / ENTER: VIEW / ESC: EXIT",
            Self::Audio => "AUDIO",
            Self::Silent => "SILENT",
            Self::AudioError => "AUDIO ERROR",
            Self::Busy => "BUSY",
        }
    }
}

/// Application-owned handles for reusable white-on-transparent bitmap labels.
#[derive(Debug, Clone, Copy)]
pub struct LabelAssets {
    named: [ImageAssetId; NAMED_COUNT],
    /// Decimal digits in numeric order; assemble numeric displays without rasterizing.
    pub digits: [ImageAssetId; 10],
    /// Chromatic key names from C3 through B5; index zero corresponds to MIDI 48.
    pub key_names: [ImageAssetId; KEY_COUNT],
}

impl LabelAssets {
    /// Prepares and registers all 66 immutable images before startup.
    ///
    /// Images are seven texels high, at most 317 wide, and total 44,184 RGBA8
    /// bytes. White glyph pixels are opaque; the background is transparent black.
    /// One fixed 8,876-byte scratch buffer is reused for registration. Consumers
    /// choose nearest filtering and integer-scaled sizes for crisp bitmap text.
    ///
    /// The application must allow enough image entries (the default 64 is too
    /// small). A failed registration returns its error; earlier successful
    /// registrations remain application-owned, since there is no batch rollback.
    pub fn new<A: Action>(application: &mut Application<A>) -> LogicResult<Self> {
        let mut pixels = [0; MAX_PIXEL_BYTES];
        let title = register(application, Label::Title.text().as_bytes(), &mut pixels)?;
        let mut named = [title; NAMED_COUNT];
        for label in Label::ALL.into_iter().skip(1) {
            named[label as usize] = register(application, label.text().as_bytes(), &mut pixels)?;
        }

        let zero = register(application, b"0", &mut pixels)?;
        let mut digits = [zero; 10];
        for (index, image) in digits.iter_mut().enumerate().skip(1) {
            *image = register(application, &[b'0' + index as u8], &mut pixels)?;
        }

        let first_key = register(application, b"C3", &mut pixels)?;
        let mut key_names = [first_key; KEY_COUNT];
        for (index, image) in key_names.iter_mut().enumerate().skip(1) {
            let (text, length) = key_text(index);
            *image = register(application, &text[..length], &mut pixels)?;
        }
        Ok(Self {
            named,
            digits,
            key_names,
        })
    }

    /// Looks up a named label without allocating or rebuilding its pixels.
    pub fn id(&self, label: Label) -> ImageAssetId {
        self.named[label as usize]
    }
}

fn key_text(index: usize) -> ([u8; 3], usize) {
    let octave = b'3' + (index / 12) as u8;
    let (letter, sharp) = match index % 12 {
        0 => (b'C', false),
        1 => (b'C', true),
        2 => (b'D', false),
        3 => (b'D', true),
        4 => (b'E', false),
        5 => (b'F', false),
        6 => (b'F', true),
        7 => (b'G', false),
        8 => (b'G', true),
        9 => (b'A', false),
        10 => (b'A', true),
        _ => (b'B', false),
    };
    if sharp {
        ([letter, b'#', octave], 3)
    } else {
        ([letter, octave, b' '], 2)
    }
}

fn register<A: Action>(
    application: &mut Application<A>,
    text: &[u8],
    pixels: &mut [u8; MAX_PIXEL_BYTES],
) -> LogicResult<ImageAssetId> {
    let width = rasterize(text, pixels);
    Ok(application.register_image_rgba8(
        width as u32,
        HEIGHT as u32,
        &pixels[..width * HEIGHT * 4],
    )?)
}

fn rasterize(text: &[u8], pixels: &mut [u8; MAX_PIXEL_BYTES]) -> usize {
    // The private caller supplies only the fixed, nonempty ASCII labels above.
    let width = text.len() * 6 - 1;
    pixels[..width * HEIGHT * 4].fill(0);
    for (character_index, &character) in text.iter().enumerate() {
        for (row, bits) in bitmap_font::glyph(character).into_iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    let offset = (row * width + character_index * 6 + column) * 4;
                    pixels[offset..offset + 4].fill(255);
                }
            }
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_logic::prelude::{ActiveCamera2d, AppConfig, ImageAssetLimits};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {}

    #[test]
    fn glyph_pixels_are_white_rgba_with_transparent_spacing() {
        let mut pixels = [19; MAX_PIXEL_BYTES];
        assert_eq!(rasterize(b"A A", &mut pixels), 17);
        for row in 0..HEIGHT {
            for column in 0..17 {
                let offset = (row * 17 + column) * 4;
                let texel = &pixels[offset..offset + 4];
                assert!(texel == [0; 4] || texel == [255; 4]);
                if (5..12).contains(&column) {
                    assert_eq!(texel, [0; 4]);
                }
            }
        }
        assert_eq!(&pixels[0..4], &[0; 4]);
        assert_eq!(&pixels[4..8], &[255; 4]);
        assert_eq!(&pixels[16..20], &[0; 4]);
        assert_eq!(rasterize(b" ", &mut pixels), 5);
        assert!(pixels[..5 * HEIGHT * 4].iter().all(|&value| value == 0));
    }

    #[test]
    fn every_named_character_and_sharp_has_a_defined_original_glyph() {
        for label in Label::ALL {
            assert!(label.text().len() * 6 - 1 <= MAX_WIDTH);
            for character in label.text().bytes().filter(|&character| character != b' ') {
                assert_ne!(bitmap_font::glyph(character), [0; HEIGHT]);
            }
        }
        assert_ne!(bitmap_font::glyph(b'#'), [0; HEIGHT]);
        assert_eq!(key_text(0), ([b'C', b'3', b' '], 2));
        assert_eq!(key_text(1), ([b'C', b'#', b'3'], 3));
        assert_eq!(key_text(12), ([b'C', b'4', b' '], 2));
        assert_eq!(key_text(35), ([b'B', b'5', b' '], 2));
    }

    #[test]
    fn registered_assets_match_the_exact_declared_budget() -> LogicResult {
        let mut config = AppConfig::default();
        config.set_image_asset_limits(ImageAssetLimits::new(
            ASSET_COUNT,
            MAX_WIDTH as u32,
            HEIGHT as u32,
            PIXEL_BYTES,
        ));
        let mut application = Application::<TestAction>::new(config)?;
        let assets = LabelAssets::new(&mut application)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let initial = application.register_world("labels", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let runner = application.build_headless(initial)?;
        assert_eq!(runner.image_asset_count(), ASSET_COUNT);
        assert_eq!(runner.image_pixel_bytes(), PIXEL_BYTES);
        let all = assets
            .named
            .iter()
            .chain(&assets.digits)
            .chain(&assets.key_names);
        let mut unique = std::collections::HashSet::new();
        let mut total = 0;
        for &id in all {
            assert!(unique.insert(id));
            assert_eq!(id.height(), HEIGHT as u32);
            let pixels = runner.image_asset(id).ok_or("missing label")?.pixels();
            assert!(pixels.chunks_exact(4).any(|pixel| pixel == [255; 4]));
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|pixel| pixel == [0; 4] || pixel == [255; 4])
            );
            total += pixels.len();
        }
        assert_eq!(total, PIXEL_BYTES);
        assert_eq!(assets.id(Label::KeyboardHelp).width(), 317);
        assert_eq!(assets.digits[9].width(), 5);
        assert_eq!(assets.key_names[0].width(), 11);
        assert_eq!(assets.key_names[1].width(), 17);
        Ok(())
    }

    #[test]
    fn insufficient_asset_count_returns_the_registration_error() -> LogicResult {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let error = LabelAssets::new(&mut application).unwrap_err();
        assert!(matches!(
            error.downcast_ref::<sim_logic::assets::ImageAssetError>(),
            Some(sim_logic::assets::ImageAssetError::ImageLimitExceeded { limit: 64 })
        ));
        Ok(())
    }
}

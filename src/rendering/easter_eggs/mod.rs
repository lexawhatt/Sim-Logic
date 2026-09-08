//! Optional Ferris mascot; ordinary managed images do all the actual drawing.

use std::{error::Error, fmt, io::Cursor};

use sim_engine::{Color, LogicalScreenPosition, LogicalScreenVector};

use crate::{
    app::Application,
    assets::ImageAssetError,
    input::Action,
    screen::{ImageFilter, ImageVisualError, ScreenImageVisual, ScreenRectangleVisual},
};

const WIDTH: u32 = 460;
const HEIGHT: u32 = 307;
const PIXEL_BYTES: usize = WIDTH as usize * HEIGHT as usize * 4;
const PNG: &[u8] = include_bytes!("assets/ferris.png");

/// Prepares Ferris for drawing at a logical screen top-left and positive width.
///
/// Requires the `easter-eggs` feature. Call before starting the application,
/// then spawn the returned [`ScreenImageVisual`] normally. Height preserves
/// the original 460:307 aspect ratio; sampling is linear, tint is white, and
/// layer/depth are the normal screen-image defaults. No entity, window, GPU
/// resource, camera or System is created by this function.
///
/// The first successful call registers one immutable 460 by 307 RGBA image
/// (564,880 pixel bytes). Further calls reuse that application's handle, even
/// when its image registry is full. Geometry is checked before any image work.
/// Asset failure publishes no image or cached handle; existing assets remain
/// unchanged. PNG decoding needs temporary memory on first registration only.
/// No process-wide cache or runtime file/network access is involved.
///
/// Ordinary image, entity and rendering limits still apply and are never
/// increased automatically. Enable screen images and texture/pass allowances
/// before startup. Copy the visual or change its position, size, tint and
/// layer for more crabs; assets survive World replacement in their application.
///
/// ```
/// use sim_logic::prelude::*;
/// # fn main() -> LogicResult {
/// let mut config = AppConfig::default();
/// config.set_render_limits(RenderLimits::default()
///     .with_max_screen_images(1)
///     .with_frame_limits(FrameLimits::new(2, 1, 6, 4096, 1024 * 1024, 1)));
/// let mut app = Application::<u8>::new(config)?;
/// let crab = draw_crab(&mut app, LogicalScreenPosition::new(24.0, 24.0), 230.0)?;
/// let camera = ActiveCamera2d::centered(1.0)?;
/// let initial = app.register_world("ferris", move |world| {
///     world.spawn(camera)?;
///     world.spawn(crab)?;
///     Ok(())
/// })?;
/// let runner = app.build_headless(initial)?;
/// assert_eq!(runner.image_asset_count(), 1);
/// # Ok(())
/// # }
/// ```
pub fn draw_crab<A: Action>(
    app: &mut Application<A>,
    position: LogicalScreenPosition,
    width: f32,
) -> Result<ScreenImageVisual, CrabDrawError> {
    let size = LogicalScreenVector::new(width, width * (HEIGHT as f32 / WIDTH as f32));
    // Same geometry/tint checks as ScreenImageVisual, before registry mutation.
    ScreenRectangleVisual::new(position, size, Color::WHITE)
        .map_err(|error| CrabDrawError::Visual(error.into()))?;
    let image = if let Some(image) = app.crab_image {
        image
    } else {
        let pixels = decode(PNG)?;
        let image = app
            .register_image_rgba8(WIDTH, HEIGHT, &pixels)
            .map_err(CrabDrawError::Image)?;
        app.crab_image = Some(image);
        image
    };
    let mut visual =
        ScreenImageVisual::new(image, position, size).map_err(CrabDrawError::Visual)?;
    visual.set_filter(ImageFilter::Linear);
    Ok(visual)
}

fn decode(bytes: &[u8]) -> Result<Vec<u8>, CrabDrawError> {
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: 2 * 1024 * 1024,
        },
    );
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    let mut reader = decoder.read_info().map_err(CrabDrawError::Decode)?;
    let info = reader.info();
    if (info.width, info.height, info.color_type, info.bit_depth)
        != (WIDTH, HEIGHT, png::ColorType::Rgba, png::BitDepth::Eight)
        || info.animation_control.is_some()
        || reader.output_buffer_size() != Some(PIXEL_BYTES)
    {
        return Err(CrabDrawError::InvalidArtwork);
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(PIXEL_BYTES)
        .map_err(|source| CrabDrawError::Image(ImageAssetError::AllocationFailed { source }))?;
    pixels.resize(PIXEL_BYTES, 0);
    let frame = reader
        .next_frame(&mut pixels)
        .map_err(CrabDrawError::Decode)?;
    if frame.buffer_size() != PIXEL_BYTES {
        return Err(CrabDrawError::InvalidArtwork);
    }
    Ok(pixels)
}

/// Failed Ferris preparation; no World or renderer is modified.
#[derive(Debug)]
#[non_exhaustive]
pub enum CrabDrawError {
    /// Destination geometry cannot be represented as a valid screen image.
    Visual(ImageVisualError),
    /// Pixel allocation or normal application image registration failed.
    Image(ImageAssetError),
    /// The embedded PNG could not be decoded.
    Decode(png::DecodingError),
    /// The embedded artwork did not have its expected static RGBA layout.
    InvalidArtwork,
}

impl fmt::Display for CrabDrawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Visual(error) => write!(formatter, "crab placement failed: {error}"),
            Self::Image(error) => write!(formatter, "crab image registration failed: {error}"),
            Self::Decode(error) => write!(formatter, "embedded Ferris PNG failed: {error}"),
            Self::InvalidArtwork => {
                formatter.write_str("embedded Ferris must be a static 460x307 RGBA8 image")
            }
        }
    }
}

impl Error for CrabDrawError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Visual(error) => Some(error),
            Self::Image(error) => Some(error),
            Self::Decode(error) => Some(error),
            Self::InvalidArtwork => None,
        }
    }
}

#[cfg(test)]
mod tests;

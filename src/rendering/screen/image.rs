//! Managed image placement using the same logical geometry as screen panels.

use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;
use sim_engine::{Color, Layer, LogicalScreenPosition, LogicalScreenVector};

use crate::assets::ImageAssetId;

use super::{ScreenRectangleVisual, ScreenVisualError};

/// Sampling applied when a source image is scaled on screen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageFilter {
    /// Select the nearest texel, preserving hard pixel boundaries by default.
    #[default]
    Nearest,
    /// Blend neighboring texels when sampling the source image.
    Linear,
}

/// An exact rectangular source region in image texels, with top-left origin.
///
/// Width and height are positive, and each origin plus extent fits in u32.
/// The region must also fit the selected image when attached to a visual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageRegion {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl ImageRegion {
    /// Creates a source region, rejecting zero extents and overflowing bounds.
    ///
    /// Coordinates are texels, not normalized UVs or logical screen pixels.
    /// No image is sampled or allocated; image-specific bounds are checked by
    /// [`ScreenImageVisual::set_source_region`].
    pub fn new(x: u32, y: u32, width: u32, height: u32) -> Result<Self, ImageVisualError> {
        let region = Self {
            x,
            y,
            width,
            height,
        };
        region.validate()?;
        Ok(region)
    }

    /// Returns the leftmost source texel coordinate.
    pub const fn x(self) -> u32 {
        self.x
    }

    /// Returns the topmost source texel coordinate.
    pub const fn y(self) -> u32 {
        self.y
    }

    /// Returns the positive source width in texels.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the positive source height in texels.
    pub const fn height(self) -> u32 {
        self.height
    }

    fn validate(self) -> Result<(), ImageVisualError> {
        if self.width == 0
            || self.height == 0
            || self.x.checked_add(self.width).is_none()
            || self.y.checked_add(self.height).is_none()
        {
            return Err(ImageVisualError::InvalidRegion {
                x: self.x,
                y: self.y,
                width: self.width,
                height: self.height,
            });
        }
        Ok(())
    }

    fn validate_image(self, image: ImageAssetId) -> Result<(), ImageVisualError> {
        self.validate()?;
        if self.x + self.width > image.width() || self.y + self.height > image.height() {
            return Err(ImageVisualError::RegionOutOfBounds {
                region: self,
                image,
            });
        }
        Ok(())
    }
}

/// An immutable image drawn in logical screen pixels above world content.
///
/// Position is the top-left corner, x increases rightward, and y increases
/// downward. Finite offscreen placement is allowed. Geometry, tint, layer, and
/// draw-order depth use the same validation as [`ScreenRectangleVisual`]. The
/// component needs no Transform, window, or GPU, and FrameUpdate may change it.
/// Extraction samples current values and excludes disabled entities.
///
/// Construction validates image-region dimensions from the issued handle;
/// extraction also checks that its application registry owns the image. A
/// foreign handle is not made valid by constructing this component. Within
/// screen content, layer, depth, and stable entity identity control ordering;
/// a rectangle precedes an image on an exact same-entity tie. No input capture
/// or hit testing is implied by visibility.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct ScreenImageVisual {
    image: ImageAssetId,
    source_region: Option<ImageRegion>,
    filter: ImageFilter,
    rectangle: ScreenRectangleVisual,
}

impl ScreenImageVisual {
    /// Places the full image with white tint and nearest-texel sampling.
    ///
    /// The default layer and depth zero match screen rectangles. Position must
    /// be finite; size must be positive and finite, with finite, increasing far
    /// bounds. Geometry is independent from the image's texel dimensions.
    pub fn new(
        image: ImageAssetId,
        position: LogicalScreenPosition,
        size: LogicalScreenVector,
    ) -> Result<Self, ImageVisualError> {
        Ok(Self {
            image,
            source_region: None,
            filter: ImageFilter::Nearest,
            rectangle: ScreenRectangleVisual::new(position, size, Color::WHITE)?,
        })
    }

    /// Returns the immutable application image handle used by this visual.
    pub const fn image(&self) -> ImageAssetId {
        self.image
    }

    /// Returns the source texel rectangle, or None for the complete image.
    pub const fn source_region(&self) -> Option<ImageRegion> {
        self.source_region
    }

    /// Returns the sampling policy used when scaling this image.
    pub const fn filter(&self) -> ImageFilter {
        self.filter
    }

    /// Returns the top-left destination in logical screen pixels.
    pub const fn position(&self) -> LogicalScreenPosition {
        self.rectangle.position()
    }

    /// Returns the positive destination width and height in logical pixels.
    pub const fn size(&self) -> LogicalScreenVector {
        self.rectangle.size()
    }

    /// Returns the normalized straight-linear RGBA multiplication tint.
    pub const fn tint(&self) -> Color {
        self.rectangle.color()
    }

    /// Returns the primary ordering layer within screen content.
    pub const fn layer(&self) -> Layer {
        self.rectangle.layer()
    }

    /// Returns the finite within-layer draw order, unrelated to camera depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.rectangle.draw_order_depth()
    }

    /// Replaces the image while retaining its source region and presentation.
    ///
    /// If the existing region does not fit the new image, nothing changes.
    /// With no explicit region, the new image's complete extent is selected.
    /// Application provenance is checked later by extraction.
    pub fn set_image(&mut self, image: ImageAssetId) -> Result<(), ImageVisualError> {
        if let Some(region) = self.source_region {
            region.validate_image(image)?;
        }
        self.image = image;
        Ok(())
    }

    /// Selects exact source texels, or resets to the full image with None.
    ///
    /// A region outside the selected image leaves every component field
    /// unchanged. This changes sampling, not destination position or size.
    pub fn set_source_region(
        &mut self,
        region: Option<ImageRegion>,
    ) -> Result<(), ImageVisualError> {
        if let Some(region) = region {
            region.validate_image(self.image)?;
        }
        self.source_region = region;
        Ok(())
    }

    /// Replaces the sampling policy without changing image or geometry.
    pub fn set_filter(&mut self, filter: ImageFilter) {
        self.filter = filter;
    }

    /// Replaces position and size together using screen rectangle validation.
    ///
    /// Invalid values leave the component unchanged. Finite negative and
    /// offscreen positions are accepted without clamping or source cropping.
    pub fn set_geometry(
        &mut self,
        position: LogicalScreenPosition,
        size: LogicalScreenVector,
    ) -> Result<(), ImageVisualError> {
        self.rectangle.set_geometry(position, size)?;
        Ok(())
    }

    /// Replaces position while retaining size; invalid bounds change nothing.
    pub fn set_position(
        &mut self,
        position: LogicalScreenPosition,
    ) -> Result<(), ImageVisualError> {
        self.rectangle.set_position(position)?;
        Ok(())
    }

    /// Replaces size while retaining position; invalid bounds change nothing.
    pub fn set_size(&mut self, size: LogicalScreenVector) -> Result<(), ImageVisualError> {
        self.rectangle.set_size(size)?;
        Ok(())
    }

    /// Replaces normalized straight-linear RGBA tint atomically.
    ///
    /// A non-finite channel or value outside `0.0..=1.0` changes nothing.
    pub fn set_tint(&mut self, tint: Color) -> Result<(), ImageVisualError> {
        self.rectangle.set_color(tint)?;
        Ok(())
    }

    /// Replaces the primary ordering layer within screen content.
    pub fn set_layer(&mut self, layer: Layer) {
        self.rectangle.set_layer(layer);
    }

    /// Replaces finite within-layer ordering depth; invalid input changes nothing.
    ///
    /// Negative depths are valid and do not change camera projection.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), ImageVisualError> {
        self.rectangle.set_draw_order_depth(depth)?;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), ImageVisualError> {
        self.rectangle.validate()?;
        if let Some(region) = self.source_region {
            region.validate_image(self.image)?;
        }
        Ok(())
    }
}

/// An invalid screen image value rejected before changing the visual.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ImageVisualError {
    /// Destination geometry, tint, or ordering failed screen validation.
    Screen(ScreenVisualError),
    /// A source region has zero extent or overflowing integer bounds.
    InvalidRegion {
        /// Rejected horizontal texel origin.
        x: u32,
        /// Rejected vertical texel origin.
        y: u32,
        /// Rejected texel width.
        width: u32,
        /// Rejected texel height.
        height: u32,
    },
    /// A valid source region extends beyond the selected image dimensions.
    RegionOutOfBounds {
        /// Rejected source region in texels.
        region: ImageRegion,
        /// Image whose dimensions the region must fit.
        image: ImageAssetId,
    },
}

impl From<ScreenVisualError> for ImageVisualError {
    fn from(source: ScreenVisualError) -> Self {
        Self::Screen(source)
    }
}

impl fmt::Display for ImageVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Screen(source) => {
                write!(formatter, "invalid screen image presentation: {source}")
            }
            Self::InvalidRegion {
                x,
                y,
                width,
                height,
            } => write!(
                formatter,
                "image source region ({x}, {y}, {width}, {height}) needs positive extents and non-overflowing texel bounds"
            ),
            Self::RegionOutOfBounds { region, image } => write!(
                formatter,
                "image source region {region:?} exceeds image dimensions {} by {}",
                image.width(),
                image.height()
            ),
        }
    }
}

impl Error for ImageVisualError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Screen(source) => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;

//! Bounded, application-owned straight-alpha sRGB RGBA8 image storage.

use std::{collections::TryReserveError, error::Error, fmt};

use crate::identity::ApplicationId;

/// Frozen limits for immutable CPU images registered before application startup.
///
/// Count and per-image dimensions are separate from total retained pixel Vec
/// capacity. Registry metadata is bounded by image count and is not charged to
/// the pixel budget. Zero limits are valid and can disable registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageAssetLimits {
    max_images: usize,
    max_width: u32,
    max_height: u32,
    max_pixel_bytes: usize,
}

impl ImageAssetLimits {
    /// Creates exact count, texel-dimension, and retained-pixel-capacity limits.
    ///
    /// No storage is allocated, and every value, including zero, is accepted.
    pub const fn new(
        max_images: usize,
        max_width: u32,
        max_height: u32,
        max_pixel_bytes: usize,
    ) -> Self {
        Self {
            max_images,
            max_width,
            max_height,
            max_pixel_bytes,
        }
    }

    /// Returns the maximum number of distinct registered image entries.
    pub const fn max_images(self) -> usize {
        self.max_images
    }

    /// Returns the maximum width of one image, in texels.
    pub const fn max_width(self) -> u32 {
        self.max_width
    }

    /// Returns the maximum height of one image, in texels.
    pub const fn max_height(self) -> u32 {
        self.max_height
    }

    /// Returns the maximum sum of retained pixel Vec capacities, in bytes.
    pub const fn max_pixel_bytes(self) -> usize {
        self.max_pixel_bytes
    }
}

impl Default for ImageAssetLimits {
    /// Permits 64 images, each at most 4096 by 4096 texels, within 64 MiB.
    fn default() -> Self {
        Self::new(64, 4096, 4096, 64 * 1024 * 1024)
    }
}

/// Opaque identity of one immutable image registered by an application.
///
/// Handles retain application provenance, registration identity, and validated
/// dimensions. They survive World replacement within their issuing application.
/// Registering equal pixels twice issues distinct handles. A handle is not a
/// filename, a save-file identifier, or a GPU resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageAssetId {
    application: ApplicationId,
    slot: usize,
    width: u32,
    height: u32,
}

impl ImageAssetId {
    /// Returns the immutable image width in texels, which is always positive.
    pub const fn width(self) -> u32 {
        self.width
    }

    /// Returns the immutable image height in texels, which is always positive.
    pub const fn height(self) -> u32 {
        self.height
    }
}

/// Read-only CPU pixels belonging to one registered application image.
///
/// Rows run top to bottom, texels within each row run left to right, and every
/// texel contains straight-alpha sRGB RGBA8 channels. There is no row padding.
/// Pixels stay owned by the application across World replacement; this type
/// exposes no mutation, decoder, platform, or GPU operation.
#[derive(Debug)]
pub struct ImageAsset {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl ImageAsset {
    /// Returns the positive image width in texels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Returns the positive image height in texels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Returns exactly width times height times four straight-alpha RGBA8 bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// A rejected immutable-image registration, leaving prior entries unchanged.
#[derive(Debug)]
#[non_exhaustive]
pub enum ImageAssetError {
    /// Width or height was zero.
    InvalidDimensions {
        /// Rejected width in texels.
        width: u32,
        /// Rejected height in texels.
        height: u32,
    },
    /// At least one dimension exceeded its configured maximum.
    DimensionsExceeded {
        /// Rejected width in texels.
        width: u32,
        /// Rejected height in texels.
        height: u32,
        /// Configured maximum width in texels.
        max_width: u32,
        /// Configured maximum height in texels.
        max_height: u32,
    },
    /// Width times height times four cannot be represented as a byte length.
    PixelByteCountOverflow {
        /// Rejected width in texels.
        width: u32,
        /// Rejected height in texels.
        height: u32,
    },
    /// The supplied slice did not contain exactly one RGBA8 value per texel.
    PixelLengthMismatch {
        /// Required byte length calculated from the validated dimensions.
        expected: usize,
        /// Supplied slice length in bytes.
        actual: usize,
    },
    /// The registry already contains its configured maximum image count.
    ImageLimitExceeded {
        /// Configured maximum number of registered entries.
        limit: usize,
    },
    /// New pixel storage would exceed the total retained-capacity budget.
    PixelByteLimitExceeded {
        /// Configured total retained-pixel-capacity limit in bytes.
        limit: usize,
        /// Pixel capacity already retained by successful registrations.
        retained: usize,
        /// Requested bytes, or actual Vec capacity if allocation reserved more.
        incoming: usize,
    },
    /// Pixel or registry metadata allocation failed before publication.
    AllocationFailed {
        /// The fallible allocator's original error.
        source: TryReserveError,
    },
}

impl fmt::Display for ImageAssetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions { width, height } => write!(
                formatter,
                "image dimensions must be positive, got {width} by {height}"
            ),
            Self::DimensionsExceeded {
                width,
                height,
                max_width,
                max_height,
            } => write!(
                formatter,
                "image dimensions {width} by {height} exceed limits {max_width} by {max_height}"
            ),
            Self::PixelByteCountOverflow { width, height } => write!(
                formatter,
                "RGBA8 byte length overflows for image dimensions {width} by {height}"
            ),
            Self::PixelLengthMismatch { expected, actual } => write!(
                formatter,
                "RGBA8 image requires exactly {expected} bytes, got {actual}"
            ),
            Self::ImageLimitExceeded { limit } => {
                write!(
                    formatter,
                    "image registry reached its limit of {limit} entries"
                )
            }
            Self::PixelByteLimitExceeded {
                limit,
                retained,
                incoming,
            } => write!(
                formatter,
                "image pixel capacity {retained} plus {incoming} bytes exceeds limit {limit}"
            ),
            Self::AllocationFailed { source } => {
                write!(formatter, "image asset storage allocation failed: {source}")
            }
        }
    }
}

impl Error for ImageAssetError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AllocationFailed { source } => Some(source),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(crate) struct ImageAssetRegistry {
    application: ApplicationId,
    limits: ImageAssetLimits,
    images: Vec<ImageAsset>,
    pixel_bytes: usize,
}

impl ImageAssetRegistry {
    pub(crate) const fn new(application: ApplicationId, limits: ImageAssetLimits) -> Self {
        Self {
            application,
            limits,
            images: Vec::new(),
            pixel_bytes: 0,
        }
    }

    pub(crate) fn register(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<ImageAssetId, ImageAssetError> {
        if width == 0 || height == 0 {
            return Err(ImageAssetError::InvalidDimensions { width, height });
        }
        if width > self.limits.max_width || height > self.limits.max_height {
            return Err(ImageAssetError::DimensionsExceeded {
                width,
                height,
                max_width: self.limits.max_width,
                max_height: self.limits.max_height,
            });
        }
        let pixel_length = usize::try_from(width)
            .ok()
            .zip(usize::try_from(height).ok())
            .and_then(|(width, height)| width.checked_mul(height))
            .and_then(|texels| texels.checked_mul(4))
            .ok_or(ImageAssetError::PixelByteCountOverflow { width, height })?;
        if pixels.len() != pixel_length {
            return Err(ImageAssetError::PixelLengthMismatch {
                expected: pixel_length,
                actual: pixels.len(),
            });
        }
        if self.images.len() >= self.limits.max_images {
            return Err(ImageAssetError::ImageLimitExceeded {
                limit: self.limits.max_images,
            });
        }
        self.check_pixel_capacity(pixel_length)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(pixel_length)
            .map_err(|source| ImageAssetError::AllocationFailed { source })?;
        // Vec's capacity is permitted to exceed the requested length. Charge
        // the actual retained capacity before publishing the entry.
        self.check_pixel_capacity(owned.capacity())?;
        owned.extend_from_slice(pixels);
        self.images
            .try_reserve_exact(1)
            .map_err(|source| ImageAssetError::AllocationFailed { source })?;
        let id = ImageAssetId {
            application: self.application,
            slot: self.images.len(),
            width,
            height,
        };
        self.pixel_bytes += owned.capacity();
        self.images.push(ImageAsset {
            width,
            height,
            pixels: owned,
        });
        Ok(id)
    }

    pub(crate) fn get(&self, id: ImageAssetId) -> Option<&ImageAsset> {
        if id.application != self.application {
            return None;
        }
        self.images
            .get(id.slot)
            .filter(|image| image.width == id.width && image.height == id.height)
    }

    pub(crate) fn len(&self) -> usize {
        self.images.len()
    }

    pub(crate) const fn pixel_bytes(&self) -> usize {
        self.pixel_bytes
    }

    fn check_pixel_capacity(&self, incoming: usize) -> Result<(), ImageAssetError> {
        if incoming > self.limits.max_pixel_bytes.saturating_sub(self.pixel_bytes) {
            return Err(ImageAssetError::PixelByteLimitExceeded {
                limit: self.limits.max_pixel_bytes,
                retained: self.pixel_bytes,
                incoming,
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;

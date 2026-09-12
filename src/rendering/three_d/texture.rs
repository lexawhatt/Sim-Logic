//! Immutable CPU texture snapshots and sampling descriptors, never GPU handles.

use std::{
    error::Error,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_engine::{Color, TextureAddressMode3d, TextureUvTransform3d};

use crate::screen::{ImageFilter, ImageRegion};

static NEXT_TEXTURE_REVISION: AtomicU64 = AtomicU64::new(1);

/// Shared immutable, top-left-origin sRGB RGBA8 pixels with straight alpha.
///
/// Construction requires an explicit source-capacity limit. Clones share CPU
/// pixels; editing returns a new snapshot and never modifies existing aliases.
/// Engine generates mipmaps and owns filtering, GPU uploads and recovery.
#[derive(Debug, Clone)]
pub struct TextureAsset3d {
    data: Arc<TextureData>,
}

#[derive(Debug)]
struct TextureData {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    revision: u64,
    parent: Option<u64>,
    region: Option<ImageRegion>,
}

impl TextureAsset3d {
    /// Takes exactly `width * height * 4` tightly packed bytes without copying.
    /// Dimensions must be positive, arithmetic must fit usize, and the Vec's
    /// capacity must fit the inclusive limit. Small Arc metadata is not charged.
    /// This validates CPU data only; device-specific limits are checked on upload.
    pub fn rgba8(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        max_source_bytes: usize,
    ) -> Result<Self, TextureVisualError> {
        let expected = pixel_bytes(width, height)?;
        if pixels.len() != expected {
            return Err(TextureVisualError::PixelLength {
                expected,
                actual: pixels.len(),
            });
        }
        check_bytes(pixels.capacity(), max_source_bytes)?;
        let revision = issue_revision()?;
        Ok(Self {
            data: Arc::new(TextureData {
                width,
                height,
                pixels,
                revision,
                parent: None,
                region: None,
            }),
        })
    }

    /// Returns the positive image width in texels.
    pub fn width(&self) -> u32 {
        self.data.width
    }

    /// Returns the positive image height in texels.
    pub fn height(&self) -> u32 {
        self.data.height
    }

    /// Returns tightly packed immutable base-level sRGB RGBA8 bytes.
    pub fn pixels(&self) -> &[u8] {
        &self.data.pixels
    }

    /// Returns retained base-level pixel capacity, excluding fixed Arc metadata.
    pub fn source_bytes(&self) -> usize {
        self.data.pixels.capacity()
    }

    /// Reports shared snapshot identity, not equality of pixel contents.
    pub fn shares_storage(&self, other: &Self) -> bool {
        self.key() == other.key()
    }

    /// Reports whether this snapshot was patched directly from the given snapshot.
    /// Equal pixel contents do not imply a revision relationship.
    pub fn is_direct_revision_of(&self, previous: &Self) -> bool {
        self.data.parent == Some(previous.key())
    }

    /// Copies a rectangular strided patch into a new complete CPU snapshot.
    ///
    /// `row_stride` is in bytes and must cover each patch row. The input length
    /// must equal `(height - 1) * row_stride + width * 4`; final-row padding is
    /// not supplied. The inclusive limit bounds this new pixel allocation;
    /// callers retaining old snapshots also retain their separately charged
    /// storage. Failure leaves all existing snapshots unchanged.
    ///
    /// Only the immediate parent's identity and dirty rectangle are retained,
    /// not its pixels or an edit history. Desktop can patch that exact revision;
    /// skipped or branched revisions safely fall back to a full replacement.
    pub fn with_region_update(
        &self,
        region: ImageRegion,
        row_stride: usize,
        pixels: &[u8],
        max_source_bytes: usize,
    ) -> Result<Self, TextureVisualError> {
        if region.x() + region.width() > self.width()
            || region.y() + region.height() > self.height()
        {
            return Err(TextureVisualError::RegionOutOfBounds);
        }
        let row_bytes = pixel_bytes(region.width(), 1)?;
        let required = (region.height() as usize - 1)
            .checked_mul(row_stride)
            .and_then(|bytes| bytes.checked_add(row_bytes))
            .ok_or(TextureVisualError::SizeOverflow)?;
        if row_stride < row_bytes || pixels.len() != required {
            return Err(TextureVisualError::InvalidPatchLayout);
        }
        let size = self.pixels().len();
        check_bytes(size, max_source_bytes)?;
        let mut revised = Vec::new();
        revised
            .try_reserve_exact(size)
            .map_err(|_| TextureVisualError::AllocationFailed {
                requested_bytes: size,
            })?;
        check_bytes(revised.capacity(), max_source_bytes)?;
        revised.extend_from_slice(self.pixels());
        let destination_stride = pixel_bytes(self.width(), 1)?;
        for row in 0..region.height() as usize {
            let destination =
                (region.y() as usize + row) * destination_stride + region.x() as usize * 4;
            let source = row * row_stride;
            revised[destination..destination + row_bytes]
                .copy_from_slice(&pixels[source..source + row_bytes]);
        }
        Ok(Self {
            data: Arc::new(TextureData {
                width: self.width(),
                height: self.height(),
                pixels: revised,
                revision: issue_revision()?,
                parent: Some(self.key()),
                region: Some(region),
            }),
        })
    }

    pub(crate) fn key(&self) -> u64 {
        self.data.revision
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn parent_key(&self) -> Option<u64> {
        self.data.parent
    }
    /// Returns the patch from the immediate parent, or None for a fresh image.
    /// It is not a cumulative patch relative to arbitrary older snapshots.
    pub fn updated_region(&self) -> Option<ImageRegion> {
        self.data.region
    }
}

impl PartialEq for TextureAsset3d {
    fn eq(&self, other: &Self) -> bool {
        self.shares_storage(other)
    }
}
impl Eq for TextureAsset3d {}

/// One shared CPU image with independent material sampling and tint.
///
/// Alpha is preserved; the mesh surface decides Opaque/Mask/Blend behavior.
/// Repeating or mipmapped packed-atlas tiles must first be isolated into separate
/// images. This descriptor neither shapes UVs nor generates mipmap pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct TextureVisual3d {
    asset: TextureAsset3d,
    filter: ImageFilter,
    mipmaps: bool,
    address: TextureAddressMode3d,
    transform: TextureUvTransform3d,
    tint: Color,
}

impl TextureVisual3d {
    /// Uses nearest filtering, mip zero, clamp addressing, identity UVs and white tint.
    pub fn new(asset: TextureAsset3d) -> Self {
        Self {
            asset,
            filter: ImageFilter::Nearest,
            mipmaps: false,
            address: TextureAddressMode3d::Clamp,
            transform: TextureUvTransform3d::IDENTITY,
            tint: Color::WHITE,
        }
    }
    /// Returns the shared immutable pixel snapshot.
    pub const fn asset(&self) -> &TextureAsset3d {
        &self.asset
    }
    /// Returns nearest or linear image filtering.
    pub const fn filter(&self) -> ImageFilter {
        self.filter
    }
    /// Reports whether Engine generates a complete mip chain.
    pub const fn mipmaps(&self) -> bool {
        self.mipmaps
    }
    /// Returns complete-image addressing, never packed-atlas tile addressing.
    pub const fn address_mode(&self) -> TextureAddressMode3d {
        self.address
    }
    /// Returns continuous scale then offset before sampling.
    pub const fn uv_transform(&self) -> TextureUvTransform3d {
        self.transform
    }
    /// Returns normalized straight-linear multiplicative RGBA tint.
    pub const fn tint(&self) -> Color {
        self.tint
    }
    /// Selects nearest or linear filtering without changing shared pixels.
    pub fn with_filter(mut self, filter: ImageFilter) -> Self {
        self.filter = filter;
        self
    }
    /// Requests a complete Engine-generated mip chain rather than mip zero only.
    /// Ordinary mip averaging does not preserve Mask alpha-test coverage.
    pub fn with_mipmaps(mut self, enabled: bool) -> Self {
        self.mipmaps = enabled;
        self
    }
    /// Selects clamp or hardware repetition of this complete independent image.
    pub fn with_address_mode(mut self, address: TextureAddressMode3d) -> Self {
        self.address = address;
        self
    }
    /// Selects an Engine-validated affine UV transform, including signed scales.
    pub fn with_uv_transform(mut self, transform: TextureUvTransform3d) -> Self {
        self.transform = transform;
        self
    }
    /// Replaces the pixel snapshot, preserving independent sampling and tint.
    pub fn with_asset(mut self, asset: TextureAsset3d) -> Self {
        self.asset = asset;
        self
    }
    /// Selects normalized straight-linear RGBA tint, rejecting invalid channels.
    pub fn with_tint(mut self, tint: Color) -> Result<Self, TextureVisualError> {
        if !tint.is_normalized() {
            return Err(TextureVisualError::InvalidTint);
        }
        self.tint = tint;
        Ok(self)
    }
    /// Returns exact nominal RGBA8 mip texel bytes, excluding driver metadata.
    /// This does not allocate or generate mip levels.
    pub fn texel_bytes(&self) -> Result<usize, TextureVisualError> {
        let (mut width, mut height) = (self.asset.width(), self.asset.height());
        let mut bytes = pixel_bytes(width, height)?;
        while self.mipmaps && (width > 1 || height > 1) {
            width = (width / 2).max(1);
            height = (height / 2).max(1);
            bytes = bytes
                .checked_add(pixel_bytes(width, height)?)
                .ok_or(TextureVisualError::SizeOverflow)?;
        }
        Ok(bytes)
    }
}

/// Invalid CPU texture data or a failed bounded snapshot allocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureVisualError {
    /// Image dimensions must both be positive.
    InvalidDimensions,
    /// Pixel or mip-chain byte arithmetic cannot be represented by usize.
    SizeOverflow,
    /// Base-level input length does not exactly match RGBA8 dimensions.
    PixelLength {
        /// Required tightly packed byte length.
        expected: usize,
        /// Supplied byte length.
        actual: usize,
    },
    /// Retained pixel capacity exceeds the explicit inclusive limit.
    SourceLimit {
        /// Required or allocated pixel capacity.
        requested: usize,
        /// Inclusive caller-provided capacity allowance.
        limit: usize,
    },
    /// A patch rectangle lies outside the image.
    RegionOutOfBounds,
    /// Patch row stride or exact input byte length is invalid.
    InvalidPatchLayout,
    /// Tint channels are not finite and normalized in 0..=1.
    InvalidTint,
    /// A fallible CPU pixel reservation was rejected.
    AllocationFailed {
        /// Requested new pixel capacity in bytes.
        requested_bytes: usize,
    },
    /// Process-local monotonic revision identities are exhausted.
    IdentityExhausted,
}

impl fmt::Display for TextureVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions => {
                formatter.write_str("3D texture dimensions must be positive")
            }
            Self::SizeOverflow => formatter.write_str("3D texture byte count overflows usize"),
            Self::PixelLength { expected, actual } => write!(
                formatter,
                "3D texture needs {expected} bytes, received {actual}"
            ),
            Self::SourceLimit { requested, limit } => write!(
                formatter,
                "3D texture capacity {requested} exceeds limit {limit}"
            ),
            Self::RegionOutOfBounds => {
                formatter.write_str("3D texture patch lies outside source image")
            }
            Self::InvalidPatchLayout => {
                formatter.write_str("3D texture patch stride or byte length is invalid")
            }
            Self::InvalidTint => formatter.write_str("3D texture tint must be normalized RGBA"),
            Self::AllocationFailed { requested_bytes } => {
                write!(formatter, "cannot reserve {requested_bytes} texture bytes")
            }
            Self::IdentityExhausted => {
                formatter.write_str("3D texture revision identities exhausted")
            }
        }
    }
}
impl Error for TextureVisualError {}

fn pixel_bytes(width: u32, height: u32) -> Result<usize, TextureVisualError> {
    if width == 0 || height == 0 {
        return Err(TextureVisualError::InvalidDimensions);
    }
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|value| value.checked_mul(4))
        .ok_or(TextureVisualError::SizeOverflow)
}

fn check_bytes(requested: usize, limit: usize) -> Result<(), TextureVisualError> {
    if requested > limit {
        return Err(TextureVisualError::SourceLimit { requested, limit });
    }
    Ok(())
}

fn issue_revision() -> Result<u64, TextureVisualError> {
    NEXT_TEXTURE_REVISION
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| TextureVisualError::IdentityExhausted)
}

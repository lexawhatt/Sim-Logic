//! Immutable CPU assets owned by an application across World replacement.
//!
//! Registration is bounded and takes place before the runner starts. Image
//! handles and pixel inspection require neither a window nor a GPU.

mod image;

pub(crate) use image::ImageAssetRegistry;
pub use image::{ImageAsset, ImageAssetError, ImageAssetId, ImageAssetLimits};

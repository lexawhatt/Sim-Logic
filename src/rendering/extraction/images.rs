//! CPU-only image records and application-provenance preflight.

use sim_engine::{Color, Layer, LogicalScreenPosition, LogicalScreenVector};

use crate::{
    assets::{ImageAssetId, ImageAssetRegistry},
    identity::{LogicEntity, WorldGeneration},
    screen::{ImageFilter, ImageRegion, ScreenImageVisual},
};

use super::super::ExtractionError;

/// One immutable screen-image description in a published CPU snapshot.
///
/// Positions are current logical pixels, independent of camera interpolation.
/// The registry owns its source pixels; this record never holds a GPU handle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedScreenImage {
    source: LogicEntity,
    visual: ScreenImageVisual,
}

impl ResolvedScreenImage {
    /// Returns the managed entity that produced this image.
    pub const fn source(self) -> LogicEntity {
        self.source
    }
    /// Returns the immutable application-registered image identity.
    pub const fn image(self) -> ImageAssetId {
        self.visual.image()
    }
    /// Returns logical screen top-left, with y increasing downward.
    pub const fn position(self) -> LogicalScreenPosition {
        self.visual.position()
    }
    /// Returns strictly positive logical width and height.
    pub const fn size(self) -> LogicalScreenVector {
        self.visual.size()
    }
    /// Returns normalized straight-linear RGBA multiplied with decoded pixels.
    pub const fn tint(self) -> Color {
        self.visual.tint()
    }
    /// Returns the optional source texel region; None means the complete image.
    pub const fn source_region(self) -> Option<ImageRegion> {
        self.visual.source_region()
    }
    /// Returns nearest or linear texture sampling.
    pub const fn filter(self) -> ImageFilter {
        self.visual.filter()
    }
    /// Returns the primary mixed-screen ordering layer.
    pub const fn layer(self) -> Layer {
        self.visual.layer()
    }
    /// Returns finite mixed-screen ordering depth, not geometric camera depth.
    pub const fn draw_order_depth(self) -> f32 {
        self.visual.draw_order_depth()
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenImageSource {
    entity: LogicEntity,
    visual: ScreenImageVisual,
    registered: bool,
}

impl ScreenImageSource {
    pub(crate) fn new(
        entity: LogicEntity,
        visual: &ScreenImageVisual,
        registry: &ImageAssetRegistry,
    ) -> Self {
        Self {
            entity,
            visual: *visual,
            registered: registry.get(visual.image()).is_some(),
        }
    }

    pub(super) fn resolve(
        self,
        generation: WorldGeneration,
    ) -> Result<ResolvedScreenImage, ExtractionError> {
        if self.entity.world_generation() != generation {
            return Err(ExtractionError::ForeignEntity {
                entity: self.entity,
                expected: generation,
            });
        }
        if !self.registered {
            return Err(ExtractionError::UnregisteredImageAsset {
                entity: self.entity,
                image: self.visual.image(),
            });
        }
        self.visual
            .validate()
            .map_err(|error| ExtractionError::InvalidScreenImage {
                entity: self.entity,
                error,
            })?;
        Ok(ResolvedScreenImage {
            source: self.entity,
            visual: self.visual,
        })
    }
}

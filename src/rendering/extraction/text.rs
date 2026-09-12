//! Shared prepared labels and application-provenance checks without GPU work.

use sim_engine::{Color, Layer, LogicalScreenPosition};

use crate::{
    identity::{LogicEntity, WorldGeneration},
    text::{ScreenTextVisual, TextFont, TextMetrics, TextRegistry},
};

use super::super::ExtractionError;

/// One prepared screen label retained by a complete CPU snapshot.
///
/// Text and its immutable font are shared with the component; extraction does
/// not reshape strings, copy font bytes, or hold a renderer resource. Positions
/// use current logical pixels, independently of fixed-step interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedScreenText {
    source: LogicEntity,
    visual: ScreenTextVisual,
}

impl ResolvedScreenText {
    /// Returns the managed entity that produced this label.
    pub const fn source(&self) -> LogicEntity {
        self.source
    }

    /// Returns the immutable application-registered font used for preparation.
    pub fn font(&self) -> &TextFont {
        self.visual.font()
    }

    /// Returns the shared UTF-8 source string.
    pub fn text(&self) -> &str {
        self.visual.text()
    }

    /// Returns shaping metrics already calculated when the label was prepared.
    pub fn metrics(&self) -> TextMetrics {
        self.visual.metrics()
    }

    /// Returns the requested baseline anchor in logical screen pixels.
    pub fn position(&self) -> LogicalScreenPosition {
        self.visual.position()
    }

    /// Returns the left baseline origin after horizontal alignment is applied.
    pub fn baseline_origin(&self) -> LogicalScreenPosition {
        self.visual.baseline_origin()
    }

    /// Returns normalized straight-linear RGBA tint for this placement.
    pub fn tint(&self) -> Color {
        self.visual.tint()
    }

    /// Returns the primary mixed-screen painter-order layer.
    pub fn layer(&self) -> Layer {
        self.visual.layer()
    }

    /// Returns secondary mixed-screen painter order, not geometric camera depth.
    pub fn draw_order_depth(&self) -> f32 {
        self.visual.draw_order_depth()
    }

    /// Returns the complete immutable snapshot value, including shared preparation.
    pub const fn visual(&self) -> &ScreenTextVisual {
        &self.visual
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ScreenTextSource {
    entity: LogicEntity,
    visual: ScreenTextVisual,
    registered: bool,
}

impl ScreenTextSource {
    pub(crate) fn new(
        entity: LogicEntity,
        visual: &ScreenTextVisual,
        registry: &TextRegistry,
    ) -> Self {
        Self {
            entity,
            visual: visual.clone(),
            registered: registry.contains(visual.font()),
        }
    }

    pub(super) fn resolve(
        self,
        generation: WorldGeneration,
    ) -> Result<ResolvedScreenText, ExtractionError> {
        if self.entity.world_generation() != generation {
            return Err(ExtractionError::ForeignEntity {
                entity: self.entity,
                expected: generation,
            });
        }
        if !self.registered {
            return Err(ExtractionError::UnregisteredTextFont {
                entity: self.entity,
                font: self.visual.font().clone(),
            });
        }
        self.visual
            .validate()
            .map_err(|error| ExtractionError::InvalidScreenText {
                entity: self.entity,
                error,
            })?;
        Ok(ResolvedScreenText {
            source: self.entity,
            visual: self.visual,
        })
    }
}

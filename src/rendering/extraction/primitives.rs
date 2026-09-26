//! Ordered screen-vector records; rendering remains Engine-owned.

use super::ResolvedScreenRectangle;
use crate::{
    identity::LogicEntity,
    screen::{ScreenCircleVisual, ScreenLineVisual},
};
use sim_engine::{Layer, SceneError, ScreenScene};

/// One sampled screen primitive. Images and labels remain separate retained sources.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ResolvedScreenPrimitive {
    /// A square/rounded filled rectangle, with its optional outline and clip.
    Rectangle(ResolvedScreenRectangle),
    /// A presentation-only line, sampled without fixed interpolation.
    Line {
        /// Managed entity producing this line.
        source: LogicEntity,
        /// Exact sampled screen geometry and style.
        visual: ScreenLineVisual,
    },
    /// A presentation-only filled circle, sampled without fixed interpolation.
    Circle {
        /// Managed entity producing this circle.
        source: LogicEntity,
        /// Exact sampled screen geometry and style.
        visual: ScreenCircleVisual,
    },
}

impl ResolvedScreenPrimitive {
    /// Returns the managed source identity.
    pub const fn source(self) -> LogicEntity {
        match self {
            Self::Rectangle(value) => value.source(),
            Self::Line { source, .. } | Self::Circle { source, .. } => source,
        }
    }
    /// Returns the mixed-screen ordering layer.
    pub const fn layer(self) -> Layer {
        match self {
            Self::Rectangle(value) => value.layer(),
            Self::Line { visual, .. } => visual.layer(),
            Self::Circle { visual, .. } => visual.layer(),
        }
    }
    /// Returns within-layer ordering depth, not camera depth.
    pub const fn draw_order_depth(self) -> f32 {
        match self {
            Self::Rectangle(value) => value.draw_order_depth(),
            Self::Line { visual, .. } => visual.draw_order_depth(),
            Self::Circle { visual, .. } => visual.draw_order_depth(),
        }
    }
    pub(super) const fn kind_order(self) -> u8 {
        match self {
            Self::Rectangle(_) => 0,
            Self::Line { .. } => 1,
            Self::Circle { .. } => 2,
        }
    }
    pub(crate) fn append(self, scene: &mut ScreenScene) -> Result<(), SceneError> {
        match self {
            Self::Rectangle(value) => value.visual().append(scene),
            Self::Line { visual, .. } => visual.append(scene),
            Self::Circle { visual, .. } => visual.append(scene),
        }
    }
}

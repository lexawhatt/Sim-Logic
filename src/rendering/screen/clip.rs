//! Explicit, allocation-free screen clipping, shared by rendering and hit tests.

use crate::input::PointerSample;
use sim_engine::{LogicalScreenPosition, LogicalScreenVector, SceneError, ScreenClipRect};

/// A logical-pixel clip scope. Disjoint intersections are empty, not unclipped.
///
/// Build parent/child scopes with `intersection` and assign the resulting value
/// to each visual. This has constant storage and no implicit entity hierarchy.
/// Clips stay fixed in screen space when content moves, rotates, or scrolls.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum ScreenClip {
    /// No additional clip beyond the viewport.
    #[default]
    Unclipped,
    /// Nothing is drawn or hit. Still counted in source/extraction budgets.
    Empty,
    /// A validated rectangle in top-left/downward logical pixels.
    Rectangle(ScreenClipRect),
}

impl ScreenClip {
    /// Constructs a nonempty clip; invalid bounds return Engine's typed error.
    /// Negative sizes follow Engine's normalization policy.
    pub fn new(
        position: LogicalScreenPosition,
        size: LogicalScreenVector,
    ) -> Result<Self, SceneError> {
        Ok(Self::Rectangle(ScreenClipRect::from_min_size(
            position, size,
        )?))
    }

    /// Intersects two scopes, preserving empty intersections without allocations.
    pub fn intersection(self, other: Self) -> Self {
        match (self, other) {
            (Self::Empty, _) | (_, Self::Empty) => Self::Empty,
            (Self::Unclipped, clip) | (clip, Self::Unclipped) => clip,
            (Self::Rectangle(a), Self::Rectangle(b)) => {
                a.intersection(b).map_or(Self::Empty, Self::Rectangle)
            }
        }
    }

    /// Tests viewport and half-open clip bounds. Does not test the visual itself.
    pub fn contains_pointer(self, sample: PointerSample) -> bool {
        sample.is_inside_viewport() && self.contains(sample.position())
    }

    pub(super) fn contains(self, position: LogicalScreenPosition) -> bool {
        let p = position.to_vec2();
        match self {
            Self::Unclipped => position.is_finite(),
            Self::Empty => false,
            Self::Rectangle(rect) => {
                let min = rect.min().to_vec2();
                let max = rect.max().to_vec2();
                p.x() >= min.x() && p.y() >= min.y() && p.x() < max.x() && p.y() < max.y()
            }
        }
    }
}

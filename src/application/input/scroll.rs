//! Bounded wheel deltas and frame-only, event-ordered pointer scrolling.

use std::{error::Error, fmt};

use super::{Action, ActionEdge, PointerSample};

/// Units preserved from the input device; no line-to-pixel conversion is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollUnit {
    /// Fractional wheel lines, independent from display scaling.
    Lines,
    /// Logical screen pixels, converted from physical pixels by the desktop adapter.
    Pixels,
}

/// One finite wheel displacement with explicit units and no duration scaling.
///
/// Positive X scrolls right and positive Y scrolls up. Platform scrolling
/// preferences are preserved; applications choose their own zoom or pan policy.
/// The runtime never combines separate events or invents a pixels-per-line ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollDelta {
    unit: ScrollUnit,
    bits: [u64; 2],
}

impl ScrollDelta {
    /// Inclusive absolute limit of each event's coordinates in its declared units.
    pub const MAX_DISPLACEMENT: f64 = 1_000_000.0;

    /// Validates fractional line displacements without changing their units.
    pub fn lines(x: f64, y: f64) -> Result<Self, ScrollDeltaError> {
        Self::new(ScrollUnit::Lines, x, y)
    }

    /// Validates logical-pixel displacements, not physical pixels or world units.
    pub fn pixels(x: f64, y: f64) -> Result<Self, ScrollDeltaError> {
        Self::new(ScrollUnit::Pixels, x, y)
    }

    fn new(unit: ScrollUnit, x: f64, y: f64) -> Result<Self, ScrollDeltaError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(ScrollDeltaError::NonFinite);
        }
        if x.abs() > Self::MAX_DISPLACEMENT || y.abs() > Self::MAX_DISPLACEMENT {
            return Err(ScrollDeltaError::OutOfRange);
        }
        Ok(Self {
            unit,
            bits: [canonical_bits(x), canonical_bits(y)],
        })
    }

    /// Returns the original line or logical-pixel units.
    pub const fn unit(self) -> ScrollUnit {
        self.unit
    }

    /// Horizontal displacement, positive to the right.
    pub const fn x(self) -> f64 {
        f64::from_bits(self.bits[0])
    }

    /// Vertical displacement, positive upward.
    pub const fn y(self) -> f64 {
        f64::from_bits(self.bits[1])
    }
}

fn canonical_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// A wheel coordinate was rejected before it could enter an input batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollDeltaError {
    /// A coordinate was NaN or infinite.
    NonFinite,
    /// A coordinate exceeded [`ScrollDelta::MAX_DISPLACEMENT`].
    OutOfRange,
}

impl fmt::Display for ScrollDeltaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFinite => "wheel displacement must be finite",
            Self::OutOfRange => "wheel displacement exceeds the per-event limit",
        })
    }
}

impl Error for ScrollDeltaError {}

/// One frame-only wheel occurrence and the pointer known when it arrived.
///
/// No binding or transition token is required. These events are not retained
/// for fixed ticks or replayed into replacement worlds. A focus-loss boundary
/// discards every wheel occurrence in that accepted frame, before and after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PointerScrollEvent {
    pub(super) delta: ScrollDelta,
    pub(super) pointer: Option<PointerSample>,
    pub(super) ordinal: usize,
}

impl PointerScrollEvent {
    /// Returns this event's displacement, including its original units.
    pub const fn delta(self) -> ScrollDelta {
        self.delta
    }

    /// Returns the event-time logical position, or None before motion or after
    /// pointer leave/capture. Later pointer movement cannot change this sample.
    pub const fn pointer(self) -> Option<PointerSample> {
        self.pointer
    }
}

/// Ordered frame input for interactions that combine clicks and wheel scrolling.
///
/// Continuous absolute motion still lives in [`super::FrameInput::pointer`];
/// mouse actions and wheel events retain their own event-time pointer samples.
/// This iterator view allocates no additional event buffer or transition tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FrameInputEvent<A: Action> {
    /// A bound button edge, including synthesized cancellation releases.
    Action(ActionEdge<A>),
    /// An unbound, frame-only wheel occurrence.
    Scroll(PointerScrollEvent),
}

pub(super) fn ordered_events<A: Action>(
    edges: &[ActionEdge<A>],
    scrolls: &[PointerScrollEvent],
) -> impl Iterator<Item = FrameInputEvent<A>> {
    let mut edges = edges.iter().copied().peekable();
    let mut scrolls = scrolls.iter().copied().peekable();
    std::iter::from_fn(move || match (edges.peek(), scrolls.peek()) {
        (Some(edge), Some(scroll)) if edge.ordinal <= scroll.ordinal => {
            edges.next().map(FrameInputEvent::Action)
        }
        (Some(_), Some(_)) | (None, Some(_)) => scrolls.next().map(FrameInputEvent::Scroll),
        (Some(_), None) => edges.next().map(FrameInputEvent::Action),
        (None, None) => None,
    })
}

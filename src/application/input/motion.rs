//! Relative pointer displacement in device motion units, not window coordinates.

use std::{error::Error, fmt};

/// Finite relative mouse displacement; positive X is right, positive Y is down.
///
/// These are raw device motion units, not logical pixels or world distances.
/// Apply application sensitivity once, without multiplying by frame duration.
/// The absolute per-axis bound also applies to each accumulated frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct RelativePointerMotion {
    bits: [u64; 2],
}

impl RelativePointerMotion {
    /// No displacement.
    pub const ZERO: Self = Self { bits: [0; 2] };
    /// Inclusive absolute per-axis event and frame displacement ceiling.
    pub const MAX_DISPLACEMENT: f64 = 1_000_000.0;

    /// Validates both coordinates before constructing a portable input value.
    pub fn new(x: f64, y: f64) -> Result<Self, RelativePointerMotionError> {
        if !x.is_finite() || !y.is_finite() {
            return Err(RelativePointerMotionError::NonFinite);
        }
        if x.abs() > Self::MAX_DISPLACEMENT || y.abs() > Self::MAX_DISPLACEMENT {
            return Err(RelativePointerMotionError::OutOfRange);
        }
        Ok(Self {
            bits: [canonical_bits(x), canonical_bits(y)],
        })
    }
    /// Horizontal raw displacement, positive to the right.
    pub const fn x(self) -> f64 {
        f64::from_bits(self.bits[0])
    }
    /// Vertical raw displacement, positive downward.
    pub const fn y(self) -> f64 {
        f64::from_bits(self.bits[1])
    }
    /// Adds displacements within the same explicit bounds, or changes nothing.
    pub fn checked_add(self, other: Self) -> Result<Self, RelativePointerMotionError> {
        Self::new(self.x() + other.x(), self.y() + other.y())
    }
}

fn canonical_bits(value: f64) -> u64 {
    if value == 0.0 { 0 } else { value.to_bits() }
}

/// A relative motion event or accumulated frame exceeded its numeric contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelativePointerMotionError {
    /// A coordinate was NaN or infinite.
    NonFinite,
    /// A coordinate exceeded the inclusive absolute displacement ceiling.
    OutOfRange,
}

impl fmt::Display for RelativePointerMotionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NonFinite => "relative pointer motion must be finite",
            Self::OutOfRange => "relative pointer motion exceeds the displacement limit",
        })
    }
}
impl Error for RelativePointerMotionError {}

pub(super) fn frame_motion(
    events: &[super::InputEvent],
) -> Result<(RelativePointerMotion, bool), RelativePointerMotionError> {
    let mut motion = RelativePointerMotion::ZERO;
    let mut focus_lost = false;
    for event in events {
        match event {
            super::InputEvent::RelativePointerMotion { motion: incoming } if !focus_lost => {
                motion = motion.checked_add(*incoming)?;
            }
            super::InputEvent::FocusLost => {
                motion = RelativePointerMotion::ZERO;
                focus_lost = true;
            }
            _ => {}
        }
    }
    Ok((motion, focus_lost))
}

//! Validated logical pointer coordinates and portable mouse buttons.

use std::{
    error::Error,
    fmt,
    hash::{Hash, Hasher},
};

use sim_engine::{Camera2d, Camera2dError, LogicalScreenPosition, LogicalViewport, Vec2};

/// A portable mouse button supported by the desktop and headless adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MouseButton {
    /// The primary (left) physical mouse button.
    Left,
    /// The secondary (right) physical mouse button.
    Right,
    /// The middle physical mouse button, usually the wheel button.
    Middle,
}

pub(super) const ALL_MOUSE_BUTTONS: [MouseButton; 3] =
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle];

pub(super) const fn mouse_button_index(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
    }
}

/// Rejection that preserves an existing mouse-button binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateMouseBinding {
    pub(super) button: MouseButton,
}

impl DuplicateMouseBinding {
    /// Returns the physical button that was already bound.
    pub const fn button(self) -> MouseButton {
        self.button
    }
}

impl fmt::Display for DuplicateMouseBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "mouse button {:?} is already bound", self.button)
    }
}

impl Error for DuplicateMouseBinding {}

/// Rejection of a pointer position containing NaN or infinity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerSampleError;

impl fmt::Display for PointerSampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("pointer coordinates must be finite logical pixels")
    }
}

impl Error for PointerSampleError {}

/// A finite pointer position and its event-time logical viewport.
///
/// Both use logical pixels with a top-left origin and downward-positive y.
/// Positions outside the viewport are allowed and are never clamped. Values
/// are immutable, so an action edge can preserve a click's position even when
/// later motion, resizing, or a DPI change updates the current pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerSample {
    position: LogicalScreenPosition,
    viewport: LogicalViewport,
}

impl PointerSample {
    /// Captures coordinates without requiring a window or renderer.
    ///
    /// Rejects non-finite positions. The supplied viewport already guarantees
    /// finite, strictly positive dimensions through its own constructor.
    pub fn new(
        position: LogicalScreenPosition,
        viewport: LogicalViewport,
    ) -> Result<Self, PointerSampleError> {
        if !position.is_finite() {
            return Err(PointerSampleError);
        }
        Ok(Self { position, viewport })
    }

    /// Returns the captured top-left-relative logical position.
    pub const fn position(self) -> LogicalScreenPosition {
        self.position
    }

    /// Returns the viewport captured alongside this position.
    pub const fn viewport(self) -> LogicalViewport {
        self.viewport
    }

    pub(crate) fn is_inside_viewport(self) -> bool {
        let point = self.position.to_vec2();
        point.x() >= 0.0
            && point.y() >= 0.0
            && point.x() < self.viewport.width()
            && point.y() < self.viewport.height()
    }

    /// Converts this sample to the supplied camera's depth-zero world plane.
    ///
    /// This delegates exactly to [`Camera2d::screen_to_world`], preserving its
    /// projection errors. No active or interpolated camera is chosen implicitly;
    /// callers decide which camera snapshot their interaction should use.
    pub fn world_position(self, camera: Camera2d) -> Result<Vec2, Camera2dError> {
        camera.screen_to_world(self.position, self.viewport)
    }
}

// Finite construction makes float equality reflexive. Hash canonicalizes the
// two signed zero representations, which compare equal under PartialEq.
impl Eq for PointerSample {}

impl Hash for PointerSample {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let position = self.position.to_vec2();
        for value in [
            position.x(),
            position.y(),
            self.viewport.width(),
            self.viewport.height(),
        ] {
            let bits = if value == 0.0 { 0 } else { value.to_bits() };
            bits.hash(state);
        }
    }
}

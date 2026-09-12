//! Physical control provenance and explicit cancellation, without UI routing.

use super::{ALL_MOUSE_BUTTONS, ALL_PHYSICAL_KEYS, MouseButton, PhysicalKeyCode};

/// Physical control that produced one logical action occurrence.
///
/// Two controls bound to the same action remain distinguishable even when a
/// mouse occurrence has no known pointer position. This identifies the supported
/// control, not a particular keyboard, mouse device, or window. It requires no
/// platform or renderer resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputControl {
    /// A physical keyboard location, independent of typed text or keyboard layout.
    Key(PhysicalKeyCode),
    /// One of the supported mouse buttons.
    MouseButton(MouseButton),
}

/// Why a held control was released without an ordinary release event.
///
/// Cancellation releases have no pointer sample. They still participate in
/// normal bounded, ordered action delivery and have their own occurrence token;
/// they do not consume input or silently undo earlier press effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InputCancellationReason {
    /// Pointer tracking ended, for example on cursor leave or a zero-size
    /// desktop viewport. Only held mouse controls are cancelled.
    PointerLeft,
    /// The host reported focus loss. Held keyboard and mouse controls are
    /// cancelled, and the continuous pointer is cleared.
    FocusLost,
}

pub(super) fn cancelled_controls(
    reason: InputCancellationReason,
) -> impl Iterator<Item = InputControl> {
    let keys: &[PhysicalKeyCode] = match reason {
        InputCancellationReason::PointerLeft => &[],
        InputCancellationReason::FocusLost => &ALL_PHYSICAL_KEYS,
    };
    keys.iter()
        .copied()
        .map(InputControl::Key)
        .chain(ALL_MOUSE_BUTTONS.into_iter().map(InputControl::MouseButton))
}

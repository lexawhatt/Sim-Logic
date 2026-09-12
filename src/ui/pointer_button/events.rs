use crate::{
    identity::TransitionIntentToken,
    input::{InputCancellationReason, PointerSample},
};

/// Why a captured button interaction ended without a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerButtonCancellation {
    /// The input runtime cancelled the physical mouse hold.
    Input(InputCancellationReason),
    /// The release was outside the captured target, outside the viewport, or
    /// over a target the caller no longer considers eligible.
    ReleasedOutside,
    /// An ordinary release had no known pointer position.
    MissingPointer,
}

/// One change emitted while interpreting an individual mouse action edge.
///
/// Targets are caller-owned identities. Samples and tokens come from this
/// exact edge, not the frame's latest pointer or the original press. Tokens
/// retain their existing runtime lifetime; storing an event does not extend it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PointerButtonEvent<T> {
    /// A known in-viewport press captured an eligible target.
    Pressed {
        /// Target chosen by the caller at the press position.
        target: T,
        /// The press's immutable event-time pointer sample.
        pointer: PointerSample,
        /// The press occurrence's runtime-issued token.
        intent: TransitionIntentToken,
    },
    /// A normal release over that same eligible target confirmed a click.
    Clicked {
        /// Original captured target, equal to the release's eligible hit.
        target: T,
        /// The release's immutable event-time pointer sample.
        pointer: PointerSample,
        /// The release occurrence's runtime-issued token.
        intent: TransitionIntentToken,
    },
    /// A captured hold ended without confirmation.
    Cancelled {
        /// Original captured target; it must not receive a click.
        target: T,
        /// Why confirmation was rejected.
        reason: PointerButtonCancellation,
        /// The release or cancellation occurrence's runtime-issued token.
        intent: TransitionIntentToken,
    },
}

/// Local routing advice and optional interaction change for one input edge.
///
/// A claimed edge can have no event, for example when an invalidated target's
/// pending release is suppressed. No input snapshot is changed by this result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "route claimed input separately from application/gameplay actions"]
pub struct PointerButtonOutcome<T> {
    pub(super) claimed: bool,
    pub(super) event: Option<PointerButtonEvent<T>>,
}

impl<T: Copy> PointerButtonOutcome<T> {
    /// Whether this controller owns the edge for local routing purposes.
    ///
    /// Do not forward claimed edges to a competing gesture handler. This is
    /// advice to the caller, not exclusive access, global input consumption,
    /// or cancellation of Systems that already ran.
    pub const fn claimed(self) -> bool {
        self.claimed
    }

    /// Returns the press, confirmed click, or cancellation emitted by this edge.
    /// No event is emitted for unrelated controls or a suppressed interaction.
    pub const fn event(self) -> Option<PointerButtonEvent<T>> {
        self.event
    }
}

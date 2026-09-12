use crate::input::{Action, ActionEdge, ButtonState, InputControl, MouseButton};

mod events;
pub use events::{PointerButtonCancellation, PointerButtonEvent, PointerButtonOutcome};

#[derive(Debug, Clone, Copy)]
enum Capture<T> {
    Idle,
    Outside,
    Target(T),
    Suppressed,
}

/// Tracks press-to-release ownership for one physical mouse button.
///
/// The caller selects an eligible target at each edge's pointer position, then
/// feeds the complete ordered [`crate::input::FrameInput::edges`] sequence once.
/// Never also feed its retained FixedInput copies: replaying a completed pair
/// can confirm a second click. Keyboard aliases and other mouse buttons cannot
/// disturb this controller. A press outside all eligible targets cannot become
/// a click merely by releasing over a button later.
///
/// This is a headless state machine, not a widget renderer, drag-motion stream,
/// OS pointer grab, keyboard-focus tree, or global input-consumption service.
/// Processing allocates no internal storage and registers no callbacks. Target
/// equality uses the caller's `Eq` implementation, whose costs remain its own.
///
/// Target liveness and overlap priority belong to the caller. Invalidate capture
/// with [`Self::cancel`] when the target is removed, disabled or made ineligible.
/// Use generation-qualified IDs such as [`crate::identity::LogicEntity`] for ECS
/// targets. Prefer a World-local Resource recreated on replacement; a persistent
/// controller must be cancelled at world changes and continue receiving release
/// boundaries. Keeping a controller in an inactive world can miss its release.
#[derive(Debug)]
pub struct PointerButton<T: Copy + Eq> {
    button: MouseButton,
    capture: Capture<T>,
}

impl<T: Copy + Eq> PointerButton<T> {
    /// Creates an idle controller without allocating or requiring a renderer.
    /// Only action edges from this physical mouse button are interpreted.
    pub const fn new(button: MouseButton) -> Self {
        Self {
            button,
            capture: Capture::Idle,
        }
    }

    /// Returns the configured physical mouse button, independent of action names.
    pub const fn button(&self) -> MouseButton {
        self.button
    }

    /// Returns the original pressed target while it still owns the gesture.
    /// Use this for held styling, not as proof that the pointer is still inside.
    pub const fn captured(&self) -> Option<T> {
        match self.capture {
            Capture::Target(target) => Some(target),
            _ => None,
        }
    }

    /// Invalidates a captured target and returns it for caller-owned cleanup.
    ///
    /// Its remaining matching mouse edges stay claimed until release. A removed
    /// or disabled button therefore cannot transfer an in-progress UI gesture
    /// to the gameplay beneath it, even if the same target value reappears.
    /// Without capture this does nothing. It neither invents an input token nor
    /// emits an event; the caller knows why it explicitly invalidated the target.
    pub fn cancel(&mut self) -> Option<T> {
        let target = self.captured()?;
        self.capture = Capture::Suppressed;
        Some(target)
    }

    /// Interprets one ordered edge using its caller-selected eligible hit target.
    ///
    /// Compute `hit` from this edge's sample, not the frame's latest pointer.
    /// Unknown or out-of-viewport samples cannot capture or confirm even if
    /// `hit` is Some. Normal release over the captured target clicks; release
    /// elsewhere or runtime cancellation ends capture without confirmation.
    /// Multiple pairs in one frame are processed in their supplied order.
    ///
    /// No snapshot is consumed. Inspect [`PointerButtonOutcome::claimed`] before
    /// forwarding this edge to gameplay. FixedUpdate precedes FrameUpdate;
    /// routing here cannot undo a game action already applied in FixedUpdate.
    /// Route competing actions together, or queue accepted gameplay intents for
    /// a later fixed tick. This helper does not change the runtime schedule.
    pub fn process<A: Action>(
        &mut self,
        edge: ActionEdge<A>,
        hit: Option<T>,
    ) -> PointerButtonOutcome<T> {
        if edge.control() != InputControl::MouseButton(self.button) {
            return outcome(false, None);
        }
        match edge.state() {
            ButtonState::Pressed => match self.capture {
                Capture::Target(_) | Capture::Suppressed => outcome(true, None),
                Capture::Outside => outcome(false, None),
                Capture::Idle => {
                    if let Some((target, pointer)) = hit.zip(edge.pointer())
                        && pointer.is_inside_viewport()
                    {
                        self.capture = Capture::Target(target);
                        outcome(
                            true,
                            Some(PointerButtonEvent::Pressed {
                                target,
                                pointer,
                                intent: edge.intent(),
                            }),
                        )
                    } else {
                        self.capture = Capture::Outside;
                        outcome(false, None)
                    }
                }
            },
            ButtonState::Released => {
                let capture = std::mem::replace(&mut self.capture, Capture::Idle);
                match capture {
                    Capture::Idle | Capture::Outside => outcome(false, None),
                    Capture::Suppressed => outcome(true, None),
                    Capture::Target(target) => {
                        let reason = if let Some(reason) = edge.cancellation_reason() {
                            PointerButtonCancellation::Input(reason)
                        } else if let Some(pointer) = edge.pointer() {
                            if pointer.is_inside_viewport() && hit == Some(target) {
                                return outcome(
                                    true,
                                    Some(PointerButtonEvent::Clicked {
                                        target,
                                        pointer,
                                        intent: edge.intent(),
                                    }),
                                );
                            }
                            PointerButtonCancellation::ReleasedOutside
                        } else {
                            PointerButtonCancellation::MissingPointer
                        };
                        outcome(
                            true,
                            Some(PointerButtonEvent::Cancelled {
                                target,
                                reason,
                                intent: edge.intent(),
                            }),
                        )
                    }
                }
            }
        }
    }
}

fn outcome<T>(claimed: bool, event: Option<PointerButtonEvent<T>>) -> PointerButtonOutcome<T> {
    PointerButtonOutcome { claimed, event }
}

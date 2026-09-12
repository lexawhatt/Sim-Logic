//! Typed, platform-independent keyboard and pointer input.

#[path = "input/control.rs"]
mod control;
use control::cancelled_controls;
pub use control::{InputCancellationReason, InputControl};

#[path = "input/pointer.rs"]
mod pointer;

use pointer::{ALL_MOUSE_BUTTONS, mouse_button_index};
pub use pointer::{DuplicateMouseBinding, MouseButton, PointerSample, PointerSampleError};

#[path = "input/motion.rs"]
mod motion;
pub use motion::{RelativePointerMotion, RelativePointerMotionError};
#[path = "input/capture.rs"]
mod capture;
pub use capture::{PointerCapture, PointerCaptureStatus};

#[cfg(test)]
#[path = "input/cancellation_tests.rs"]
mod cancellation_tests;
#[cfg(test)]
#[path = "input/pointer_tests.rs"]
mod pointer_tests;

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
    hash::Hash,
};

use bevy_ecs::{
    prelude::{Res, Resource},
    system::SystemParam,
};
use sim_engine::Vec2;

use crate::identity::{ApplicationId, TransitionIntentToken, WorldGeneration};

/// Default maximum number of physical input events accepted in one frame and
/// logical edges generated per frame or retained for the next FixedUpdate delivery.
pub const DEFAULT_INPUT_EVENT_LIMIT: usize = 1_024;

/// Marker contract for values used as logical input actions.
///
/// Actions are copied into immutable frame and fixed-update snapshots. Equality
/// and hashing are used only for lookup; iteration order is never part of the
/// input contract.
pub trait Action: Copy + Eq + Hash + Send + Sync + 'static {}

impl<T> Action for T where T: Copy + Eq + Hash + Send + Sync + 'static {}

/// Four logical actions sampled as a digital two-dimensional axis.
///
/// The descriptor contains no input state and is independent from physical
/// key bindings. Reusing one action in several slots is allowed: each slot is
/// sampled independently and opposite slots cancel through subtraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DigitalAxis2d<A: Action> {
    negative_x: A,
    positive_x: A,
    negative_y: A,
    positive_y: A,
}

impl<A: Action> DigitalAxis2d<A> {
    /// Creates a descriptor in negative-x, positive-x, negative-y,
    /// positive-y order.
    ///
    /// For conventional world movement this normally receives left, right,
    /// down, and up actions. The mapping is infallible and may contain the same
    /// action more than once.
    pub const fn new(negative_x: A, positive_x: A, negative_y: A, positive_y: A) -> Self {
        Self {
            negative_x,
            positive_x,
            negative_y,
            positive_y,
        }
    }

    /// Returns the action contributing `-1.0` to the x component while held.
    pub const fn negative_x(self) -> A {
        self.negative_x
    }

    /// Returns the action contributing `1.0` to the x component while held.
    pub const fn positive_x(self) -> A {
        self.positive_x
    }

    /// Returns the action contributing `-1.0` to the y component while held.
    pub const fn negative_y(self) -> A {
        self.negative_y
    }

    /// Returns the action contributing `1.0` to the y component while held.
    pub const fn positive_y(self) -> A {
        self.positive_y
    }
}

/// A portable physical keyboard key supported by the runtime.
///
/// Desktop adapters translate platform key codes into this enum before input
/// reaches the headless runtime core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PhysicalKeyCode {
    /// The physical W key.
    KeyW,
    /// The physical A key.
    KeyA,
    /// The physical S key.
    KeyS,
    /// The physical D key.
    KeyD,
    /// The main Enter key.
    Enter,
    /// The physical Space key.
    Space,
    /// The physical left-arrow key.
    ArrowLeft,
    /// The physical right-arrow key.
    ArrowRight,
    /// The physical down-arrow key.
    ArrowDown,
    /// The physical up-arrow key.
    ArrowUp,
    /// The physical Escape key.
    Escape,
    /// The physical P key.
    KeyP,
    /// The physical R key.
    KeyR,
    /// The physical N key.
    KeyN,
    /// The physical 1 key on the number row, not the numeric keypad.
    Digit1,
    /// The physical 2 key on the number row, not the numeric keypad.
    Digit2,
    /// The physical 3 key on the number row, not the numeric keypad.
    Digit3,
    /// The physical 4 key on the number row, not the numeric keypad.
    Digit4,
    /// The physical 5 key on the number row, not the numeric keypad.
    Digit5,
    /// The physical F3 function key.
    F3,
    /// The physical F4 function key.
    F4,
    /// The physical F5 function key.
    F5,
    /// The physical F6 function key.
    F6,
    /// The physical F8 function key.
    F8,
    /// The physical F9 function key.
    F9,
    /// The physical L key.
    KeyL,
    /// The physical F key.
    KeyF,
    /// The physical M key.
    KeyM,
    /// The physical T key.
    KeyT,
    /// The physical V key.
    KeyV,
    /// The physical 6 key on the number row.
    Digit6,
    /// The physical 7 key on the number row.
    Digit7,
    /// The physical 8 key on the number row.
    Digit8,
    /// The physical 9 key on the number row.
    Digit9,
    /// The physical F7 function key.
    F7,
    /// The physical E key.
    KeyE,
    /// The physical left Shift key.
    ShiftLeft,
}

pub(crate) const ALL_PHYSICAL_KEYS: [PhysicalKeyCode; 37] = [
    PhysicalKeyCode::KeyW,
    PhysicalKeyCode::KeyA,
    PhysicalKeyCode::KeyS,
    PhysicalKeyCode::KeyD,
    PhysicalKeyCode::Enter,
    PhysicalKeyCode::Space,
    PhysicalKeyCode::ArrowLeft,
    PhysicalKeyCode::ArrowRight,
    PhysicalKeyCode::ArrowDown,
    PhysicalKeyCode::ArrowUp,
    PhysicalKeyCode::Escape,
    PhysicalKeyCode::KeyP,
    PhysicalKeyCode::KeyR,
    PhysicalKeyCode::KeyN,
    PhysicalKeyCode::Digit1,
    PhysicalKeyCode::Digit2,
    PhysicalKeyCode::Digit3,
    PhysicalKeyCode::Digit4,
    PhysicalKeyCode::Digit5,
    PhysicalKeyCode::F3,
    PhysicalKeyCode::F4,
    PhysicalKeyCode::F5,
    PhysicalKeyCode::F6,
    PhysicalKeyCode::F8,
    PhysicalKeyCode::F9,
    PhysicalKeyCode::KeyL,
    PhysicalKeyCode::KeyF,
    PhysicalKeyCode::KeyM,
    PhysicalKeyCode::KeyT,
    PhysicalKeyCode::KeyV,
    PhysicalKeyCode::Digit6,
    PhysicalKeyCode::Digit7,
    PhysicalKeyCode::Digit8,
    PhysicalKeyCode::Digit9,
    PhysicalKeyCode::F7,
    PhysicalKeyCode::KeyE,
    PhysicalKeyCode::ShiftLeft,
];

pub(crate) const SUPPORTED_PHYSICAL_KEY_COUNT: usize = ALL_PHYSICAL_KEYS.len();

/// The state carried by a physical button event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ButtonState {
    /// The key or mouse button transitioned from up to down.
    Pressed,
    /// The key or mouse button transitioned from down to up.
    Released,
}

/// One platform-independent physical input event.
///
/// Repeated presses of an already-held key or mouse button, and repeated
/// releases of one already up, do not create logical edges. Motion updates
/// continuous state without creating edges. Event order determines the pointer
/// sample captured by each mouse edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputEvent {
    /// A physical keyboard state change.
    Key {
        /// The physical location of the key.
        key: PhysicalKeyCode,
        /// The reported button state.
        state: ButtonState,
    },
    /// Updates the continuous pointer without generating a logical action edge.
    PointerMoved {
        /// Finite logical coordinates and the viewport at this point in event order.
        sample: PointerSample,
    },
    /// Adds raw displacement to this frame's relative motion without changing
    /// the absolute pointer or generating action edges. Not retained for fixed
    /// ticks. FocusLost clears motion and ignores later motion in the same batch.
    RelativePointerMotion {
        /// Validated raw device displacement, independent of the viewport/DPI.
        motion: RelativePointerMotion,
    },
    /// A physical mouse-button state change, using the latest pointer sample.
    MouseButton {
        /// The physical mouse button.
        button: MouseButton,
        /// The reported button state.
        state: ButtonState,
    },
    /// Clears the pointer and releases held mouse buttons in Left/Right/Middle
    /// order. Synthesized releases have no pointer sample. Keyboard state is
    /// unchanged. The generated releases have a PointerLeft cancellation reason.
    PointerLeft,
    /// Clears the pointer and cancels held mapped keyboard controls in the
    /// supported key catalog order, then mouse controls in Left/Right/Middle
    /// order. Generated releases have a FocusLost reason and no pointer sample.
    ///
    /// One event can create many logical edges; all normal frame and retained
    /// limits apply atomically. Later events in the same batch are still read.
    /// This is not a persistent focus-state flag or an input-routing layer.
    FocusLost,
}

impl InputEvent {
    /// Creates a physical keyboard event for the headless or desktop adapter.
    pub const fn key(key: PhysicalKeyCode, state: ButtonState) -> Self {
        Self::Key { key, state }
    }

    /// Creates an ordered pointer-position update, independent of bindings.
    pub const fn pointer_moved(sample: PointerSample) -> Self {
        Self::PointerMoved { sample }
    }

    /// Supplies validated raw displacement; headless injection needs no OS capture.
    pub const fn relative_pointer_motion(motion: RelativePointerMotion) -> Self {
        Self::RelativePointerMotion { motion }
    }

    /// Creates a mouse-button event. Its logical edge captures the latest
    /// preceding pointer sample, or None when no position is known.
    pub const fn mouse_button(button: MouseButton, state: ButtonState) -> Self {
        Self::MouseButton { button, state }
    }
}

/// Rejection returned when one physical key is bound more than once.
///
/// Several different keys may intentionally map to the same action. A single
/// physical key has exactly one action in the first vertical slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateKeyBinding {
    key: PhysicalKeyCode,
}

impl DuplicateKeyBinding {
    /// Returns the physical key whose existing binding was preserved.
    pub const fn key(self) -> PhysicalKeyCode {
        self.key
    }
}

impl fmt::Display for DuplicateKeyBinding {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "physical key {:?} is already bound", self.key)
    }
}

impl Error for DuplicateKeyBinding {}

#[derive(Debug, Clone)]
pub(crate) struct ActionBindings<A: Action> {
    by_key: [Option<A>; SUPPORTED_PHYSICAL_KEY_COUNT],
    by_mouse: [Option<A>; ALL_MOUSE_BUTTONS.len()],
}

impl<A: Action> Default for ActionBindings<A> {
    fn default() -> Self {
        Self {
            by_key: [None; SUPPORTED_PHYSICAL_KEY_COUNT],
            by_mouse: [None; ALL_MOUSE_BUTTONS.len()],
        }
    }
}

impl<A: Action> ActionBindings<A> {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn bind_mouse_button(
        &mut self,
        button: MouseButton,
        action: A,
    ) -> Result<&mut Self, DuplicateMouseBinding> {
        let slot = &mut self.by_mouse[mouse_button_index(button)];
        if slot.is_some() {
            return Err(DuplicateMouseBinding { button });
        }
        *slot = Some(action);
        Ok(self)
    }

    fn mouse_action_for(&self, button: MouseButton) -> Option<A> {
        self.by_mouse[mouse_button_index(button)]
    }

    fn control_action_for(&self, control: InputControl) -> Option<A> {
        match control {
            InputControl::Key(key) => self.action_for(key),
            InputControl::MouseButton(button) => self.mouse_action_for(button),
        }
    }

    /// Binds one physical key without replacing an existing binding.
    ///
    /// On failure the binding table is unchanged.
    pub(crate) fn bind(
        &mut self,
        key: PhysicalKeyCode,
        action: A,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bind_all([(key, action)])
    }

    /// Atomically binds W/A/S/D to the matching slots of one logical axis.
    pub(crate) fn bind_wasd(
        &mut self,
        axis: DigitalAxis2d<A>,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bind_all([
            (PhysicalKeyCode::KeyW, axis.positive_y()),
            (PhysicalKeyCode::KeyA, axis.negative_x()),
            (PhysicalKeyCode::KeyS, axis.negative_y()),
            (PhysicalKeyCode::KeyD, axis.positive_x()),
        ])
    }

    /// Atomically binds the arrow keys to the matching slots of one axis.
    pub(crate) fn bind_arrows(
        &mut self,
        axis: DigitalAxis2d<A>,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bind_all([
            (PhysicalKeyCode::ArrowLeft, axis.negative_x()),
            (PhysicalKeyCode::ArrowRight, axis.positive_x()),
            (PhysicalKeyCode::ArrowDown, axis.negative_y()),
            (PhysicalKeyCode::ArrowUp, axis.positive_y()),
        ])
    }

    /// Atomically binds both W/A/S/D and the arrow keys to one axis.
    pub(crate) fn bind_wasd_and_arrows(
        &mut self,
        axis: DigitalAxis2d<A>,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bind_all([
            (PhysicalKeyCode::KeyW, axis.positive_y()),
            (PhysicalKeyCode::KeyA, axis.negative_x()),
            (PhysicalKeyCode::KeyS, axis.negative_y()),
            (PhysicalKeyCode::KeyD, axis.positive_x()),
            (PhysicalKeyCode::ArrowLeft, axis.negative_x()),
            (PhysicalKeyCode::ArrowRight, axis.positive_x()),
            (PhysicalKeyCode::ArrowDown, axis.negative_y()),
            (PhysicalKeyCode::ArrowUp, axis.positive_y()),
        ])
    }

    fn bind_all<const N: usize>(
        &mut self,
        bindings: [(PhysicalKeyCode, A); N],
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        for (key, _) in &bindings {
            if self.action_for(*key).is_some() {
                return Err(DuplicateKeyBinding { key: *key });
            }
        }
        for (key, action) in bindings {
            self.by_key[physical_key_index(key)] = Some(action);
        }
        Ok(self)
    }

    fn action_for(&self, key: PhysicalKeyCode) -> Option<A> {
        self.by_key[physical_key_index(key)]
    }
}

/// Failure to configure or collect application input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputCollectionError {
    /// Summing relative pointer events would exceed their numeric frame limit.
    RelativeMotion(RelativePointerMotionError),
    /// The shared physical-event, generated-edge, and retained-edge limit is zero.
    ZeroEventLimit,
    /// The runtime could not reserve the bounded input storage required by the
    /// configured limit.
    StorageAllocationFailed {
        /// Configured physical-event, generated-edge, and retained-edge limit.
        limit: usize,
    },
    /// The supplied frame exceeds its physical event budget.
    EventLimitExceeded {
        /// Maximum accepted event count.
        limit: usize,
        /// Event count supplied for the rejected frame.
        received: usize,
    },
    /// This frame would generate too many logical edges, including releases
    /// synthesized by PointerLeft or FocusLost. Checked even when fixed updates
    /// are paused.
    FrameEdgeLimitExceeded {
        /// Maximum generated frame-input edge count.
        limit: usize,
        /// New logical edges produced by the rejected frame.
        incoming: usize,
    },
    /// Retaining this frame's logical edges for FixedUpdate would exceed the
    /// shared input event limit.
    RetainedFixedEdgeLimitExceeded {
        /// Maximum retained fixed-input edge count.
        limit: usize,
        /// Edges already waiting for their first FixedUpdate delivery.
        retained: usize,
        /// New logical edges produced by the rejected frame.
        incoming: usize,
    },
    /// The runtime cannot issue enough never-reused occurrence numbers.
    OccurrenceIdentityExhausted,
}

impl fmt::Display for InputCollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RelativeMotion(error) => {
                write!(formatter, "relative input collection failed: {error}")
            }
            Self::ZeroEventLimit => {
                formatter.write_str("the input event and retained-edge limit must be positive")
            }
            Self::StorageAllocationFailed { limit } => write!(
                formatter,
                "failed to reserve bounded input storage for the configured limit of {limit}"
            ),
            Self::EventLimitExceeded { limit, received } => write!(
                formatter,
                "input frame contains {received} physical events, exceeding the limit of {limit}"
            ),
            Self::FrameEdgeLimitExceeded { limit, incoming } => write!(
                formatter,
                "input frame generates {incoming} logical edges, exceeding the limit of {limit}"
            ),
            Self::RetainedFixedEdgeLimitExceeded {
                limit,
                retained,
                incoming,
            } => write!(
                formatter,
                "fixed input retains {retained} edges and cannot accept {incoming} more without exceeding the limit of {limit}"
            ),
            Self::OccurrenceIdentityExhausted => {
                formatter.write_str("input occurrence identity space is exhausted")
            }
        }
    }
}

impl Error for InputCollectionError {}

/// One logical action edge and its causal transition token.
///
/// An edge belongs to one physical occurrence. Different keys or mouse buttons
/// mapped to the same action remain separate entries in physical event order.
/// Pointer leave synthesizes releases in Left/Right/Middle order. Focus loss
/// releases keys in portable catalog order, then mouse buttons in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActionEdge<A: Action> {
    action: A,
    state: ButtonState,
    intent: TransitionIntentToken,
    pointer: Option<PointerSample>,
    control: InputControl,
    cancellation: Option<InputCancellationReason>,
}

impl<A: Action> ActionEdge<A> {
    /// Returns the physical control associated with this occurrence, even if
    /// several controls share its action or its pointer position is unknown.
    /// The original value is preserved across frame and delayed fixed delivery.
    pub const fn control(self) -> InputControl {
        self.control
    }

    /// Returns why this release was synthesized, or None for ordinary input.
    /// Presses always return None. A mouse release without a known pointer is
    /// not necessarily cancelled; use this method rather than guessing from
    /// pointer absence. Cancellation does not erase an earlier press occurrence.
    pub const fn cancellation_reason(self) -> Option<InputCancellationReason> {
        self.cancellation
    }

    /// Returns whether this is a cancellation release rather than an ordinary
    /// physical release. Commit-on-release interactions should normally ignore
    /// cancelled releases while still clearing their own held/drag state.
    pub const fn is_cancelled(self) -> bool {
        self.cancellation.is_some()
    }

    /// Returns this mouse occurrence's event-time pointer sample. Keyboard
    /// edges, cancellation releases, and mouse edges without a known
    /// position return None. Later movement does not change this value.
    pub const fn pointer(self) -> Option<PointerSample> {
        self.pointer
    }
    /// Returns the logical action associated with this occurrence.
    pub const fn action(self) -> A {
        self.action
    }

    /// Returns whether this occurrence pressed or released its physical control.
    pub const fn state(self) -> ButtonState {
        self.state
    }

    /// Returns the runtime-issued token shared by frame and fixed delivery.
    ///
    /// The token is valid only while the underlying occurrence remains live in
    /// one of those delivery queues.
    pub const fn intent(self) -> TransitionIntentToken {
        self.intent
    }
}

/// Read-only logical input exposed to FrameUpdate systems.
///
/// Its edges contain only physical occurrences collected for the current
/// application frame. Held state reflects the final state after all accepted
/// events in that frame were processed.
#[derive(Debug, Clone, Resource)]
pub(crate) struct FrameInputState<A: Action> {
    held: HashSet<A>,
    edges: Vec<ActionEdge<A>>,
    pointer: Option<PointerSample>,
    relative_motion: RelativePointerMotion,
    focus_lost: bool,
}

impl<A: Action> FrameInputState<A> {
    /// Returns whether at least one bound key or mouse button for `action` is held.
    pub fn held(&self, action: A) -> bool {
        self.held.contains(&action)
    }

    fn digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        sample_digital_axis(axis, |action| self.held(action))
    }

    /// Iterates over every press occurrence for `action` in event order.
    pub fn pressed(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.action == action && edge.state == ButtonState::Pressed)
    }

    /// Iterates over every release occurrence for `action` in event order.
    pub fn released(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.action == action && edge.state == ButtonState::Released)
    }

    pub(crate) fn empty() -> Self {
        Self {
            held: HashSet::new(),
            edges: Vec::new(),
            pointer: None,
            relative_motion: RelativePointerMotion::ZERO,
            focus_lost: false,
        }
    }

    pub(crate) fn clear_reusing_storage(&mut self) {
        self.held.clear();
        self.edges.clear();
        self.pointer = None;
        self.relative_motion = RelativePointerMotion::ZERO;
        self.focus_lost = false;
    }

    fn replace_from(&mut self, held_action_counts: &HashMap<A, usize>, edges: &[ActionEdge<A>]) {
        replace_snapshot(&mut self.held, &mut self.edges, held_action_counts, edges);
    }
}

/// Read-only logical input for the current FrameUpdate delivery.
///
/// This custom System parameter deliberately exposes no `ResMut` path to the
/// runtime-owned snapshot.
#[derive(SystemParam)]
pub struct FrameInput<'w, A: Action> {
    state: Res<'w, FrameInputState<A>>,
}

impl<A: Action> FrameInput<'_, A> {
    /// Returns this frame's raw displacement. Apply sensitivity once, without
    /// delta-time scaling. No fixed-tick delivery or transition replay occurs.
    pub fn relative_motion(&self) -> RelativePointerMotion {
        self.state.relative_motion
    }

    /// Reports a focus-loss boundary even when no mapped control was held.
    /// All relative motion in that accepted batch is discarded.
    pub fn focus_lost(&self) -> bool {
        self.state.focus_lost
    }

    /// Returns the latest accepted pointer sample, or None before motion or
    /// after leaving. Inactive-stage snapshots are empty. Use an action edge's
    /// [`ActionEdge::pointer`] for the position of a particular click.
    pub fn pointer(&self) -> Option<PointerSample> {
        self.state.pointer
    }

    /// Returns whether at least one bound key or mouse button for `action` is held.
    pub fn held(&self, action: A) -> bool {
        self.state.held(action)
    }

    /// Returns whether this frame contains at least one press occurrence.
    ///
    /// This does not consume the occurrence and is independent from aggregate
    /// [`Self::held`] state: another physical key for the same action may
    /// already be held, and one frame may contain both press and release
    /// occurrences. Use [`Self::pressed`] when occurrence count, order, or
    /// intent tokens are needed.
    pub fn has_press_occurrence(&self, action: A) -> bool {
        self.pressed(action).next().is_some()
    }

    /// Returns whether this frame contains at least one release occurrence.
    ///
    /// This does not consume the occurrence. It can return `true` while
    /// [`Self::held`] also returns `true` when another physical key remains
    /// bound to and held for the same action. Use [`Self::released`] when
    /// occurrence count, order, or intent tokens are needed.
    pub fn has_release_occurrence(&self, action: A) -> bool {
        self.released(action).next().is_some()
    }

    /// Samples four held actions as a raw digital axis.
    ///
    /// Each component is exactly `-1.0`, `0.0`, or `1.0`; opposite directions
    /// cancel. This performs four held-state lookups without allocation and
    /// does not read or consume action edges. Use
    /// [`Self::normalized_digital_axis`] when diagonal movement should retain
    /// cardinal speed.
    pub fn digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        self.state.digital_axis(axis)
    }

    /// Samples four held actions as a constant-speed digital direction.
    ///
    /// This is exactly equivalent to `self.digital_axis(axis).normalized()`:
    /// no input or mutually cancelled directions produce [`Vec2::ZERO`],
    /// cardinal directions stay unchanged, and non-zero diagonals have unit
    /// length. It performs no allocation and neither reads nor consumes action
    /// edges. Use [`Self::digital_axis`] when raw diagonal magnitude or grid
    /// semantics are wanted.
    pub fn normalized_digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        self.digital_axis(axis).normalized()
    }

    /// Iterates over every press occurrence for `action` in event order.
    pub fn pressed(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.state.pressed(action)
    }

    /// Iterates over all action edges in physical event order.
    ///
    /// Use this for gestures whose presses and releases can both arrive in
    /// one display frame. Each mouse edge keeps its own event-time pointer;
    /// the latest pointer is not necessarily the release endpoint.
    pub fn edges(&self) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.state.edges.iter().copied()
    }

    /// Iterates over every release occurrence for `action` in event order.
    pub fn released(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.state.released(action)
    }
}

/// Read-only logical input exposed to FixedUpdate systems.
///
/// Fixed edges persist across application frames until the first fixed tick
/// that can receive them. All systems in that tick see the same snapshot;
/// later catch-up ticks do not see those edges again.
#[derive(Debug, Clone, Resource)]
pub(crate) struct FixedInputState<A: Action> {
    held: HashSet<A>,
    edges: Vec<ActionEdge<A>>,
    pointer: Option<PointerSample>,
    focus_lost: bool,
}

impl<A: Action> FixedInputState<A> {
    /// Returns whether at least one bound key or mouse button for `action` is held.
    pub fn held(&self, action: A) -> bool {
        self.held.contains(&action)
    }

    fn digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        sample_digital_axis(axis, |action| self.held(action))
    }

    /// Iterates over every queued press occurrence for `action` in event order.
    pub fn pressed(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.action == action && edge.state == ButtonState::Pressed)
    }

    /// Iterates over every queued release occurrence for `action` in event order.
    pub fn released(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.edges
            .iter()
            .copied()
            .filter(move |edge| edge.action == action && edge.state == ButtonState::Released)
    }

    pub(crate) fn empty() -> Self {
        Self {
            held: HashSet::new(),
            edges: Vec::new(),
            pointer: None,
            focus_lost: false,
        }
    }

    pub(crate) fn clear_reusing_storage(&mut self) {
        self.held.clear();
        self.edges.clear();
        self.pointer = None;
        self.focus_lost = false;
    }

    fn replace_from(&mut self, held_action_counts: &HashMap<A, usize>, edges: &[ActionEdge<A>]) {
        replace_snapshot(&mut self.held, &mut self.edges, held_action_counts, edges);
    }
}

/// Read-only logical input for the current FixedUpdate delivery.
///
/// Fixed edges survive frames with no fixed tick and are delivered to every
/// system in the first later tick, without exposing mutable snapshot access.
/// Their total retained count is bounded by the application's input event
/// limit; a frame that would exceed it is rejected before input state changes.
#[derive(SystemParam)]
pub struct FixedInput<'w, A: Action> {
    state: Res<'w, FixedInputState<A>>,
}

impl<A: Action> FixedInput<'_, A> {
    /// Reports focus loss in this application frame, even with no held control.
    /// True for every catch-up tick in that frame, so cached movement can stop
    /// before FrameUpdate. Unlike edges, this boundary is not retained into a
    /// later frame with a fixed tick. False outside FixedUpdate.
    pub fn focus_lost(&self) -> bool {
        self.state.focus_lost
    }

    /// Returns the latest accepted pointer sample for this tick. Retained
    /// click edges keep their own earlier samples; later catch-up ticks retain
    /// the continuous pointer but do not replay those edges. During FrameUpdate
    /// this snapshot returns None. Input resources are not present in Startup.
    pub fn pointer(&self) -> Option<PointerSample> {
        self.state.pointer
    }

    /// Returns whether at least one bound key or mouse button for `action` is held.
    pub fn held(&self, action: A) -> bool {
        self.state.held(action)
    }

    /// Returns whether this fixed tick contains at least one press occurrence.
    ///
    /// Press occurrences survive frames with no fixed tick, appear in the
    /// first later tick, and do not appear in its later catch-up ticks. This
    /// method does not consume them and is independent from aggregate
    /// [`Self::held`] state. Use [`Self::pressed`] when occurrence count,
    /// order, or intent tokens are needed.
    pub fn has_press_occurrence(&self, action: A) -> bool {
        self.pressed(action).next().is_some()
    }

    /// Returns whether this fixed tick contains at least one release occurrence.
    ///
    /// Release occurrences follow the same retained first-tick delivery as
    /// [`Self::has_press_occurrence`] and are not consumed by this call. It can
    /// return `true` while [`Self::held`] also returns `true` when another
    /// physical key remains bound to and held for the action. Use
    /// [`Self::released`] when occurrence count, order, or intent tokens are
    /// needed.
    pub fn has_release_occurrence(&self, action: A) -> bool {
        self.released(action).next().is_some()
    }

    /// Samples four held actions as a raw digital axis.
    ///
    /// Each component is exactly `-1.0`, `0.0`, or `1.0`; opposite directions
    /// cancel. This performs four held-state lookups without allocation and
    /// does not read or consume action edges. Use
    /// [`Self::normalized_digital_axis`] when diagonal movement should retain
    /// cardinal speed.
    pub fn digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        self.state.digital_axis(axis)
    }

    /// Samples four held actions as a constant-speed digital direction.
    ///
    /// This is exactly equivalent to `self.digital_axis(axis).normalized()`:
    /// no input or mutually cancelled directions produce [`Vec2::ZERO`],
    /// cardinal directions stay unchanged, and non-zero diagonals have unit
    /// length. It performs no allocation and neither reads nor consumes action
    /// edges. Use [`Self::digital_axis`] when raw diagonal magnitude or grid
    /// semantics are wanted.
    pub fn normalized_digital_axis(&self, axis: DigitalAxis2d<A>) -> Vec2 {
        self.digital_axis(axis).normalized()
    }

    /// Iterates over every queued press occurrence for `action` in event order.
    pub fn pressed(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.state.pressed(action)
    }

    /// Iterates over every queued release occurrence for `action` in event order.
    pub fn released(&self, action: A) -> impl Iterator<Item = ActionEdge<A>> + '_ {
        self.state.released(action)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InputCollectionReport {
    pub(crate) physical_events: usize,
    pub(crate) logical_edges: usize,
    pub(crate) suppressed_repeats: usize,
    pub(crate) unmapped_events: usize,
}

#[derive(Clone, Copy)]
struct EdgeDelivery {
    paused: bool,
    application: ApplicationId,
    origin: WorldGeneration,
    frame: u64,
}

#[derive(Debug)]
pub(crate) struct InputState<A: Action> {
    bindings: ActionBindings<A>,
    event_limit: usize,
    held_keys: [bool; SUPPORTED_PHYSICAL_KEY_COUNT],
    held_mouse: [bool; ALL_MOUSE_BUTTONS.len()],
    pointer: Option<PointerSample>,
    relative_motion: RelativePointerMotion,
    focus_lost: bool,
    held_action_counts: HashMap<A, usize>,
    frame_edges: Vec<ActionEdge<A>>,
    fixed_edges: Vec<ActionEdge<A>>,
    next_occurrence: u64,
}

impl<A: Action> InputState<A> {
    pub(crate) fn new(
        bindings: ActionBindings<A>,
        event_limit: usize,
    ) -> Result<Self, InputCollectionError> {
        if event_limit == 0 {
            return Err(InputCollectionError::ZeroEventLimit);
        }

        let mut held_action_counts = HashMap::new();
        let mut frame_edges = Vec::new();
        let mut fixed_edges = Vec::new();
        let held_capacity = SUPPORTED_PHYSICAL_KEY_COUNT + ALL_MOUSE_BUTTONS.len();
        held_action_counts
            .try_reserve(held_capacity)
            .map_err(|_| InputCollectionError::StorageAllocationFailed { limit: event_limit })?;
        frame_edges
            .try_reserve(event_limit)
            .map_err(|_| InputCollectionError::StorageAllocationFailed { limit: event_limit })?;
        fixed_edges
            .try_reserve(event_limit)
            .map_err(|_| InputCollectionError::StorageAllocationFailed { limit: event_limit })?;

        Ok(Self {
            bindings,
            event_limit,
            held_keys: [false; SUPPORTED_PHYSICAL_KEY_COUNT],
            held_mouse: [false; ALL_MOUSE_BUTTONS.len()],
            pointer: None,
            relative_motion: RelativePointerMotion::ZERO,
            focus_lost: false,
            held_action_counts,
            frame_edges,
            fixed_edges,
            next_occurrence: 0,
        })
    }

    pub(crate) fn collect_frame(
        &mut self,
        events: &[InputEvent],
        paused: bool,
        application: ApplicationId,
        origin: WorldGeneration,
        frame: u64,
    ) -> Result<InputCollectionReport, InputCollectionError> {
        if events.len() > self.event_limit {
            return Err(InputCollectionError::EventLimitExceeded {
                limit: self.event_limit,
                received: events.len(),
            });
        }

        let (relative_motion, focus_lost) =
            motion::frame_motion(events).map_err(InputCollectionError::RelativeMotion)?;
        let report = self.preflight_frame(events, paused)?;
        let logical_edge_count = u64::try_from(report.logical_edges)
            .map_err(|_| InputCollectionError::OccurrenceIdentityExhausted)?;
        if self
            .next_occurrence
            .checked_add(logical_edge_count)
            .is_none()
        {
            return Err(InputCollectionError::OccurrenceIdentityExhausted);
        }

        self.frame_edges.clear();
        self.relative_motion = relative_motion;
        self.focus_lost = focus_lost;
        let delivery = EdgeDelivery {
            paused,
            application,
            origin,
            frame,
        };

        for event in events {
            match *event {
                InputEvent::Key { key, state } => {
                    self.record_edge(InputControl::Key(key), state, None, delivery);
                }
                InputEvent::MouseButton { button, state } => {
                    self.record_edge(InputControl::MouseButton(button), state, None, delivery);
                }
                InputEvent::PointerMoved { sample } => self.pointer = Some(sample),
                InputEvent::RelativePointerMotion { .. } => {}
                InputEvent::PointerLeft | InputEvent::FocusLost => {
                    let reason = cancellation_reason(*event);
                    self.pointer = None;
                    for control in cancelled_controls(reason) {
                        self.record_edge(control, ButtonState::Released, Some(reason), delivery);
                    }
                }
            }
        }

        Ok(report)
    }

    fn preflight_frame(
        &self,
        events: &[InputEvent],
        paused: bool,
    ) -> Result<InputCollectionReport, InputCollectionError> {
        let mut held_keys = self.held_keys;
        let mut held_mouse = self.held_mouse;
        let mut report = InputCollectionReport {
            physical_events: events.len(),
            logical_edges: 0,
            suppressed_repeats: 0,
            unmapped_events: 0,
        };

        for event in events {
            match *event {
                InputEvent::Key { key, state } => {
                    preflight_button(
                        self.bindings.action_for(key),
                        &mut held_keys[physical_key_index(key)],
                        state,
                        &mut report,
                    );
                }
                InputEvent::MouseButton { button, state } => {
                    preflight_button(
                        self.bindings.mouse_action_for(button),
                        &mut held_mouse[mouse_button_index(button)],
                        state,
                        &mut report,
                    );
                }
                InputEvent::PointerMoved { .. } | InputEvent::RelativePointerMotion { .. } => {}
                InputEvent::PointerLeft | InputEvent::FocusLost => {
                    for control in cancelled_controls(cancellation_reason(*event)) {
                        // Synthetic no-op releases are not physical repeats
                        // or unmapped events; only actual held buttons count.
                        let held = match control {
                            InputControl::Key(key) => &mut held_keys[physical_key_index(key)],
                            InputControl::MouseButton(button) => {
                                &mut held_mouse[mouse_button_index(button)]
                            }
                        };
                        if self.bindings.control_action_for(control).is_some()
                            && change_button(held, ButtonState::Released)
                        {
                            report.logical_edges += 1;
                        }
                    }
                }
            }
        }

        if report.logical_edges > self.event_limit {
            return Err(InputCollectionError::FrameEdgeLimitExceeded {
                limit: self.event_limit,
                incoming: report.logical_edges,
            });
        }

        if !paused && report.logical_edges > self.event_limit.saturating_sub(self.fixed_edges.len())
        {
            return Err(InputCollectionError::RetainedFixedEdgeLimitExceeded {
                limit: self.event_limit,
                retained: self.fixed_edges.len(),
                incoming: report.logical_edges,
            });
        }

        Ok(report)
    }

    #[cfg(test)]
    pub(crate) fn frame_input(&self) -> FrameInputState<A> {
        let mut snapshot = FrameInputState::empty();
        self.copy_frame_snapshot_into(&mut snapshot);
        snapshot
    }

    #[cfg(test)]
    pub(crate) fn fixed_input(&self) -> FixedInputState<A> {
        let mut snapshot = FixedInputState::empty();
        self.copy_fixed_snapshot_into(&mut snapshot);
        snapshot
    }

    pub(crate) fn copy_frame_snapshot_into(&self, target: &mut FrameInputState<A>) {
        target.replace_from(&self.held_action_counts, &self.frame_edges);
        target.pointer = self.pointer;
        target.relative_motion = self.relative_motion;
        target.focus_lost = self.focus_lost;
    }

    pub(crate) fn copy_fixed_snapshot_into(&self, target: &mut FixedInputState<A>) {
        target.replace_from(&self.held_action_counts, &self.fixed_edges);
        target.pointer = self.pointer;
        target.focus_lost = self.focus_lost;
    }

    pub(crate) fn consume_fixed_delivery(&mut self) {
        self.fixed_edges.clear();
    }

    pub(crate) fn discard_fixed_edges(&mut self) {
        self.fixed_edges.clear();
    }

    pub(crate) fn consume_token_occurrence(&mut self, token: TransitionIntentToken) -> bool {
        let was_live = self.token_is_live(token);
        self.frame_edges.retain(|edge| edge.intent != token);
        self.fixed_edges.retain(|edge| edge.intent != token);
        was_live
    }

    pub(crate) fn token_is_live(&self, token: TransitionIntentToken) -> bool {
        self.frame_edges
            .iter()
            .chain(&self.fixed_edges)
            .any(|edge| edge.intent == token)
    }

    pub(crate) fn token_is_live_in_frame(&self, token: TransitionIntentToken) -> bool {
        self.frame_edges.iter().any(|edge| edge.intent == token)
    }

    pub(crate) fn token_is_live_in_fixed(&self, token: TransitionIntentToken) -> bool {
        self.fixed_edges.iter().any(|edge| edge.intent == token)
    }

    pub(crate) fn clear_world_edges(&mut self) {
        self.frame_edges.clear();
        self.fixed_edges.clear();
        self.relative_motion = RelativePointerMotion::ZERO;
        self.focus_lost = false;
    }

    pub(crate) fn end_frame(&mut self) {
        self.frame_edges.clear();
        self.relative_motion = RelativePointerMotion::ZERO;
        self.focus_lost = false;
    }

    fn record_edge(
        &mut self,
        control: InputControl,
        state: ButtonState,
        cancellation: Option<InputCancellationReason>,
        delivery: EdgeDelivery,
    ) {
        let Some(action) = self.bindings.control_action_for(control) else {
            return;
        };
        let held = match control {
            InputControl::Key(key) => &mut self.held_keys[physical_key_index(key)],
            InputControl::MouseButton(button) => &mut self.held_mouse[mouse_button_index(button)],
        };
        if !change_button(held, state) {
            return;
        }
        let pointer = match (control, cancellation) {
            (InputControl::MouseButton(_), None) => self.pointer,
            _ => None,
        };
        self.update_held_action(action, state);
        let occurrence = self.next_occurrence;
        // Whole-frame preflight proved both queue bounds and never-reused
        // occurrence space, including every pointer-leave or focus-loss release.
        self.next_occurrence += 1;
        let edge = ActionEdge {
            action,
            state,
            pointer,
            control,
            cancellation,
            intent: TransitionIntentToken::input(
                delivery.application,
                delivery.origin,
                delivery.frame,
                occurrence,
            ),
        };
        self.frame_edges.push(edge);
        if !delivery.paused {
            self.fixed_edges.push(edge);
        }
    }

    fn update_held_action(&mut self, action: A, state: ButtonState) {
        match state {
            ButtonState::Pressed => {
                let count = self.held_action_counts.entry(action).or_insert(0);
                *count += 1;
            }
            ButtonState::Released => {
                if let Some(count) = self.held_action_counts.get_mut(&action) {
                    *count -= 1;
                    if *count == 0 {
                        self.held_action_counts.remove(&action);
                    }
                }
            }
        }
    }
}

fn cancellation_reason(event: InputEvent) -> InputCancellationReason {
    // Only the two explicit cancellation branches call this helper. Keep their
    // control enumeration identical between preflight and committed collection.
    if matches!(event, InputEvent::FocusLost) {
        InputCancellationReason::FocusLost
    } else {
        InputCancellationReason::PointerLeft
    }
}

fn change_button(held: &mut bool, state: ButtonState) -> bool {
    let next = state == ButtonState::Pressed;
    let changed = *held != next;
    *held = next;
    changed
}

fn preflight_button<A: Action>(
    action: Option<A>,
    held: &mut bool,
    state: ButtonState,
    report: &mut InputCollectionReport,
) {
    if action.is_none() {
        report.unmapped_events += 1;
    } else if change_button(held, state) {
        report.logical_edges += 1;
    } else {
        report.suppressed_repeats += 1;
    }
}

fn replace_snapshot<A: Action>(
    target_held: &mut HashSet<A>,
    target_edges: &mut Vec<ActionEdge<A>>,
    held_action_counts: &HashMap<A, usize>,
    source_edges: &[ActionEdge<A>],
) {
    target_held.clear();
    target_held.extend(held_action_counts.keys().copied());
    target_edges.clear();
    target_edges.extend_from_slice(source_edges);
}

fn sample_digital_axis<A: Action>(axis: DigitalAxis2d<A>, mut held: impl FnMut(A) -> bool) -> Vec2 {
    let x = f32::from(held(axis.positive_x)) - f32::from(held(axis.negative_x));
    let y = f32::from(held(axis.positive_y)) - f32::from(held(axis.negative_y));
    Vec2::new(x, y)
}

pub(crate) const fn physical_key_index(key: PhysicalKeyCode) -> usize {
    match key {
        PhysicalKeyCode::KeyW => 0,
        PhysicalKeyCode::KeyA => 1,
        PhysicalKeyCode::KeyS => 2,
        PhysicalKeyCode::KeyD => 3,
        PhysicalKeyCode::Enter => 4,
        PhysicalKeyCode::Space => 5,
        PhysicalKeyCode::ArrowLeft => 6,
        PhysicalKeyCode::ArrowRight => 7,
        PhysicalKeyCode::ArrowDown => 8,
        PhysicalKeyCode::ArrowUp => 9,
        PhysicalKeyCode::Escape => 10,
        PhysicalKeyCode::KeyP => 11,
        PhysicalKeyCode::KeyR => 12,
        PhysicalKeyCode::KeyN => 13,
        PhysicalKeyCode::Digit1 => 14,
        PhysicalKeyCode::Digit2 => 15,
        PhysicalKeyCode::Digit3 => 16,
        PhysicalKeyCode::Digit4 => 17,
        PhysicalKeyCode::Digit5 => 18,
        PhysicalKeyCode::F3 => 19,
        PhysicalKeyCode::F4 => 20,
        PhysicalKeyCode::F5 => 21,
        PhysicalKeyCode::F6 => 22,
        PhysicalKeyCode::F8 => 23,
        PhysicalKeyCode::F9 => 24,
        PhysicalKeyCode::KeyL => 25,
        PhysicalKeyCode::KeyF => 26,
        PhysicalKeyCode::KeyM => 27,
        PhysicalKeyCode::KeyT => 28,
        PhysicalKeyCode::KeyV => 29,
        PhysicalKeyCode::Digit6 => 30,
        PhysicalKeyCode::Digit7 => 31,
        PhysicalKeyCode::Digit8 => 32,
        PhysicalKeyCode::Digit9 => 33,
        PhysicalKeyCode::F7 => 34,
        PhysicalKeyCode::KeyE => 35,
        PhysicalKeyCode::ShiftLeft => 36,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {
        Move,
        Enter,
        Fire,
        Left,
        Right,
        Down,
        Up,
    }

    const TEST_AXIS: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
        TestAction::Left,
        TestAction::Right,
        TestAction::Down,
        TestAction::Up,
    );

    fn identity() -> (ApplicationId, WorldGeneration) {
        let application = ApplicationId::from_raw(7);
        (application, WorldGeneration::new(application, 3))
    }

    fn bindings() -> ActionBindings<TestAction> {
        let mut bindings = ActionBindings::new();
        assert!(
            bindings
                .bind(PhysicalKeyCode::KeyW, TestAction::Move)
                .is_ok()
        );
        assert!(
            bindings
                .bind(PhysicalKeyCode::KeyA, TestAction::Move)
                .is_ok()
        );
        assert!(
            bindings
                .bind(PhysicalKeyCode::Enter, TestAction::Enter)
                .is_ok()
        );
        assert!(
            bindings
                .bind(PhysicalKeyCode::Space, TestAction::Fire)
                .is_ok()
        );
        bindings
    }

    fn state(limit: usize) -> InputState<TestAction> {
        let result = InputState::new(bindings(), limit);
        let Ok(state) = result else {
            panic!("test input state should be valid");
        };
        state
    }

    #[test]
    fn duplicate_physical_key_is_rejected_without_replacement() {
        let mut bindings = bindings();
        let duplicate = bindings.bind(PhysicalKeyCode::KeyW, TestAction::Enter);

        assert_eq!(
            duplicate.map(|_| ()),
            Err(DuplicateKeyBinding {
                key: PhysicalKeyCode::KeyW,
            })
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::KeyW),
            Some(TestAction::Move)
        );
    }

    #[test]
    fn physical_key_catalog_is_unique_and_round_trips_every_index() {
        assert_eq!(SUPPORTED_PHYSICAL_KEY_COUNT, 37);
        let mut unique = HashSet::new();

        for (index, key) in ALL_PHYSICAL_KEYS.into_iter().enumerate() {
            assert!(unique.insert(key), "duplicate catalog key {key:?}");
            assert_eq!(physical_key_index(key), index);
            assert_eq!(ALL_PHYSICAL_KEYS[physical_key_index(key)], key);
        }
    }

    #[test]
    fn wasd_binding_maps_axis_slots_and_remains_compatible_with_manual_keys() {
        let mut bindings = ActionBindings::new();
        assert!(
            bindings
                .bind(PhysicalKeyCode::Enter, TestAction::Enter)
                .is_ok()
        );
        assert!(bindings.bind_wasd(TEST_AXIS).is_ok());
        assert!(
            bindings
                .bind(PhysicalKeyCode::Space, TestAction::Fire)
                .is_ok()
        );

        assert_eq!(
            bindings.action_for(PhysicalKeyCode::KeyW),
            Some(TestAction::Up)
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::KeyA),
            Some(TestAction::Left)
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::KeyS),
            Some(TestAction::Down)
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::KeyD),
            Some(TestAction::Right)
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::Enter),
            Some(TestAction::Enter)
        );
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::Space),
            Some(TestAction::Fire)
        );
    }

    #[test]
    fn wasd_binding_conflicts_are_atomic_for_every_key() {
        let preset_keys = [
            PhysicalKeyCode::KeyW,
            PhysicalKeyCode::KeyA,
            PhysicalKeyCode::KeyS,
            PhysicalKeyCode::KeyD,
        ];

        for occupied in preset_keys {
            let mut bindings = ActionBindings::new();
            assert!(bindings.bind(occupied, TestAction::Move).is_ok());

            assert_eq!(
                bindings.bind_wasd(TEST_AXIS).map(|_| ()),
                Err(DuplicateKeyBinding { key: occupied })
            );
            for key in preset_keys {
                let expected = (key == occupied).then_some(TestAction::Move);
                assert_eq!(bindings.action_for(key), expected, "occupied={occupied:?}");
            }
        }
    }

    #[test]
    fn wasd_binding_uses_deterministic_conflict_order_and_allows_repeated_actions() {
        let mut conflicted = ActionBindings::new();
        assert!(
            conflicted
                .bind(PhysicalKeyCode::KeyS, TestAction::Fire)
                .is_ok()
        );
        assert!(
            conflicted
                .bind(PhysicalKeyCode::KeyW, TestAction::Move)
                .is_ok()
        );
        assert_eq!(
            conflicted.bind_wasd(TEST_AXIS).map(|_| ()),
            Err(DuplicateKeyBinding {
                key: PhysicalKeyCode::KeyW,
            })
        );

        let repeated = DigitalAxis2d::new(
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
        );
        let mut bindings = ActionBindings::new();
        assert!(bindings.bind_wasd(repeated).is_ok());
        for key in [
            PhysicalKeyCode::KeyW,
            PhysicalKeyCode::KeyA,
            PhysicalKeyCode::KeyS,
            PhysicalKeyCode::KeyD,
        ] {
            assert_eq!(bindings.action_for(key), Some(TestAction::Move));
        }
    }

    #[test]
    fn arrow_binding_maps_axis_slots_and_coexists_with_wasd_and_manual_escape() {
        let mut bindings = ActionBindings::new();
        assert!(bindings.bind_wasd(TEST_AXIS).is_ok());
        assert!(bindings.bind_arrows(TEST_AXIS).is_ok());
        assert!(
            bindings
                .bind(PhysicalKeyCode::Escape, TestAction::Enter)
                .is_ok()
        );

        for (key, expected) in [
            (PhysicalKeyCode::ArrowLeft, TestAction::Left),
            (PhysicalKeyCode::ArrowRight, TestAction::Right),
            (PhysicalKeyCode::ArrowDown, TestAction::Down),
            (PhysicalKeyCode::ArrowUp, TestAction::Up),
        ] {
            assert_eq!(bindings.action_for(key), Some(expected));
        }
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::Escape),
            Some(TestAction::Enter)
        );
    }

    #[test]
    fn arrow_binding_conflicts_are_atomic_and_use_axis_slot_order() {
        let arrow_keys = [
            PhysicalKeyCode::ArrowLeft,
            PhysicalKeyCode::ArrowRight,
            PhysicalKeyCode::ArrowDown,
            PhysicalKeyCode::ArrowUp,
        ];

        for occupied in arrow_keys {
            let mut bindings = ActionBindings::new();
            assert!(bindings.bind(occupied, TestAction::Move).is_ok());

            assert_eq!(
                bindings.bind_arrows(TEST_AXIS).map(|_| ()),
                Err(DuplicateKeyBinding { key: occupied })
            );
            for key in arrow_keys {
                let expected = (key == occupied).then_some(TestAction::Move);
                assert_eq!(bindings.action_for(key), expected, "occupied={occupied:?}");
            }
        }

        let mut two_conflicts = ActionBindings::new();
        assert!(
            two_conflicts
                .bind(PhysicalKeyCode::ArrowUp, TestAction::Fire)
                .is_ok()
        );
        assert!(
            two_conflicts
                .bind(PhysicalKeyCode::ArrowRight, TestAction::Move)
                .is_ok()
        );
        assert_eq!(
            two_conflicts.bind_arrows(TEST_AXIS).map(|_| ()),
            Err(DuplicateKeyBinding {
                key: PhysicalKeyCode::ArrowRight,
            })
        );

        let repeated = DigitalAxis2d::new(
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
        );
        let mut bindings = ActionBindings::new();
        assert!(bindings.bind_arrows(repeated).is_ok());
        for key in arrow_keys {
            assert_eq!(bindings.action_for(key), Some(TestAction::Move));
        }
    }

    #[test]
    fn combined_directional_binding_maps_every_slot_and_preserves_manual_keys() {
        let mut bindings = ActionBindings::new();
        assert!(
            bindings
                .bind(PhysicalKeyCode::Escape, TestAction::Enter)
                .is_ok()
        );
        assert!(bindings.bind_wasd_and_arrows(TEST_AXIS).is_ok());

        for (key, expected) in [
            (PhysicalKeyCode::KeyW, TestAction::Up),
            (PhysicalKeyCode::KeyA, TestAction::Left),
            (PhysicalKeyCode::KeyS, TestAction::Down),
            (PhysicalKeyCode::KeyD, TestAction::Right),
            (PhysicalKeyCode::ArrowLeft, TestAction::Left),
            (PhysicalKeyCode::ArrowRight, TestAction::Right),
            (PhysicalKeyCode::ArrowDown, TestAction::Down),
            (PhysicalKeyCode::ArrowUp, TestAction::Up),
        ] {
            assert_eq!(bindings.action_for(key), Some(expected));
        }
        assert_eq!(
            bindings.action_for(PhysicalKeyCode::Escape),
            Some(TestAction::Enter)
        );
        assert_eq!(bindings.action_for(PhysicalKeyCode::Enter), None);
        assert_eq!(bindings.action_for(PhysicalKeyCode::Space), None);
        let before_repeat = ALL_PHYSICAL_KEYS.map(|key| bindings.action_for(key));
        assert_eq!(
            bindings.bind_wasd_and_arrows(TEST_AXIS).map(|_| ()),
            Err(DuplicateKeyBinding {
                key: PhysicalKeyCode::KeyW,
            })
        );
        assert_eq!(
            ALL_PHYSICAL_KEYS.map(|key| bindings.action_for(key)),
            before_repeat
        );
    }

    #[test]
    fn combined_directional_binding_conflicts_are_atomic_for_every_key() {
        let preset_keys = [
            PhysicalKeyCode::KeyW,
            PhysicalKeyCode::KeyA,
            PhysicalKeyCode::KeyS,
            PhysicalKeyCode::KeyD,
            PhysicalKeyCode::ArrowLeft,
            PhysicalKeyCode::ArrowRight,
            PhysicalKeyCode::ArrowDown,
            PhysicalKeyCode::ArrowUp,
        ];

        for occupied in preset_keys {
            let mut bindings = ActionBindings::new();
            assert!(bindings.bind(occupied, TestAction::Move).is_ok());
            assert!(
                bindings
                    .bind(PhysicalKeyCode::Enter, TestAction::Enter)
                    .is_ok()
            );
            let before = ALL_PHYSICAL_KEYS.map(|key| bindings.action_for(key));

            assert_eq!(
                bindings.bind_wasd_and_arrows(TEST_AXIS).map(|_| ()),
                Err(DuplicateKeyBinding { key: occupied })
            );
            assert_eq!(
                ALL_PHYSICAL_KEYS.map(|key| bindings.action_for(key)),
                before,
                "occupied={occupied:?}"
            );
        }
    }

    #[test]
    fn combined_directional_binding_uses_one_order_and_allows_repeated_actions() {
        let preset_keys = [
            PhysicalKeyCode::KeyW,
            PhysicalKeyCode::KeyA,
            PhysicalKeyCode::KeyS,
            PhysicalKeyCode::KeyD,
            PhysicalKeyCode::ArrowLeft,
            PhysicalKeyCode::ArrowRight,
            PhysicalKeyCode::ArrowDown,
            PhysicalKeyCode::ArrowUp,
        ];
        for first_index in 0..preset_keys.len() {
            for second_index in (first_index + 1)..preset_keys.len() {
                let mut conflicted = ActionBindings::new();
                assert!(
                    conflicted
                        .bind(preset_keys[second_index], TestAction::Fire)
                        .is_ok()
                );
                assert!(
                    conflicted
                        .bind(preset_keys[first_index], TestAction::Move)
                        .is_ok()
                );
                let before = ALL_PHYSICAL_KEYS.map(|key| conflicted.action_for(key));
                assert_eq!(
                    conflicted.bind_wasd_and_arrows(TEST_AXIS).map(|_| ()),
                    Err(DuplicateKeyBinding {
                        key: preset_keys[first_index],
                    })
                );
                assert_eq!(
                    ALL_PHYSICAL_KEYS.map(|key| conflicted.action_for(key)),
                    before
                );
            }
        }

        let repeated = DigitalAxis2d::new(
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
            TestAction::Move,
        );
        let mut bindings = ActionBindings::new();
        assert!(bindings.bind_wasd_and_arrows(repeated).is_ok());
        for key in preset_keys {
            assert_eq!(bindings.action_for(key), Some(TestAction::Move));
        }
    }

    #[test]
    fn digital_axis_descriptor_is_const_copy_and_preserves_slot_order() {
        const COPY: DigitalAxis2d<TestAction> = TEST_AXIS;

        assert_eq!(COPY.negative_x(), TestAction::Left);
        assert_eq!(COPY.positive_x(), TestAction::Right);
        assert_eq!(COPY.negative_y(), TestAction::Down);
        assert_eq!(COPY.positive_y(), TestAction::Up);
    }

    #[test]
    fn frame_and_fixed_digital_axes_cover_all_held_combinations() {
        let actions = [
            TestAction::Left,
            TestAction::Right,
            TestAction::Down,
            TestAction::Up,
        ];

        for mask in 0_u8..16 {
            let held: HashSet<_> = actions
                .into_iter()
                .enumerate()
                .filter_map(|(index, action)| (mask & (1 << index) != 0).then_some(action))
                .collect();
            let frame = FrameInputState {
                held: held.clone(),
                ..FrameInputState::empty()
            };
            let fixed = FixedInputState {
                held,
                ..FixedInputState::empty()
            };
            let expected = Vec2::new(
                f32::from(mask & 0b0010 != 0) - f32::from(mask & 0b0001 != 0),
                f32::from(mask & 0b1000 != 0) - f32::from(mask & 0b0100 != 0),
            );

            assert_eq!(frame.digital_axis(TEST_AXIS), expected, "mask {mask:04b}");
            assert_eq!(fixed.digital_axis(TEST_AXIS), expected, "mask {mask:04b}");
        }

        let diagonal = FrameInputState {
            held: HashSet::from([TestAction::Right, TestAction::Up]),
            ..FrameInputState::empty()
        }
        .digital_axis(TEST_AXIS);
        assert_eq!(diagonal, Vec2::ONE);
        assert!((diagonal.normalized().length() - 1.0).abs() < 0.000_1);
    }

    #[test]
    fn repeated_actions_follow_independent_slot_arithmetic() {
        let held = |action| action == TestAction::Move;
        let cancelled = DigitalAxis2d::new(
            TestAction::Move,
            TestAction::Move,
            TestAction::Enter,
            TestAction::Fire,
        );
        let diagonal = DigitalAxis2d::new(
            TestAction::Enter,
            TestAction::Move,
            TestAction::Fire,
            TestAction::Move,
        );

        assert_eq!(sample_digital_axis(cancelled, held), Vec2::ZERO);
        assert_eq!(sample_digital_axis(diagonal, held), Vec2::ONE);
    }

    #[test]
    fn digital_axis_does_not_consume_press_or_release_edges() {
        let (application, generation) = identity();
        let mut input = state(4);
        let events = [
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
        ];
        assert!(
            input
                .collect_frame(&events, false, application, generation, 1)
                .is_ok()
        );
        let axis = DigitalAxis2d::new(
            TestAction::Enter,
            TestAction::Move,
            TestAction::Fire,
            TestAction::Fire,
        );
        let frame = input.frame_input();
        let fixed = input.fixed_input();

        assert_eq!(frame.digital_axis(axis), Vec2::ZERO);
        assert_eq!(fixed.digital_axis(axis), Vec2::ZERO);
        assert_eq!(frame.pressed(TestAction::Move).count(), 1);
        assert_eq!(frame.released(TestAction::Move).count(), 1);
        assert_eq!(fixed.pressed(TestAction::Move).count(), 1);
        assert_eq!(fixed.released(TestAction::Move).count(), 1);
        assert_eq!(frame.digital_axis(axis), Vec2::ZERO);
        assert_eq!(fixed.digital_axis(axis), Vec2::ZERO);
    }

    #[test]
    fn space_produces_typed_press_and_release_edges() {
        let (application, generation) = identity();
        let mut input = state(2);

        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::Space,
                        ButtonState::Pressed,
                    )],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        assert!(input.frame_input().held(TestAction::Fire));
        assert_eq!(input.frame_input().pressed(TestAction::Fire).count(), 1);

        input.end_frame();
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::Space,
                        ButtonState::Released,
                    )],
                    false,
                    application,
                    generation,
                    2,
                )
                .is_ok()
        );
        assert!(!input.frame_input().held(TestAction::Fire));
        assert_eq!(input.frame_input().released(TestAction::Fire).count(), 1);
    }

    #[test]
    fn unreservable_input_limit_returns_a_typed_creation_error() {
        let result = InputState::new(bindings(), usize::MAX);

        assert!(matches!(
            result,
            Err(InputCollectionError::StorageAllocationFailed { limit: usize::MAX })
        ));
    }

    #[test]
    fn overflow_rejects_the_frame_before_mutating_input() {
        let (application, generation) = identity();
        let mut input = state(1);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        let before_frame = input.frame_input();
        let before_fixed = input.fixed_input();

        let result = input.collect_frame(
            &[
                InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
                InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
            ],
            false,
            application,
            generation,
            2,
        );

        assert_eq!(
            result,
            Err(InputCollectionError::EventLimitExceeded {
                limit: 1,
                received: 2,
            })
        );
        assert_eq!(
            before_frame.pressed(TestAction::Move).count(),
            input.frame_input().pressed(TestAction::Move).count()
        );
        assert_eq!(
            before_fixed.pressed(TestAction::Move).count(),
            input.fixed_input().pressed(TestAction::Move).count()
        );
        assert!(input.frame_input().held(TestAction::Move));
    }

    #[test]
    fn retained_fixed_edge_overflow_is_atomic_without_fixed_delivery() {
        let (application, generation) = identity();
        let mut input = state(2);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        input.end_frame();
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::KeyW,
                        ButtonState::Released,
                    )],
                    false,
                    application,
                    generation,
                    2,
                )
                .is_ok()
        );
        input.end_frame();

        let occurrence_before = input.next_occurrence;
        let result = input.collect_frame(
            &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed)],
            false,
            application,
            generation,
            3,
        );

        assert_eq!(
            result,
            Err(InputCollectionError::RetainedFixedEdgeLimitExceeded {
                limit: 2,
                retained: 2,
                incoming: 1,
            })
        );
        assert_eq!(input.next_occurrence, occurrence_before);
        assert!(!input.frame_input().held(TestAction::Move));
        assert_eq!(input.frame_input().pressed(TestAction::Move).count(), 0);
        assert_eq!(input.fixed_input().pressed(TestAction::Move).count(), 1);
        assert_eq!(input.fixed_input().released(TestAction::Move).count(), 1);
    }

    #[test]
    fn consuming_fixed_delivery_releases_the_retained_edge_budget() {
        let (application, generation) = identity();
        let mut input = state(1);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        input.end_frame();
        input.consume_fixed_delivery();

        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::KeyW,
                        ButtonState::Released,
                    )],
                    false,
                    application,
                    generation,
                    2,
                )
                .is_ok()
        );
        assert_eq!(input.fixed_input().released(TestAction::Move).count(), 1);
    }

    #[test]
    fn repeated_physical_states_do_not_create_edges() {
        let (application, generation) = identity();
        let mut input = state(8);
        let events = [
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
        ];

        let report = input.collect_frame(&events, false, application, generation, 1);
        let Ok(report) = report else {
            panic!("bounded input should be accepted");
        };

        assert_eq!(report.logical_edges, 2);
        assert_eq!(report.suppressed_repeats, 2);
        assert_eq!(input.frame_input().pressed(TestAction::Move).count(), 1);
        assert_eq!(input.frame_input().released(TestAction::Move).count(), 1);
        assert!(!input.frame_input().held(TestAction::Move));
    }

    #[test]
    fn every_supported_key_suppresses_repeated_physical_states() {
        let (application, generation) = identity();
        let new_keys = ALL_PHYSICAL_KEYS;
        let mut bindings = ActionBindings::new();
        for key in new_keys {
            assert!(bindings.bind(key, TestAction::Move).is_ok());
        }
        let mut input = InputState::new(bindings, new_keys.len() * 4).unwrap();
        let mut events = Vec::new();
        for key in new_keys {
            events.extend([
                InputEvent::key(key, ButtonState::Pressed),
                InputEvent::key(key, ButtonState::Pressed),
                InputEvent::key(key, ButtonState::Released),
                InputEvent::key(key, ButtonState::Released),
            ]);
        }

        let report = input
            .collect_frame(&events, false, application, generation, 1)
            .unwrap();

        assert_eq!(report.logical_edges, new_keys.len() * 2);
        assert_eq!(report.suppressed_repeats, new_keys.len() * 2);
        assert!(!input.frame_input().held(TestAction::Move));
    }

    #[test]
    fn wasd_and_arrow_edges_keep_shared_action_held_until_both_release() {
        let (application, generation) = identity();
        let mut bindings = ActionBindings::new();
        assert!(
            bindings
                .bind(PhysicalKeyCode::KeyW, TestAction::Move)
                .is_ok()
        );
        assert!(
            bindings
                .bind(PhysicalKeyCode::ArrowUp, TestAction::Move)
                .is_ok()
        );
        assert!(
            bindings
                .bind(PhysicalKeyCode::Escape, TestAction::Enter)
                .is_ok()
        );
        let mut input = InputState::new(bindings, 8).unwrap();
        let first = [
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::ArrowUp, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        ];
        input
            .collect_frame(&first, false, application, generation, 1)
            .unwrap();

        let frame = input.frame_input();
        let move_edges: Vec<_> = frame.pressed(TestAction::Move).collect();
        assert_eq!(move_edges.len(), 2);
        assert_ne!(move_edges[0].intent(), move_edges[1].intent());
        assert_eq!(
            frame.pressed(TestAction::Enter).next(),
            input.fixed_input().pressed(TestAction::Enter).next()
        );

        input
            .collect_frame(
                &[InputEvent::key(
                    PhysicalKeyCode::KeyW,
                    ButtonState::Released,
                )],
                false,
                application,
                generation,
                2,
            )
            .unwrap();
        assert!(input.frame_input().held(TestAction::Move));

        input
            .collect_frame(
                &[InputEvent::key(
                    PhysicalKeyCode::ArrowUp,
                    ButtonState::Released,
                )],
                false,
                application,
                generation,
                3,
            )
            .unwrap();
        assert!(!input.frame_input().held(TestAction::Move));
    }

    #[test]
    fn two_keys_for_one_action_keep_held_state_until_both_release() {
        let (application, generation) = identity();
        let mut input = state(8);
        assert!(
            input
                .collect_frame(
                    &[
                        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
                        InputEvent::key(PhysicalKeyCode::KeyA, ButtonState::Pressed),
                        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
                    ],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );

        let axis = DigitalAxis2d::new(
            TestAction::Enter,
            TestAction::Move,
            TestAction::Fire,
            TestAction::Fire,
        );
        assert!(input.frame_input().held(TestAction::Move));
        assert_eq!(input.frame_input().digital_axis(axis), Vec2::X);

        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::KeyA,
                        ButtonState::Released,
                    )],
                    false,
                    application,
                    generation,
                    2,
                )
                .is_ok()
        );
        assert!(!input.frame_input().held(TestAction::Move));
        assert_eq!(input.frame_input().digital_axis(axis), Vec2::ZERO);
    }

    #[test]
    fn fixed_edges_persist_until_one_tick_consumes_them() {
        let (application, generation) = identity();
        let mut input = state(8);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::Enter,
                        ButtonState::Pressed,
                    )],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        input.end_frame();

        assert!(
            input
                .collect_frame(&[], false, application, generation, 2)
                .is_ok()
        );
        assert_eq!(input.frame_input().pressed(TestAction::Enter).count(), 0);
        assert_eq!(input.fixed_input().pressed(TestAction::Enter).count(), 1);

        input.consume_fixed_delivery();
        assert_eq!(input.fixed_input().pressed(TestAction::Enter).count(), 0);
    }

    #[test]
    fn paused_events_update_frame_and_held_but_not_fixed_edges() {
        let (application, generation) = identity();
        let mut input = state(8);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed,)],
                    true,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );

        assert!(input.frame_input().held(TestAction::Move));
        assert_eq!(input.frame_input().pressed(TestAction::Move).count(), 1);
        assert_eq!(input.fixed_input().pressed(TestAction::Move).count(), 0);
    }

    #[test]
    fn frame_and_fixed_views_share_one_causal_token() {
        let (application, generation) = identity();
        let mut input = state(8);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::Enter,
                        ButtonState::Pressed,
                    )],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );

        let frame_token = input.frame_input().pressed(TestAction::Enter).next();
        let fixed_token = input.fixed_input().pressed(TestAction::Enter).next();
        assert_eq!(frame_token, fixed_token);
    }

    #[test]
    fn reusable_snapshots_preserve_data_tokens_and_capacity() {
        let (application, generation) = identity();
        let mut input = state(8);
        let events = [
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
        ];
        assert!(
            input
                .collect_frame(&events, false, application, generation, 1)
                .is_ok()
        );

        let expected_frame_edges = input.frame_edges.clone();
        let expected_fixed_edges = input.fixed_edges.clone();
        let mut frame = FrameInputState::empty();
        let mut fixed = FixedInputState::empty();
        input.copy_frame_snapshot_into(&mut frame);
        input.copy_fixed_snapshot_into(&mut fixed);

        assert_eq!(frame.edges, expected_frame_edges);
        assert_eq!(fixed.edges, expected_fixed_edges);
        assert_eq!(input.frame_edges, expected_frame_edges);
        assert_eq!(input.fixed_edges, expected_fixed_edges);
        assert!(frame.held(TestAction::Move));
        assert!(frame.held(TestAction::Enter));
        assert!(fixed.held(TestAction::Move));
        assert!(fixed.held(TestAction::Enter));

        let frame_held_capacity = frame.held.capacity();
        let frame_edge_capacity = frame.edges.capacity();
        let fixed_held_capacity = fixed.held.capacity();
        let fixed_edge_capacity = fixed.edges.capacity();
        frame.clear_reusing_storage();
        fixed.clear_reusing_storage();
        assert!(frame.held.is_empty());
        assert!(frame.edges.is_empty());
        assert!(fixed.held.is_empty());
        assert!(fixed.edges.is_empty());
        assert_eq!(frame.held.capacity(), frame_held_capacity);
        assert_eq!(frame.edges.capacity(), frame_edge_capacity);
        assert_eq!(fixed.held.capacity(), fixed_held_capacity);
        assert_eq!(fixed.edges.capacity(), fixed_edge_capacity);

        input.end_frame();
        input.consume_fixed_delivery();
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::KeyW,
                        ButtonState::Released,
                    )],
                    false,
                    application,
                    generation,
                    2,
                )
                .is_ok()
        );
        input.copy_frame_snapshot_into(&mut frame);
        input.copy_fixed_snapshot_into(&mut fixed);
        assert_eq!(frame.edges, input.frame_edges);
        assert_eq!(fixed.edges, input.fixed_edges);
        assert_eq!(frame.held.capacity(), frame_held_capacity);
        assert_eq!(frame.edges.capacity(), frame_edge_capacity);
        assert_eq!(fixed.held.capacity(), fixed_held_capacity);
        assert_eq!(fixed.edges.capacity(), fixed_edge_capacity);
    }

    #[test]
    fn using_a_token_consumes_its_occurrence_in_both_views() {
        let (application, generation) = identity();
        let mut input = state(8);
        assert!(
            input
                .collect_frame(
                    &[InputEvent::key(
                        PhysicalKeyCode::Enter,
                        ButtonState::Pressed,
                    )],
                    false,
                    application,
                    generation,
                    1,
                )
                .is_ok()
        );
        let edge = input.frame_input().pressed(TestAction::Enter).next();
        let Some(edge) = edge else {
            panic!("press edge should exist");
        };

        assert!(input.consume_token_occurrence(edge.intent()));
        assert!(!input.token_is_live(edge.intent()));
        assert_eq!(input.frame_input().pressed(TestAction::Enter).count(), 0);
        assert_eq!(input.fixed_input().pressed(TestAction::Enter).count(), 0);
    }
}

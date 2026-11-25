//! Explicit ordered keyboard routing; pointer capture stays with its existing owner.
use super::{ButtonState, PhysicalKeyCode, SUPPORTED_PHYSICAL_KEY_COUNT, physical_key_index};

/// One keyboard delivery to a caller-owned modal scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoutedKey<S> {
    /// Scope that accepted the original press.
    pub scope: S,
    /// Physical key identity, never a character.
    pub key: PhysicalKeyCode,
    /// Press or paired release.
    pub state: ButtonState,
    /// True for a scope change or explicit cancellation, never an activation.
    pub cancelled: bool,
}

/// Bounded keyboard press ownership for application-selected modal scopes.
/// Call once per occurrence in order, then dispatch only the returned delivery.
/// Do not also dispatch raw edges to gameplay. Text belongs to the active editor;
/// this helper does not consume runtime snapshots or change pointer capture.
/// Repeats and unmatched releases are suppressed. Scope changes cancel old
/// presses and quarantine their keys until release, including same-frame changes.
/// Store this in World state or explicitly cancel it on World replacement.
pub struct ModalKeyboard<S: Copy + Eq> {
    scope: S,
    held: [bool; SUPPORTED_PHYSICAL_KEY_COUNT],
    owners: [Option<S>; SUPPORTED_PHYSICAL_KEY_COUNT],
}
impl<S: Copy + Eq> ModalKeyboard<S> {
    /// Creates a router with no held keys and a caller-chosen initial scope.
    pub fn new(scope: S) -> Self {
        Self {
            scope,
            held: [false; SUPPORTED_PHYSICAL_KEY_COUNT],
            owners: [None; SUPPORTED_PHYSICAL_KEY_COUNT],
        }
    }
    /// Returns the scope receiving fresh presses and semantic editor events.
    pub const fn scope(&self) -> S {
        self.scope
    }
    /// Changes scope and returns cancellation releases in portable key order.
    /// Re-selecting the current scope changes nothing. No held key is replayed.
    pub fn set_scope(&mut self, scope: S) -> Vec<RoutedKey<S>> {
        if scope == self.scope {
            return Vec::new();
        }
        let cancelled = self.cancel();
        self.scope = scope;
        cancelled
    }
    /// Cancels owners and forgets held keys when native focus is lost. Unlike a
    /// scope change, focus loss may never deliver physical releases. Call this
    /// for raw InputEvent::FocusLost in FrameText::ordered so a fresh press after
    /// focus returns is accepted. Pointer capture is unaffected by this helper.
    pub fn focus_lost(&mut self) -> Vec<RoutedKey<S>> {
        let cancelled = self.cancel();
        self.held.fill(false);
        cancelled
    }

    /// Cancels owners while quarantining held keys until their physical release.
    /// Feed runtime focus-loss releases through route to clear their held state.
    pub fn cancel(&mut self) -> Vec<RoutedKey<S>> {
        super::ALL_PHYSICAL_KEYS
            .iter()
            .copied()
            .filter_map(|key| {
                self.owners[physical_key_index(key)]
                    .take()
                    .map(|scope| RoutedKey {
                        scope,
                        key,
                        state: ButtonState::Released,
                        cancelled: true,
                    })
            })
            .collect()
    }
    /// Consumes one ordered physical occurrence. A cancelled release clears
    /// ownership but must not trigger commit-on-release behavior.
    pub fn route(
        &mut self,
        key: PhysicalKeyCode,
        state: ButtonState,
        cancelled: bool,
    ) -> Option<RoutedKey<S>> {
        let index = physical_key_index(key);
        match state {
            ButtonState::Pressed => {
                if self.held[index] || cancelled {
                    return None;
                }
                self.held[index] = true;
                self.owners[index] = Some(self.scope);
                Some(RoutedKey {
                    scope: self.scope,
                    key,
                    state,
                    cancelled: false,
                })
            }
            ButtonState::Released => {
                self.held[index] = false;
                self.owners[index].take().map(|scope| RoutedKey {
                    scope,
                    key,
                    state,
                    cancelled,
                })
            }
        }
    }
}

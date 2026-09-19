//! Explicit keyboard focus without hidden input dispatch, layout, or widget storage.

use std::{error::Error, fmt};

/// One caller-accepted UI navigation action. Feed only fresh, uncancelled presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusCommand {
    /// Focus the next eligible target, wrapping at the end.
    Next,
    /// Focus the previous eligible target, wrapping at the start.
    Previous,
    /// Focus the first eligible target.
    First,
    /// Focus the last eligible target.
    Last,
    /// Report activation of the current eligible target; dispatch remains caller-owned.
    Activate,
    /// Clear focus, for example after input focus loss.
    Clear,
}

/// A rejected focus operation; previous scope and focused identity are unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusError {
    /// The supplied eligible order exceeds the configured maximum.
    TargetLimitExceeded,
    /// Repeated target IDs make navigation ambiguous.
    DuplicateTarget,
    /// An explicit focus request names an ineligible target.
    IneligibleTarget,
}
impl fmt::Display for FocusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid keyboard focus request: {self:?}")
    }
}
impl Error for FocusError {}

/// Explicit focus/activation output. No runtime input is consumed by the helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusOutcome<T> {
    /// Current eligible focus after this operation.
    pub focused: Option<T>,
    /// Target activated by this command, if any.
    pub activated: Option<T>,
}

/// An allocation-free focus controller for one caller-selected modal scope.
///
/// The caller supplies a bounded, ordered slice of unique eligible target IDs on
/// every operation. Disabled/hidden targets must be omitted. Changing scope or
/// removing the current target clears focus before interpreting the command;
/// activation never silently moves to another target. Use generation-qualified
/// entity IDs, or clear/recreate the controller when a World is replaced.
///
/// This helper registers no systems and reads no keys. The application chooses
/// Tab/Shift-Tab/arrow bindings and routes each accepted action exactly once.
/// Equality is caller code. Validation costs O(n^2) comparisons, bounded by the
/// configured target limit; navigation is O(n), storage is constant, no heap work.
#[derive(Debug)]
pub struct KeyboardFocus<T: Copy + Eq, S: Copy + Eq> {
    focused: Option<T>,
    scope: Option<S>,
    max_targets: usize,
}

impl<T: Copy + Eq, S: Copy + Eq> KeyboardFocus<T, S> {
    /// Creates an empty controller with an explicit eligible-target cap.
    pub const fn new(max_targets: usize) -> Self {
        Self {
            focused: None,
            scope: None,
            max_targets,
        }
    }
    /// Returns focused identity; call synchronize after changing eligibility.
    pub const fn focused(&self) -> Option<T> {
        self.focused
    }
    /// Returns the last explicitly supplied modal scope.
    pub const fn scope(&self) -> Option<S> {
        self.scope
    }
    /// Clears focus without changing the caller's active scope.
    pub fn clear(&mut self) {
        self.focused = None;
    }

    /// Reconciles scope and eligibility without choosing or activating a new target.
    pub fn synchronize(&mut self, scope: S, eligible: &[T]) -> Result<(), FocusError> {
        self.validate(eligible)?;
        if self.scope != Some(scope)
            || self
                .focused
                .is_some_and(|target| !eligible.contains(&target))
        {
            self.focused = None;
        }
        self.scope = Some(scope);
        Ok(())
    }

    /// Sets an eligible focus explicitly, for example after pointer selection.
    /// Invalid target/order/limits preserve both old scope and old focus.
    pub fn set_focused(
        &mut self,
        scope: S,
        eligible: &[T],
        target: Option<T>,
    ) -> Result<(), FocusError> {
        self.validate(eligible)?;
        if target.is_some_and(|target| !eligible.contains(&target)) {
            return Err(FocusError::IneligibleTarget);
        }
        self.scope = Some(scope);
        self.focused = target;
        Ok(())
    }

    /// Applies one caller-approved command after reconciling the active modal scope.
    /// Empty orders clear focus. Failed validation performs no mutation.
    pub fn process(
        &mut self,
        scope: S,
        eligible: &[T],
        command: FocusCommand,
    ) -> Result<FocusOutcome<T>, FocusError> {
        self.synchronize(scope, eligible)?;
        let index = self
            .focused
            .and_then(|target| eligible.iter().position(|item| *item == target));
        let mut activated = None;
        match command {
            FocusCommand::Clear => self.focused = None,
            FocusCommand::Activate => activated = self.focused,
            FocusCommand::First => self.focused = eligible.first().copied(),
            FocusCommand::Last => self.focused = eligible.last().copied(),
            FocusCommand::Next if !eligible.is_empty() => {
                self.focused = eligible
                    .get(index.map_or(0, |index| (index + 1) % eligible.len()))
                    .copied();
            }
            FocusCommand::Previous if !eligible.is_empty() => {
                self.focused = eligible
                    .get(
                        index
                            .filter(|index| *index > 0)
                            .map_or(eligible.len() - 1, |index| index - 1),
                    )
                    .copied();
            }
            FocusCommand::Next | FocusCommand::Previous => {}
        }
        Ok(FocusOutcome {
            focused: self.focused,
            activated,
        })
    }

    fn validate(&self, eligible: &[T]) -> Result<(), FocusError> {
        if eligible.len() > self.max_targets {
            return Err(FocusError::TargetLimitExceeded);
        }
        for (index, target) in eligible.iter().enumerate() {
            if eligible[..index].contains(target) {
                return Err(FocusError::DuplicateTarget);
            }
        }
        Ok(())
    }
}

//! Bounded semantic text input, independent of physical action bindings.
use bevy_ecs::{
    prelude::{Res, Resource},
    system::SystemParam,
};
use std::{error::Error, fmt};

/// Semantic editor command. Selection extension is carried separately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditCommand {
    /// Delete the preceding grapheme (segmentation belongs to the editor).
    Backspace,
    /// Delete the following grapheme.
    Delete,
    /// Move toward the previous caret position.
    Left,
    /// Move toward the next caret position.
    Right,
    /// Move to the beginning of the line.
    Home,
    /// Move to the end of the line.
    End,
    /// Insert a line break or accept, according to application policy.
    Enter,
    /// Leave or cancel the editor.
    Escape,
}

/// One semantic occurrence. Debug deliberately redacts all user text.
#[derive(Clone, PartialEq, Eq)]
pub enum TextEvent {
    /// Committed UTF-8 from the keyboard layout or IME; never physical key names.
    Commit(String),
    /// Caller-provided clipboard content; no implicit clipboard reads.
    Paste(String),
    /// IME candidate and optional UTF-8 byte selection endpoints.
    Preedit {
        /// Uncommitted text, which must not be saved as document content.
        text: String,
        /// Endpoints must be character boundaries inside text; reversed selections are valid.
        selection: Option<(usize, usize)>,
    },
    /// Logical editing command, including native repeat occurrences.
    Edit {
        /// Editing intent, independent of keyboard position.
        command: EditCommand,
        /// Whether the platform Shift modifier requests extending selection.
        extend_selection: bool,
    },
    /// Composition/focus ended. Clear preedit and pending editor gestures.
    Cancel,
}
impl fmt::Debug for TextEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Commit(_) => f.write_str("Commit(<redacted>)"),
            Self::Paste(_) => f.write_str("Paste(<redacted>)"),
            Self::Preedit { .. } => f.write_str("Preedit(<redacted>)"),
            Self::Edit {
                command,
                extend_selection,
            } => f
                .debug_struct("Edit")
                .field("command", command)
                .field("extend_selection", extend_selection)
                .finish(),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}
impl TextEvent {
    fn bytes(&self) -> usize {
        match self {
            Self::Commit(s) | Self::Paste(s) | Self::Preedit { text: s, .. } => s.len(),
            _ => 0,
        }
    }
}
/// Rejected text batch. No text content is included in diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputError {
    /// One event exceeds 16 KiB, the batch exceeds 64 KiB or 1024 events.
    LimitExceeded,
    /// A preedit selection is outside the text or splits a UTF-8 code point.
    InvalidSelection,
    /// Text markers must enumerate every semantic event exactly once in order.
    InvalidOrder,
}
impl fmt::Display for TextInputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "text input rejected: {self:?}")
    }
}
impl Error for TextInputError {}

/// Validates before the runtime changes time, physical input or World state.
/// Limits count UTF-8 bytes, including repeated preedit snapshots and paste.
pub fn validate_text_events(events: &[TextEvent]) -> Result<(), TextInputError> {
    if events.len() > 1024 {
        return Err(TextInputError::LimitExceeded);
    }
    let mut total = 0usize;
    for event in events {
        let bytes = event.bytes();
        if bytes > 16 * 1024 {
            return Err(TextInputError::LimitExceeded);
        }
        total += bytes;
        if total > 64 * 1024 {
            return Err(TextInputError::LimitExceeded);
        }
        if let TextEvent::Preedit {
            text,
            selection: Some((start, end)),
        } = event
            && (!text.is_char_boundary(*start) || !text.is_char_boundary(*end))
        {
            return Err(TextInputError::InvalidSelection);
        }
    }
    Ok(())
}

#[derive(Default, Resource)]
pub(crate) struct TextFrameState(pub Vec<TextEvent>, pub Vec<super::InputEvent>);

/// One occurrence in the shared physical/semantic source order.
#[derive(Debug)]
pub enum EditorEvent<'a> {
    /// Raw physical occurrence, including keys without action bindings.
    Physical(super::InputEvent),
    /// Semantic editor occurrence, never interpreted as a gameplay key.
    Text(&'a TextEvent),
}

pub(crate) fn validate_text_order(
    events: &[super::InputEvent],
    text: &[TextEvent],
) -> Result<(), TextInputError> {
    let mut next = 0;
    for event in events {
        if let super::InputEvent::Text { index } = event {
            if *index != next || next >= text.len() {
                return Err(TextInputError::InvalidOrder);
            }
            next += 1;
        }
    }
    if next != 0 && next != text.len() {
        return Err(TextInputError::InvalidOrder);
    }
    Ok(())
}

/// FrameUpdate-only semantic input. Empty during other lifecycle stages.
/// Events are delivered once in source order, never replayed into fixed ticks.
/// A World transition that skips FrameUpdate discards this frame's text.
#[derive(SystemParam)]
pub struct FrameText<'w> {
    state: Res<'w, TextFrameState>,
}
impl FrameText<'_> {
    /// Iterates physical and text events in source order. If no text markers
    /// were supplied, text follows all physical events. Use this single stream
    /// for same-frame open/type/close routing instead of a second raw dispatch.
    pub fn ordered(&self) -> impl Iterator<Item = EditorEvent<'_>> {
        let marked = self
            .state
            .1
            .iter()
            .any(|e| matches!(e, super::InputEvent::Text { .. }));
        self.state
            .1
            .iter()
            .filter_map(|event| match event {
                super::InputEvent::Text { index } => {
                    self.state.0.get(*index).map(EditorEvent::Text)
                }
                other => Some(EditorEvent::Physical(*other)),
            })
            .chain(
                self.state
                    .0
                    .iter()
                    .filter(move |_| !marked)
                    .map(EditorEvent::Text),
            )
    }

    /// Ordered committed text, composition updates, editing commands and cancellations.
    pub fn events(&self) -> &[TextEvent] {
        &self.state.0
    }
}

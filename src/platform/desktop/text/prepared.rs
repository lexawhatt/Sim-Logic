//! Exact-DPI layout selection before Engine's prepared-line atlas handoff.

use sim_engine::ShapedLine;

use crate::text::{ScreenTextVisual, TextError};

/// A scale-1 label can hand off its immutable CPU layout without reshaping.
/// Engine includes DPI in provenance, so other scales require a distinct line.
pub(super) enum DesktopPreparedLine<'visual> {
    Shared(&'visual ShapedLine),
    DpiSpecific(ShapedLine),
}

impl<'visual> DesktopPreparedLine<'visual> {
    pub(super) fn new(visual: &'visual ScreenTextVisual, scale: f32) -> Result<Self, TextError> {
        let font = visual.font();
        let settings = font.settings();
        let style = settings.style(scale)?;
        let line = visual.shaped_line();
        if *line.style() == style {
            line.validate_for(font.face(), &style, &settings.layout_budget())?;
            Ok(Self::Shared(line))
        } else {
            // Never relabel provenance or reinterpret a scale-1 prepared line as
            // a different-DPI line. This is an explicit changed-label/rebuild
            // cost; unchanged GPU runs bypass this helper completely.
            let shaped =
                font.face()
                    .shape_line(visual.text(), &style, &settings.layout_budget())?;
            Ok(Self::DpiSpecific(shaped))
        }
    }

    pub(super) fn line(&self) -> &ShapedLine {
        match self {
            Self::Shared(line) => line,
            Self::DpiSpecific(line) => line,
        }
    }
}

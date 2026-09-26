//! Allocation-free native grouping; never crosses another screen source.

use crate::{ExtractedFrame, ScreenDraw};

use super::images::DesktopImageError;

pub(super) enum DesktopDraw {
    Single(ScreenDraw),
    #[cfg(feature = "text")]
    TextBatch(std::ops::Range<usize>),
}

/// A small fixed partition bounds update work without creating a global sort.
#[cfg(feature = "text")]
pub(super) const MAX_TEXT_PLACEMENTS: usize = 256;

pub(super) fn draws(
    extracted: &ExtractedFrame,
    batching: bool,
) -> impl Iterator<Item = Result<DesktopDraw, DesktopImageError>> {
    Draws {
        extracted,
        batching,
        next: 0,
    }
}

struct Draws<'a> {
    extracted: &'a ExtractedFrame,
    #[cfg_attr(not(feature = "text"), allow(dead_code))]
    batching: bool,
    next: usize,
}

impl Iterator for Draws<'_> {
    type Item = Result<DesktopDraw, DesktopImageError>;

    fn next(&mut self) -> Option<Self::Item> {
        let draw = *self.extracted.screen_draws().get(self.next)?;
        self.next += 1;
        #[cfg(feature = "text")]
        if self.batching
            && let ScreenDraw::Text { index } = draw
        {
            let labels = self.extracted.resolved_screen_texts();
            let Some(first) = labels.get(index) else {
                return Some(Err(DesktopImageError::InvalidDrawPlan));
            };
            if visible(first) {
                let mut count = 1;
                while count < MAX_TEXT_PLACEMENTS {
                    let Some(ScreenDraw::Text { index: next_index }) =
                        self.extracted.screen_draws().get(self.next)
                    else {
                        break;
                    };
                    // Published text indices are monotonic; check rather than
                    // accidentally include a text not belonging to this group.
                    if *next_index != index + count {
                        break;
                    }
                    let Some(next) = labels.get(*next_index) else {
                        break;
                    };
                    if next.font() != first.font() || !visible(next) {
                        break;
                    }
                    count += 1;
                    self.next += 1;
                }
                return Some(Ok(DesktopDraw::TextBatch(index..index + count)));
            }
        }
        Some(Ok(DesktopDraw::Single(draw)))
    }
}

#[cfg(feature = "text")]
pub(super) fn visible(text: &crate::ResolvedScreenText) -> bool {
    !text.text().is_empty() && text.visual().clip() != crate::screen::ScreenClip::Empty
}

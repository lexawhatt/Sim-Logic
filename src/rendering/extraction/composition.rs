//! Exact mixed screen ordering and bounded, reusable rectangle runs.

use std::mem::size_of;

use sim_engine::{Color, SceneBudget, ScreenScene, ShapeStyle};

use super::{
    super::{ExtractionError, compare_visual_order},
    ScreenExtractionBuffer,
};

/// One ordered screen source, after all world content.
///
/// Images index `ExtractedFrame::resolved_screen_images`; rectangle runs are
/// inspected with `ExtractedFrame::screen_rectangle_run_records`. These indices
/// belong only to their containing snapshot, not to future frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenDraw {
    /// One contiguous subsequence of sorted screen rectangles.
    Rectangles {
        /// Snapshot-local rectangle-run index.
        run: usize,
    },
    /// One registered image placed between rectangle runs or other images.
    Image {
        /// Index into the snapshot's resolved image list.
        index: usize,
    },
    /// One prepared screen label, sharing mixed painter order with other kinds.
    #[cfg(feature = "text")]
    Text {
        /// Index into the snapshot's resolved text list.
        index: usize,
    },
}

#[derive(Clone, Copy)]
enum NonRectangleKind {
    Image,
    #[cfg(feature = "text")]
    Text,
}

pub(super) struct RectangleRun {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) scene: ScreenScene,
}

impl ScreenExtractionBuffer {
    pub(super) fn compose(&mut self, budget: SceneBudget) -> Result<(), ExtractionError> {
        let result = self.compose_inner(budget);
        if result.is_err() {
            // Rejected repartitions must not retain new large prefix runs
            // beside untouched large tails from earlier failed attempts.
            self.runs.clear();
            self.draws.clear();
        }
        result
    }

    fn compose_inner(&mut self, budget: SceneBudget) -> Result<(), ExtractionError> {
        self.draws.clear();
        if !self.has_non_rectangles() {
            // Keep the established single-scene path and do not build duplicate
            // rectangle runs for applications that have not enabled images.
            self.runs.clear();
            if !self.resolved.is_empty() {
                self.push_draw(ScreenDraw::Rectangles { run: 0 })?;
            }
            return Ok(());
        }

        let mut rectangle = 0;
        let mut image = 0;
        #[cfg(feature = "text")]
        let mut text = 0;
        let mut run_start = 0;
        let mut run_count = 0;
        loop {
            let next = self.images.get(image).map(|image| {
                (
                    NonRectangleKind::Image,
                    image.layer(),
                    image.draw_order_depth(),
                    image.source(),
                )
            });
            #[cfg(feature = "text")]
            let next = match (next, self.texts.get(text)) {
                (Some(image), Some(text))
                    if !compare_visual_order(
                        image.1,
                        image.2,
                        image.3,
                        text.layer(),
                        text.draw_order_depth(),
                        text.source(),
                    )
                    .is_gt() =>
                {
                    Some(image)
                }
                (_, Some(text)) => Some((
                    NonRectangleKind::Text,
                    text.layer(),
                    text.draw_order_depth(),
                    text.source(),
                )),
                (image, None) => image,
            };
            let take_rectangle = match (self.resolved.get(rectangle), next) {
                (Some(left), Some(right)) => !compare_visual_order(
                    left.layer(),
                    left.draw_order_depth(),
                    left.source(),
                    right.1,
                    right.2,
                    right.3,
                )
                .is_gt(),
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_rectangle {
                rectangle += 1;
            } else if let Some((kind, _, _, _)) = next {
                if run_start != rectangle {
                    self.prepare_run(run_count, run_start, rectangle, budget)?;
                    self.push_draw(ScreenDraw::Rectangles { run: run_count })?;
                    run_count += 1;
                }
                match kind {
                    NonRectangleKind::Image => {
                        self.push_draw(ScreenDraw::Image { index: image })?;
                        image += 1;
                    }
                    #[cfg(feature = "text")]
                    NonRectangleKind::Text => {
                        self.push_draw(ScreenDraw::Text { index: text })?;
                        text += 1;
                    }
                }
                run_start = rectangle;
            }
        }
        if run_start != rectangle {
            self.prepare_run(run_count, run_start, rectangle, budget)?;
            self.push_draw(ScreenDraw::Rectangles { run: run_count })?;
            run_count += 1;
        }
        self.runs.truncate(run_count);
        Ok(())
    }

    fn push_draw(&mut self, draw: ScreenDraw) -> Result<(), ExtractionError> {
        self.draws
            .try_reserve(1)
            .map_err(|_| ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ScreenDraw>(),
            })?;
        self.draws.push(draw);
        Ok(())
    }

    fn prepare_run(
        &mut self,
        index: usize,
        start: usize,
        end: usize,
        budget: SceneBudget,
    ) -> Result<(), ExtractionError> {
        let count = end - start;
        // Replacing a scene whenever its run length changes prevents each slot
        // retaining its historical largest partition. The command cap bounds
        // content, not allocator slack; Engine still enforces Scene byte caps.
        // The aggregate full ScreenScene already validated total limits.
        let run_budget = SceneBudget::new(
            count,
            0,
            budget.max_tessellated_vertices(),
            budget.max_retained_bytes(),
            budget.max_allocation_bytes(),
            budget.max_upload_bytes(),
            budget.max_draw_batches(),
        );
        if index == self.runs.len() {
            self.runs
                .try_reserve(1)
                .map_err(|_| ExtractionError::AllocationFailed {
                    requested_bytes: size_of::<RectangleRun>(),
                })?;
            let scene = ScreenScene::with_budget(Color::TRANSPARENT, run_budget)
                .map_err(ExtractionError::ScreenScene)?;
            self.runs.push(RectangleRun { start, end, scene });
        } else if self.runs[index].end - self.runs[index].start != count {
            // Construction is empty; dropping the previous scene releases its
            // command storage before the changed partition is populated.
            self.runs[index].scene = ScreenScene::with_budget(Color::TRANSPARENT, run_budget)
                .map_err(ExtractionError::ScreenScene)?;
        }
        let run = &mut self.runs[index];
        run.start = start;
        run.end = end;
        run.scene.clear();
        for rectangle in &self.resolved[start..end] {
            run.scene
                .try_square_rect_on_layer(
                    rectangle.layer(),
                    rectangle.position(),
                    rectangle.size(),
                    ShapeStyle::filled(rectangle.color()),
                )
                .map_err(ExtractionError::ScreenScene)?;
        }
        Ok(())
    }
}

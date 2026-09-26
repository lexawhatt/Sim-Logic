//! Retained snapshots of adjacent labels. Individual runs remain atlas sources.

use std::time::{Duration, Instant};

use sim_engine::{GlyphBatch2d, GlyphBatchBudget, GlyphRunPlacement2d};

use super::*;
use crate::desktop::draw_plan::{self, DesktopDraw, MAX_TEXT_PLACEMENTS};
use crate::screen::ScreenClip;

/// Native text preparation outside Engine's surface-frame timings.
///
/// Byte counters cover only additional batch snapshots, not source text runs,
/// font atlases, renderer bindings, temporary placements or driver allocations.
/// Source runs remain retained independently. This is not a total text-memory
/// or zero-allocation claim. Counters describe the last successful preparation.
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopTextUpdates {
    /// Exact-value groups reused without an Engine batch update or upload.
    pub reused_batches: usize,
    /// New batches, including replacements on font change or capacity growth.
    pub created_batches: usize,
    /// Changed snapshots submitted to the capacity-aware Engine updater.
    pub updated_batches: usize,
    /// Current grouped labels, including whitespace-only labels.
    pub placements: usize,
    /// Actual raster glyph instances across current groups.
    pub glyphs: usize,
    /// Ordered clip spans; different clips cannot merge into one draw call.
    pub draw_calls: usize,
    /// Actual batch-instance upload bytes; excludes source-run and atlas uploads.
    pub uploaded_bytes: usize,
    /// Batch recovery storage and comparison-record capacities, excluding shared
    /// prepared strings/font storage and fixed metadata.
    pub retained_cpu_bytes: usize,
    /// Live batch instance buffer allocations, separate from source runs.
    pub retained_gpu_bytes: usize,
    /// Source atlas/run lookup and preparation time, before grouping.
    pub source_cpu_time: Duration,
    /// Group comparison, placement conversion and resource-update CPU wall time.
    pub batch_cpu_time: Duration,
}

pub(super) struct BatchEntry {
    pub(super) records: Vec<ResolvedScreenText>,
    font: TextFont,
    resource: GlyphBatch2d,
}

impl DesktopText {
    pub(super) fn prepare_batches(
        &mut self,
        renderer: &WgpuRenderer,
        extracted: &ExtractedFrame,
    ) -> Result<(), DesktopTextError> {
        let started = Instant::now();
        let mut count = 0;
        for draw in draw_plan::draws(extracted, true) {
            let DesktopDraw::TextBatch(range) =
                draw.map_err(|_| DesktopTextError::InvalidDrawPlan)?
            else {
                continue;
            };
            let records = &extracted.resolved_screen_texts()[range];
            if let Some(entry) = self.batches.get(count)
                && entry.records == records
            {
                self.updates.reused_batches += 1;
            } else {
                self.update_batch(renderer, count, records)?;
            }
            count += 1;
        }
        self.batches.truncate(count);
        for entry in &self.batches {
            let resource = &entry.resource;
            self.updates.placements += resource.placement_count();
            self.updates.glyphs += resource.glyph_count();
            self.updates.draw_calls += resource.draw_count();
            self.updates.retained_cpu_bytes = self.updates.retained_cpu_bytes.saturating_add(
                resource.recovery_memory_bytes().saturating_add(
                    entry
                        .records
                        .capacity()
                        .saturating_mul(size_of::<ResolvedScreenText>()),
                ),
            );
            self.updates.retained_gpu_bytes = self
                .updates
                .retained_gpu_bytes
                .saturating_add(resource.gpu_allocation_bytes());
        }
        self.updates.batch_cpu_time = started.elapsed();
        Ok(())
    }

    fn update_batch(
        &mut self,
        renderer: &WgpuRenderer,
        index: usize,
        records: &[ResolvedScreenText],
    ) -> Result<(), DesktopTextError> {
        let first = records.first().ok_or(DesktopTextError::InvalidDrawPlan)?;
        let font_index = self
            .cache
            .fonts
            .binary_search_by_key(&first.font().slot(), |entry| entry.font.slot())
            .map_err(|_| DesktopTextError::InvalidDrawPlan)?;
        let atlas = &self.cache.fonts[font_index].atlas;
        let mut placements = Vec::new();
        reserve(&mut placements, records.len())?;
        let mut glyphs = 0usize;
        for visual in records {
            let run_index = self
                .cache
                .runs
                .binary_search_by_key(&visual.source().stable_bits(), |run| {
                    run.source.stable_bits()
                })
                .map_err(|_| DesktopTextError::InvalidDrawPlan)?;
            let run = &self.cache.runs[run_index];
            if run.source != visual.source()
                || run.font != *first.font()
                || run.font != *visual.font()
                || run.run.text() != visual.text()
            {
                return Err(DesktopTextError::InvalidDrawPlan);
            }
            let position = visual.baseline_origin().to_vec2();
            let placement = ImageBatchPlacement::new(
                LogicalScreenVector::new(position.x(), position.y()),
                visual.tint(),
            )
            .map_err(|error| DesktopTextError::Batch {
                source: first.source(),
                error: sim_engine::GlyphError::Image(error),
            })?;
            let mut placed = GlyphRunPlacement2d::new(run.run.glyph_run(), placement);
            match visual.visual().clip() {
                ScreenClip::Rectangle(clip) => placed = placed.with_clip(clip),
                ScreenClip::Unclipped => (),
                ScreenClip::Empty => return Err(DesktopTextError::InvalidDrawPlan),
            }
            glyphs = glyphs
                .checked_add(run.run.glyph_run().glyph_count())
                .ok_or(DesktopTextError::BatchSizeOverflow)?;
            placements.push(placed);
        }
        let error = |error| DesktopTextError::Batch {
            source: first.source(),
            error,
        };
        if let Some(entry) = self.batches.get_mut(index)
            && entry.font == *first.font()
            && glyphs <= entry.resource.budget().max_glyphs()
        {
            let additional = records.len().saturating_sub(entry.records.len());
            reserve(&mut entry.records, additional)?;
            let report = renderer
                .update_glyph_batch(atlas.atlas(), &mut entry.resource, &placements)
                .map_err(error)?;
            entry.records.clear();
            entry.records.extend_from_slice(records);
            self.updates.updated_batches += 1;
            self.updates.uploaded_bytes = self
                .updates
                .uploaded_bytes
                .saturating_add(report.instances().uploaded_instance_bytes());
        } else {
            let mut keys = Vec::new();
            reserve(&mut keys, records.len())?;
            keys.extend_from_slice(records);
            if index == self.batches.len() {
                reserve(&mut self.batches, 1)?;
            }
            let budget = batch_budget(glyphs).map_err(error)?;
            let resource = renderer
                .create_glyph_batch(atlas.atlas(), &placements, budget)
                .map_err(error)?;
            // Nonempty creation uploads the entire exact-sized instance buffer.
            // Empty groups allocate a sentinel but upload no instance bytes.
            self.updates.uploaded_bytes =
                self.updates.uploaded_bytes.saturating_add(if glyphs == 0 {
                    0
                } else {
                    resource.gpu_allocation_bytes()
                });
            let entry = BatchEntry {
                records: keys,
                font: first.font().clone(),
                resource,
            };
            if index == self.batches.len() {
                self.batches.push(entry);
            } else {
                self.batches[index] = entry;
            }
            self.updates.created_batches += 1;
        }
        Ok(())
    }

    pub(in crate::desktop) fn draw_batch<'a>(
        &'a self,
        frame: &mut FrameComposer<'a>,
        index: usize,
        options: FramePassOptions,
    ) -> Result<(), super::super::images::ScreenPresentationError> {
        let entry = self
            .batches
            .get(index)
            .ok_or(DesktopTextError::InvalidDrawPlan)?;
        let font_index = self
            .cache
            .fonts
            .binary_search_by_key(&entry.font.slot(), |font| font.font.slot())
            .map_err(|_| DesktopTextError::InvalidDrawPlan)?;
        frame.draw_glyph_batch(
            self.cache.fonts[font_index].atlas.atlas(),
            &entry.resource,
            ImageSampling::Linear,
            options,
        )?;
        Ok(())
    }
}

fn reserve<T>(items: &mut Vec<T>, additional: usize) -> Result<(), DesktopTextError> {
    items
        .try_reserve_exact(additional)
        .map_err(|_| DesktopTextError::Allocation {
            requested_bytes: additional.saturating_mul(size_of::<T>()),
        })
}

fn batch_budget(glyphs: usize) -> Result<GlyphBatchBudget, sim_engine::GlyphError> {
    let overflow = || sim_engine::GlyphError::InvalidBudget;
    let glyphs = glyphs
        .max(1)
        .checked_next_power_of_two()
        .ok_or_else(overflow)?;
    let bytes = glyphs
        .checked_mul(GlyphBatchBudget::RETAINED_BYTES_PER_GLYPH)
        .and_then(|bytes| {
            bytes.checked_add(MAX_TEXT_PLACEMENTS * GlyphBatchBudget::RETAINED_BYTES_PER_PLACEMENT)
        })
        .ok_or_else(overflow)?;
    GlyphBatchBudget::new(MAX_TEXT_PLACEMENTS, glyphs, bytes)
}

//! Exact-value retained screen runs. Engine alone builds and validates geometry.

use std::{
    error::Error,
    fmt,
    mem::size_of,
    time::{Duration, Instant},
};

use sim_engine::{
    FrameComposer, FrameComposerError, FramePassOptions, PreparedSceneError, PreparedScreenScene,
    ScreenPrimitive2d, ScreenPrimitiveBatch2d, ScreenPrimitiveBudget, ScreenPrimitiveError,
    WgpuRenderer,
};

use crate::{ExtractedFrame, ResolvedScreenPrimitive, ScreenDraw, identity::WorldGeneration};

/// Desktop geometry route. Both retained routes preserve mixed painter order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DesktopScreenMode {
    /// Engine analytic primitives where supported; prepared triangles otherwise.
    /// Compact primitives have Engine's explicit subpixel precision envelope.
    Compact,
    /// Retain ordinary tessellation, including its polygonal circle appearance.
    #[default]
    Prepared,
    /// Legacy streaming route, useful for paired measurements on the same source.
    Streaming,
}

/// Resource preparation outside Engine's surface-frame metrics. Not GPU timing.
/// In Compact mode, run counters count ordered sub-runs (at most 256 records).
#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopScreenUpdates {
    /// Runs reused by exact source/value comparison without tessellation or upload.
    pub reused_runs: usize,
    /// Newly prepared ordinary runs.
    pub prepared_runs: usize,
    /// Created compact runs.
    pub compact_runs: usize,
    /// Compact runs updated without changing their kind.
    pub updated_runs: usize,
    /// Actual geometry uploads during successful preparation, excluding uniforms.
    pub uploaded_bytes: usize,
    /// Current retained Engine recovery storage plus comparison-record capacity.
    /// Excludes fixed object metadata, temporary conversion, driver and in-flight memory.
    /// Shared source path points/styles belong to the CPU visual/snapshot, not this count.
    pub retained_cpu_bytes: usize,
    /// CPU wall time for comparison, construction and queueing resource updates.
    pub cpu_time: Duration,
}

/// A retained screen resource could not be prepared; no partial frame is drawn.
#[derive(Debug)]
pub enum DesktopScreenError {
    /// Engine rejected reconstruction of one ordered CPU sub-run.
    Scene(sim_engine::SceneError),
    /// Ordinary Engine preparation failed.
    Prepared(PreparedSceneError),
    /// Compact Engine preparation failed.
    Compact(ScreenPrimitiveError),
    /// Bounded comparison/cache metadata allocation failed.
    Allocation {
        /// Requested additional bytes.
        requested_bytes: usize,
    },
    /// The private draw plan does not match its published snapshot.
    InvalidDrawPlan,
}

impl fmt::Display for DesktopScreenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "screen sub-run: {error}"),
            Self::Prepared(error) => write!(f, "retained screen geometry: {error}"),
            Self::Compact(error) => write!(f, "compact screen geometry: {error}"),
            Self::Allocation { requested_bytes } => {
                write!(f, "screen cache could not reserve {requested_bytes} bytes")
            }
            Self::InvalidDrawPlan => f.write_str("screen cache and draw plan disagree"),
        }
    }
}
impl Error for DesktopScreenError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            Self::Prepared(error) => Some(error),
            Self::Compact(error) => Some(error),
            _ => None,
        }
    }
}

enum Resource {
    Empty,
    Prepared(PreparedScreenScene),
    Compact(ScreenPrimitiveBatch2d),
}

struct Entry {
    records: Vec<ResolvedScreenPrimitive>,
    resource: Resource,
}

#[derive(Default)]
struct Run {
    parts: Vec<Entry>,
}

/// One entry per *current* geometry run, never per historic entity or revision.
/// Admission is bounded by the aggregate ScreenScene budget and RenderLimits.
/// Replacement may retain old and new resource storage simultaneously. A failed
/// preparation does not present, but successful earlier updates are not rolled back.
#[derive(Default)]
pub(super) struct DesktopGeometry {
    generation: Option<WorldGeneration>,
    entries: Vec<Run>,
    scratch: Vec<ResolvedScreenPrimitive>,
    pub(super) updates: DesktopScreenUpdates,
}

impl DesktopGeometry {
    pub(super) fn clear(&mut self) {
        self.entries.clear();
        self.scratch.clear();
        self.generation = None;
    }
    pub(super) fn has_runs(&self) -> bool {
        !self.entries.is_empty()
    }

    pub(super) fn prepare(
        &mut self,
        renderer: &WgpuRenderer,
        snapshot: &ExtractedFrame,
        mode: DesktopScreenMode,
    ) -> Result<(), DesktopScreenError> {
        let started = Instant::now();
        self.updates = DesktopScreenUpdates::default();
        if mode == DesktopScreenMode::Streaming {
            self.clear();
            return Ok(());
        }
        if self.generation != Some(snapshot.world_generation()) {
            self.clear();
            self.generation = Some(snapshot.world_generation());
        }
        let count = snapshot
            .screen_draws()
            .iter()
            .filter(|draw| {
                matches!(
                    draw,
                    ScreenDraw::Rectangles { .. } | ScreenDraw::Primitives { .. }
                )
            })
            .count();
        self.entries.truncate(count);
        self.entries
            .try_reserve_exact(count.saturating_sub(self.entries.len()))
            .map_err(|_| DesktopScreenError::Allocation {
                requested_bytes: count.saturating_mul(size_of::<Run>()),
            })?;
        self.entries.resize_with(count, Run::default);
        for draw in snapshot.screen_draws() {
            let run = match *draw {
                ScreenDraw::Rectangles { run } | ScreenDraw::Primitives { run } => run,
                _ => continue,
            };
            let mut records = snapshot
                .screen_primitive_run_records(run)
                .ok_or(DesktopScreenError::InvalidDrawPlan)?
                .peekable();
            let scene = snapshot
                .screen_rectangle_run(run)
                .ok_or(DesktopScreenError::InvalidDrawPlan)?;
            let parts = &mut self
                .entries
                .get_mut(run)
                .ok_or(DesktopScreenError::InvalidDrawPlan)?
                .parts;
            let mut index = 0;
            while let Some(first) = records.peek() {
                let compact = mode == DesktopScreenMode::Compact && compact_candidate(first);
                self.scratch.clear();
                while records.peek().is_some_and(|record| {
                    mode != DesktopScreenMode::Compact
                        || (self.scratch.len() < 256 && compact_candidate(record) == compact)
                }) {
                    self.scratch
                        .try_reserve(1)
                        .map_err(|_| DesktopScreenError::Allocation {
                            requested_bytes: size_of::<ResolvedScreenPrimitive>(),
                        })?;
                    self.scratch
                        .push(records.next().ok_or(DesktopScreenError::InvalidDrawPlan)?);
                }
                prepare_part(
                    renderer,
                    parts,
                    index,
                    &self.scratch,
                    scene,
                    compact,
                    &mut self.updates,
                )?;
                index += 1;
            }
            parts.truncate(index);
        }
        // Scratch must not retain shared path snapshots from a retired run.
        self.scratch.clear();
        self.updates.retained_cpu_bytes = self
            .entries
            .iter()
            .flat_map(|run| &run.parts)
            .map(|entry| {
                let bytes = match &entry.resource {
                    Resource::Empty => 0,
                    Resource::Prepared(value) => value.recovery_memory_bytes(),
                    Resource::Compact(value) => value.retained_cpu_bytes(),
                };
                bytes.saturating_add(
                    entry
                        .records
                        .capacity()
                        .saturating_mul(size_of::<ResolvedScreenPrimitive>()),
                )
            })
            .fold(
                self.scratch
                    .capacity()
                    .saturating_mul(size_of::<ResolvedScreenPrimitive>()),
                usize::saturating_add,
            );
        self.updates.cpu_time = started.elapsed();
        Ok(())
    }

    pub(super) fn draw<'a>(
        &'a self,
        frame: &mut FrameComposer<'a>,
        run: usize,
        options: FramePassOptions,
    ) -> Result<(), FrameComposerError> {
        // Preparation checks run indices from this same immutable snapshot.
        for entry in &self.entries[run].parts {
            match &entry.resource {
                Resource::Empty => (),
                Resource::Prepared(value) => frame.draw_prepared_screen_scene(value, options)?,
                Resource::Compact(value) => frame.draw_screen_primitive_batch(value, options)?,
            }
        }
        Ok(())
    }
}

fn compact_candidate(record: &ResolvedScreenPrimitive) -> bool {
    match record {
        ResolvedScreenPrimitive::Rectangle(value) => {
            value.visual().corner_radius() == 0.0 && value.visual().stroke().is_none()
        }
        ResolvedScreenPrimitive::Line { .. } => true,
        ResolvedScreenPrimitive::Circle { visual, .. } => visual.stroke().is_none(),
        ResolvedScreenPrimitive::Polyline { .. } => false,
    }
}

fn prepare_part(
    renderer: &WgpuRenderer,
    parts: &mut Vec<Entry>,
    index: usize,
    records: &[ResolvedScreenPrimitive],
    source: &sim_engine::ScreenScene,
    compact: bool,
    updates: &mut DesktopScreenUpdates,
) -> Result<(), DesktopScreenError> {
    if parts.get(index).is_some_and(|old| old.records == records) {
        updates.reused_runs += 1;
        return Ok(());
    }
    parts
        .try_reserve_exact(index.saturating_add(1).saturating_sub(parts.len()))
        .map_err(|_| DesktopScreenError::Allocation {
            requested_bytes: size_of::<Entry>(),
        })?;
    let mut scene = sim_engine::ScreenScene::with_budget(
        sim_engine::Color::TRANSPARENT,
        source.budget().ok_or(DesktopScreenError::InvalidDrawPlan)?,
    )
    .map_err(DesktopScreenError::Scene)?;
    for record in records {
        record
            .append(&mut scene)
            .map_err(DesktopScreenError::Scene)?;
    }
    // Allocate comparison records fallibly before changing the retained resource.
    let mut captured = Vec::new();
    captured
        .try_reserve_exact(records.len())
        .map_err(|_| DesktopScreenError::Allocation {
            requested_bytes: records
                .len()
                .saturating_mul(size_of::<ResolvedScreenPrimitive>()),
        })?;
    for record in records {
        captured.push(record.clone());
    }
    let compact = if compact {
        match ScreenPrimitive2d::from_screen_scene(&scene) {
            Ok(values) => Some(values),
            // These only reject the optional compact representation.
            // The already validated ordinary scene remains authoritative.
            Err(
                ScreenPrimitiveError::UnsupportedCommand { .. }
                | ScreenPrimitiveError::PrecisionLimit
                | ScreenPrimitiveError::InvalidGeometry,
            ) => None,
            Err(error) => return Err(DesktopScreenError::Compact(error)),
        }
    } else {
        None
    };
    let resource = if scene.statistics().accepted_commands() == 0 {
        Resource::Empty
    } else if let Some(values) = compact {
        if let Some(Entry {
            records,
            resource: Resource::Compact(batch),
        }) = parts.get_mut(index)
        {
            // A growing run receives a new bound below. Fitting changes use
            // Engine's capacity-aware update and exact recovery snapshot.
            if values.len() <= batch.budget().max_primitives() {
                let report = renderer
                    .update_screen_primitive_batch(batch, &values)
                    .map_err(DesktopScreenError::Compact)?;
                updates.uploaded_bytes = updates
                    .uploaded_bytes
                    .saturating_add(report.uploaded_bytes());
                updates.updated_runs += 1;
                *records = captured;
                return Ok(());
            }
        }
        // Engine's compact per-item storage is smaller than the scene's
        // admitted tessellation. Use that same finite upload ceiling;
        // metadata has a separate finite, count-derived allowance.
        let bytes = scene.statistics().estimated_upload_bytes().max(256);
        let budget =
            ScreenPrimitiveBudget::new(values.len(), bytes.saturating_mul(2), bytes, bytes);
        let batch = renderer
            .create_screen_primitive_batch(&values, budget)
            .map_err(DesktopScreenError::Compact)?;
        updates.uploaded_bytes = updates
            .uploaded_bytes
            .saturating_add(batch.gpu_buffer_bytes());
        updates.compact_runs += 1;
        Resource::Compact(batch)
    } else {
        let prepared = renderer
            .prepare_screen_scene(&scene)
            .map_err(DesktopScreenError::Prepared)?;
        updates.uploaded_bytes = updates
            .uploaded_bytes
            .saturating_add(prepared.tessellation_stats().upload_bytes());
        updates.prepared_runs += 1;
        Resource::Prepared(prepared)
    };
    let entry = Entry {
        records: captured,
        resource,
    };
    if index < parts.len() {
        parts[index] = entry;
    } else if index == parts.len() {
        parts.push(entry);
    } else {
        return Err(DesktopScreenError::InvalidDrawPlan);
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
#[path = "geometry/tests.rs"]
mod tests;

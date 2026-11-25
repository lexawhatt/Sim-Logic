//! Bounded asynchronous GPU samples correlated to the logical frame that submitted them.

use crate::identity::WorldGeneration;

use sim_engine::{
    GpuTimingBatch, GpuTimingId, GpuTimingSample, GpuTimingSource, GpuTimingStatistics,
    WgpuRenderer,
};

const MAX_PENDING: usize = 32;
const MAX_COLLECTED: usize = 8;

/// Host role of an exactly correlated Engine pass. The Engine layered depth
/// compositor has no timing ID, so no measured sample is fabricated for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopGpuPassRole {
    /// Ordinary single-scene depth pass.
    Ordinary3d,
    /// Lower-resolution Base shading.
    BaseLow,
    /// Base coverage/depth (and shading) at Detail/output resolution.
    BaseVisibility,
    /// Detail shading at output resolution.
    Detail,
    /// Final window composition including World 2D and screen UI.
    WindowComposition,
}

/// One completed GPU pass with its originating logical frame and device generation.
///
/// Composition and retained 3D remain separate sources. This is hardware pass
/// time, not CPU work, frame latency, acquisition or confirmed monitor scanout.
#[derive(Debug, Clone, Copy)]
pub struct DesktopGpuTimingSample {
    logic_frame: u64,
    generation: u64,
    sample: GpuTimingSample,
    world: WorldGeneration,
    role: DesktopGpuPassRole,
}

impl DesktopGpuTimingSample {
    /// Returns the shared runner frame index that submitted this exact pass.
    pub const fn logic_frame_index(self) -> u64 {
        self.logic_frame
    }

    /// Returns zero for the initial device, incremented after each successful recovery.
    pub const fn device_generation(self) -> u64 {
        self.generation
    }

    /// World whose complete snapshot submitted this pass.
    pub const fn world_generation(self) -> WorldGeneration {
        self.world
    }

    /// Host pass role, matched by Engine source and timing ID.
    pub const fn role(self) -> DesktopGpuPassRole {
        self.role
    }

    /// Returns Engine's original source, opaque report ID and hardware interval.
    pub const fn sample(self) -> GpuTimingSample {
        self.sample
    }
}

/// Latest bounded GPU diagnostics for the current desktop renderer generation.
///
/// No timing is inferred for Disabled/Unavailable devices, skipped sampling,
/// failed readback or pending work. Statistics preserve Engine's explicit status
/// and loss counters. At most eight completed samples and thirty-two pending
/// report-to-frame associations are retained; no sample history grows with time.
/// Late or failed samples can outlive association capacity, in which case they
/// remain in the raw batch but are never attributed to a guessed logical frame.
///
/// Recovery clears old samples, associations and counters. This is diagnostic
/// state only; collection uses one nonblocking poll without waiting for the GPU.
#[derive(Debug, Default)]
pub struct DesktopGpuTimings {
    generation: u64,
    statistics: Option<GpuTimingStatistics>,
    last_batch: Option<GpuTimingBatch>,
    samples: [Option<DesktopGpuTimingSample>; MAX_COLLECTED],
    pending: PendingTimings<GpuTimingId>,
    unmatched_samples: u64,
    evicted_correlations: u64,
}

impl DesktopGpuTimings {
    /// Returns the current device generation, zero before and after initial creation.
    pub const fn device_generation(&self) -> u64 {
        self.generation
    }

    /// Returns Engine's latest current-device counters and explicit availability.
    /// `None` means no renderer was initialized, not zero measured GPU work.
    pub const fn statistics(&self) -> Option<GpuTimingStatistics> {
        self.statistics
    }

    /// Returns the latest nonempty collection, which can belong to earlier frames.
    ///
    /// IDs/source categories remain authoritative; batch order is not frame order.
    /// A subsequent empty collection preserves this batch. It is cleared during
    /// device recovery. Engine statistics still include every collection's CPU cost.
    pub const fn last_batch(&self) -> Option<GpuTimingBatch> {
        self.last_batch
    }

    /// Iterates samples in the latest nonempty batch with exact source-and-ID matches.
    /// An unmatched raw sample is omitted, not paired with the most recent frame.
    pub fn samples(&self) -> impl Iterator<Item = &DesktopGpuTimingSample> {
        self.samples.iter().filter_map(Option::as_ref)
    }

    /// Counts collected samples with no retained matching submission association.
    /// This is separate from Engine query/readback losses and resets on recovery.
    pub const fn unmatched_samples(&self) -> u64 {
        self.unmatched_samples
    }

    /// Counts associations replaced when the thirty-two-entry metadata limit was full.
    /// Failed readbacks can leave associations that never receive a sample.
    pub const fn evicted_correlations(&self) -> u64 {
        self.evicted_correlations
    }

    pub(super) fn reset(&mut self, generation: u64, statistics: GpuTimingStatistics) {
        self.reset_generation(generation);
        self.statistics = Some(statistics);
    }

    fn reset_generation(&mut self, generation: u64) {
        *self = Self {
            generation,
            ..Self::default()
        };
    }

    pub(super) fn submitted_pass(
        &mut self,
        logic_frame: u64,
        world: WorldGeneration,
        role: DesktopGpuPassRole,
        source: GpuTimingSource,
        id: Option<GpuTimingId>,
    ) {
        if let Some(id) = id
            && self.pending.insert_context(
                id,
                source,
                Correlation {
                    logic_frame,
                    world: Some(world),
                    role,
                },
            )
        {
            self.evicted_correlations = self.evicted_correlations.saturating_add(1);
        }
    }

    pub(super) fn collect(&mut self, renderer: &mut WgpuRenderer) {
        let batch = renderer.collect_gpu_timings();
        self.statistics = Some(renderer.gpu_timing_statistics());
        if batch.samples().is_empty() {
            return;
        }
        self.samples.fill(None);
        for (index, sample) in batch.samples().iter().copied().enumerate() {
            if let Some(context) = self.pending.take(sample.id(), sample.source())
                && let Some(world) = context.world
                && let Some(destination) = self.samples.get_mut(index)
            {
                *destination = Some(DesktopGpuTimingSample {
                    logic_frame: context.logic_frame,
                    world,
                    role: context.role,
                    generation: self.generation,
                    sample,
                });
            } else {
                self.unmatched_samples = self.unmatched_samples.saturating_add(1);
            }
        }
        self.last_batch = Some(batch);
    }
}

#[derive(Debug, Clone, Copy)]
struct PendingTiming<Id> {
    id: Id,
    source: GpuTimingSource,
    context: Correlation,
}
#[derive(Debug, Clone, Copy)]
struct Correlation {
    logic_frame: u64,
    world: Option<WorldGeneration>,
    role: DesktopGpuPassRole,
}

#[derive(Debug)]
struct PendingTimings<Id> {
    entries: [Option<PendingTiming<Id>>; MAX_PENDING],
}

impl<Id: Copy> Default for PendingTimings<Id> {
    fn default() -> Self {
        Self {
            entries: [None; MAX_PENDING],
        }
    }
}

impl<Id: Copy + Ord> PendingTimings<Id> {
    /// Returns whether a previous association had to be evicted.
    #[cfg(test)]
    fn insert(&mut self, id: Id, source: GpuTimingSource, logic_frame: u64) -> bool {
        self.insert_context(
            id,
            source,
            Correlation {
                logic_frame,
                world: None,
                role: DesktopGpuPassRole::Ordinary3d,
            },
        )
    }
    fn insert_context(&mut self, id: Id, source: GpuTimingSource, context: Correlation) -> bool {
        if self
            .entries
            .iter()
            .flatten()
            .any(|entry| entry.id == id && entry.source == source)
        {
            return false;
        }
        let vacant = self.entries.iter().position(Option::is_none);
        // Engine IDs are monotonic. Lost readbacks leave old associations;
        // evict the oldest one only after using the full bounded metadata pool.
        let index = vacant.or_else(|| {
            self.entries
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| entry.map(|entry| (index, entry.id)))
                .min_by_key(|(_, id)| *id)
                .map(|(index, _)| index)
        });
        let Some(index) = index else {
            return false;
        };
        self.entries[index] = Some(PendingTiming {
            id,
            source,
            context,
        });
        vacant.is_none()
    }

    fn take(&mut self, id: Id, source: GpuTimingSource) -> Option<Correlation> {
        self.entries
            .iter_mut()
            .find(|entry| entry.is_some_and(|entry| entry.id == id && entry.source == source))?
            .take()
            .map(|entry| entry.context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_requires_both_id_and_source_and_accepts_out_of_order_completion() {
        let mut pending = PendingTimings::<u64>::default();
        assert!(!pending.insert(10, GpuTimingSource::Scene3d, 100));
        assert!(!pending.insert(11, GpuTimingSource::FrameComposer, 100));
        assert!(!pending.insert(12, GpuTimingSource::Scene3d, 101));
        assert_eq!(
            pending
                .take(10, GpuTimingSource::FrameComposer)
                .map(|c| c.logic_frame),
            None
        );
        assert_eq!(
            pending
                .take(12, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            Some(101)
        );
        assert_eq!(
            pending
                .take(10, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            Some(100)
        );
        assert_eq!(
            pending
                .take(11, GpuTimingSource::FrameComposer)
                .map(|c| c.logic_frame),
            Some(100)
        );
        assert_eq!(
            pending
                .take(10, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            None
        );
    }

    #[test]
    fn lost_samples_have_bounded_metadata_and_cannot_reuse_an_evicted_frame() {
        let mut pending = PendingTimings::<u64>::default();
        for id in 1..=MAX_PENDING as u64 {
            assert!(!pending.insert(id, GpuTimingSource::Scene3d, id + 100));
        }
        assert!(pending.insert(33, GpuTimingSource::Scene3d, 117));
        assert_eq!(
            pending
                .take(1, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            None
        );
        assert_eq!(
            pending
                .take(33, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            Some(117)
        );
        assert!(!pending.insert(34, GpuTimingSource::Scene3d, 118));
        assert_eq!(
            pending
                .take(2, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            Some(102)
        );
    }

    #[test]
    fn repeated_submission_does_not_reassign_a_timing_id_to_another_frame() {
        let mut pending = PendingTimings::<u64>::default();
        assert!(!pending.insert(1, GpuTimingSource::Scene3d, 10));
        assert!(!pending.insert(1, GpuTimingSource::Scene3d, 11));
        assert_eq!(
            pending
                .take(1, GpuTimingSource::Scene3d)
                .map(|c| c.logic_frame),
            Some(10)
        );
    }

    #[test]
    fn layered_roles_keep_their_own_world_and_frame_out_of_order() {
        use crate::identity::ApplicationId;
        let world = WorldGeneration::new(ApplicationId::from_raw(7), 2);
        let next = WorldGeneration::new(ApplicationId::from_raw(7), 3);
        let mut pending = PendingTimings::<u64>::default();
        for (id, role) in [
            (1, DesktopGpuPassRole::BaseLow),
            (2, DesktopGpuPassRole::BaseVisibility),
            (3, DesktopGpuPassRole::Detail),
        ] {
            pending.insert_context(
                id,
                GpuTimingSource::Scene3d,
                Correlation {
                    logic_frame: 10,
                    world: Some(world),
                    role,
                },
            );
        }
        pending.insert_context(
            4,
            GpuTimingSource::Scene3d,
            Correlation {
                logic_frame: 11,
                world: Some(next),
                role: DesktopGpuPassRole::BaseLow,
            },
        );
        for (id, role) in [
            (3, DesktopGpuPassRole::Detail),
            (1, DesktopGpuPassRole::BaseLow),
            (2, DesktopGpuPassRole::BaseVisibility),
        ] {
            let context = pending.take(id, GpuTimingSource::Scene3d).unwrap();
            assert_eq!(context.role, role);
            assert_eq!(context.world, Some(world));
            assert_eq!(context.logic_frame, 10);
        }
        let context = pending.take(4, GpuTimingSource::Scene3d).unwrap();
        assert_eq!(context.world, Some(next));
        assert_eq!(context.logic_frame, 11);
    }

    #[test]
    fn uninitialized_diagnostics_have_no_fabricated_gpu_times() {
        let diagnostics = DesktopGpuTimings::default();
        assert!(diagnostics.statistics().is_none());
        assert!(diagnostics.last_batch().is_none());
        assert_eq!(diagnostics.samples().count(), 0);
        assert_eq!(diagnostics.unmatched_samples(), 0);
        assert_eq!(diagnostics.evicted_correlations(), 0);
    }

    #[test]
    fn recovery_generation_discards_old_diagnostic_counters() {
        let mut diagnostics = DesktopGpuTimings {
            generation: 1,
            unmatched_samples: 7,
            evicted_correlations: 9,
            ..DesktopGpuTimings::default()
        };
        diagnostics.reset_generation(2);
        assert_eq!(diagnostics.device_generation(), 2);
        assert_eq!(diagnostics.unmatched_samples(), 0);
        assert_eq!(diagnostics.evicted_correlations(), 0);
        assert!(diagnostics.last_batch().is_none());
        assert!(diagnostics.statistics().is_none());
        assert_eq!(diagnostics.samples().count(), 0);
    }
}

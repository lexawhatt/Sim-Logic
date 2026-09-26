//! Exact source proofs for reusing CPU screen scenes, never hashed guesses.
use super::*;

/// CPU screen extraction work, not tessellation, upload or GPU timing.
///
/// Each of the two atomic publication buffers retains its own proof. A warm
/// unchanged frame compares current sources but does not recollect/sort them
/// or reconstruct Engine scenes. Changed frames still validate all source
/// limits; geometry and mixed sub-runs can reuse their independent proofs.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ScreenExtractionUpdates {
    /// Sources compared with the previous accepted input sequence of this buffer.
    pub compared_sources: usize,
    /// The entire screen snapshot was unchanged; no sorting/scene reconstruction.
    pub reused_snapshot: bool,
    /// The aggregate Engine screen scene was reused, even if labels/images changed.
    pub reused_scene: bool,
    /// Mixed geometry sub-runs whose exact records allowed CPU scene reuse.
    pub reused_runs: usize,
    /// Mixed geometry sub-runs reconstructed during this extraction.
    pub rebuilt_runs: usize,
    /// Metadata capacity of source proofs, scratch and sub-run comparison records.
    /// Excludes shared fonts/paths/images and Engine scenes, which have separate
    /// owners. This counts one publication buffer, not both buffers combined.
    pub retained_key_bytes: usize,
}

impl ScreenSource {
    fn is_geometry(&self) -> bool {
        match self {
            Self::Rectangle(_) | Self::Line(..) | Self::Circle(..) | Self::Polyline(..) => true,
            Self::Image(_) => false,
            #[cfg(feature = "headless-text")]
            Self::Text(_) => false,
        }
    }
}

impl ScreenExtractionBuffer {
    pub(crate) fn extract(
        &mut self,
        generation: WorldGeneration,
        limits: RenderLimits,
        sources: impl IntoIterator<Item = ScreenSource>,
    ) -> Result<(), ExtractionError> {
        self.updates = ScreenExtractionUpdates::default();
        let same_scope =
            limits.screen_scene_reuse() && self.cache_scope == Some((generation, limits));
        let mut sources = sources.into_iter().fuse();
        let mut matched = 0;
        let mut first_changed = None;
        if same_scope {
            for previous in &self.source_keys {
                match sources.next() {
                    Some(current) => {
                        self.updates.compared_sources += 1;
                        if current == *previous {
                            matched += 1;
                        } else {
                            first_changed = Some(current);
                            break;
                        }
                    }
                    None => break,
                }
            }
            if matched == self.source_keys.len() {
                first_changed = sources.next();
                if first_changed.is_none() {
                    self.updates.reused_snapshot = true;
                    self.updates.reused_scene = true;
                    self.updates.reused_runs = self.runs.len();
                    self.update_key_bytes();
                    return Ok(());
                }
            }
        }
        // A failed build must never bless a partial source sequence as reusable.
        self.cache_scope = None;
        let mut previous = std::mem::take(&mut self.source_keys);
        self.source_keys = std::mem::take(&mut self.source_scratch);
        self.source_keys.clear();
        self.clear_sources();
        if !same_scope {
            // Invalidate proofs, but retain bounded ordinary scene capacity for
            // the uncached comparison route and generation changes alike.
            for run in &mut self.runs {
                run.records.clear();
            }
        }
        let result = (|| {
            // Replay only the equal prefix. There is no allocation on the fully
            // unchanged route or dependence on ECS tick age. Reordered source
            // iteration safely misses this proof, even if painter order is equal.
            self.collect_sources(
                generation,
                limits,
                previous[..matched]
                    .iter()
                    .cloned()
                    .chain(first_changed)
                    .chain(sources),
            )?;
            let same_geometry = same_scope
                && previous
                    .iter()
                    .filter(|key| key.is_geometry())
                    .eq(self.source_keys.iter().filter(|key| key.is_geometry()));
            if same_geometry {
                self.updates.reused_scene = true;
            } else {
                self.scene.clear();
                for index in 0..self.geometry_len() {
                    if let Some(primitive) = self.geometry(index) {
                        primitive
                            .append(&mut self.scene)
                            .map_err(ExtractionError::ScreenScene)?;
                    }
                }
            }
            self.compose(limits.screen_scene_budget())?;
            Ok(())
        })();
        // Keep bounded metadata capacity, but release old shared payloads now.
        previous.clear();
        self.source_scratch = previous;
        if result.is_ok() {
            self.cache_scope = Some((generation, limits));
        } else {
            self.clear();
        }
        self.update_key_bytes();
        result
    }

    pub(super) fn remember_source(&mut self, key: ScreenSource) -> Result<(), ExtractionError> {
        // Called only after the existing per-kind/aggregate source admission.
        self.source_keys
            .try_reserve(1)
            .map_err(|_| ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ScreenSource>(),
            })?;
        self.source_keys.push(key);
        Ok(())
    }

    fn update_key_bytes(&mut self) {
        self.updates.retained_key_bytes = self
            .source_keys
            .capacity()
            .saturating_add(self.source_scratch.capacity())
            .saturating_mul(size_of::<ScreenSource>())
            .saturating_add(
                self.runs
                    .iter()
                    .map(|run| {
                        run.records
                            .capacity()
                            .saturating_mul(size_of::<ResolvedScreenPrimitive>())
                    })
                    .fold(0usize, usize::saturating_add),
            );
    }
}

#[cfg(test)]
#[path = "screen_cache_tests.rs"]
mod tests;

//! Optional application-owned recovery from presentation budget exhaustion.
use crate::identity::WorldGeneration;
use sim_engine::SceneBudgetResource;

/// Work counters that can reject a composed frame before presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBudgetResource {
    /// An Engine CPU scene construction limit.
    Scene(SceneBudgetResource),
    /// Composed frame items/passes.
    Passes,
    /// Source commands.
    Commands,
    /// Submitted vertices.
    Vertices,
    /// Host-to-GPU upload bytes.
    UploadBytes,
    /// Referenced texture bytes.
    TextureBytes,
    /// Draw calls.
    DrawCalls,
}

/// The boundary that rejected one complete presentation attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationBudgetStage {
    /// Extraction of world-space geometry.
    WorldScene,
    /// Extraction of fixed-screen geometry.
    ScreenScene,
    /// Combined native frame preflight/submission.
    Composition,
}

/// Exact budget rejection available to the next application update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PresentationBudgetRejection {
    /// Logical frame that completed its application work but could not be drawn.
    pub frame_index: u64,
    /// World that produced the rejected attempt, not the previously visible World.
    pub generation: WorldGeneration,
    /// Rejected construction/composition boundary.
    pub stage: PresentationBudgetStage,
    /// Which allowance was exceeded.
    pub resource: PresentationBudgetResource,
    /// Configured inclusive ceiling.
    pub limit: usize,
    /// Required work at the failing boundary (not necessarily the whole scene).
    pub requested: usize,
}

/// Opt-in recoverable presentation budgets, independent of GPU dependencies.
///
/// Register this as an Application Resource to keep the stock desktop host alive
/// after typed scene/composition budget rejections. Read via `AppRes` next frame
/// to reduce presentation work, close a document, request another World or exit.
/// No limits are raised, no geometry is silently dropped and canonical updates
/// are not rolled back. Fixed and frame systems continue; pausing is app policy.
///
/// A rejected attempt is not presented. The last CPU snapshot survives failed
/// extraction, but the host never replays a stale-generation frame. The window
/// manager may retain the previous surface image; this is not guaranteed after
/// resize or loss. Allocation, invalid-geometry, system and invariant errors
/// remain errors. Without this optional resource, desktop stays fail-fast.
#[derive(Debug, Default)]
pub struct PresentationFeedback {
    last_rejection: Option<PresentationBudgetRejection>,
    rejected_frames: u64,
}

impl PresentationFeedback {
    /// Last rejection, cleared only after a confirmed native presentation.
    /// Headless operation never invents a native submission result.
    pub const fn last_rejection(&self) -> Option<PresentationBudgetRejection> {
        self.last_rejection
    }
    /// Saturating lifetime count, independent of temporary surface skips.
    pub const fn rejected_frames(&self) -> u64 {
        self.rejected_frames
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn reject(&mut self, rejection: PresentationBudgetRejection) {
        self.last_rejection = Some(rejection);
        self.rejected_frames = self.rejected_frames.saturating_add(1);
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn presented(&mut self) {
        self.last_rejection = None;
    }
}

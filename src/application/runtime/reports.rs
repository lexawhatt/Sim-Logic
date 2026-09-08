//! Public frame inputs, outcomes, lifecycle records, and error contracts.

use std::{error::Error, fmt, time::Duration};

use sim_engine::LogicalViewport;

use crate::{
    ExtractionError,
    commands::CommandBatchError,
    component::ComponentApprovalError,
    identity::{WorldFactoryId, WorldGeneration},
    input::{InputCollectionError, InputEvent},
    system::{Stage, SystemRunFailure, SystemSetupError},
    time::{DroppedFixedTime, FixedFramePlan, TimeAdvanceError},
    transition::{ConvergentTransitionWarning, TransitionRejection},
    world::WorldBuildError,
};

/// Immutable inputs for one shared core application frame.
pub struct FrameRequest<'a> {
    elapsed: Duration,
    events: &'a [InputEvent],
    viewport: LogicalViewport,
}

impl<'a> FrameRequest<'a> {
    /// Creates one frame request from wall time, physical events, and a
    /// validated logical viewport.
    pub const fn new(
        elapsed: Duration,
        events: &'a [InputEvent],
        viewport: LogicalViewport,
    ) -> Self {
        Self {
            elapsed,
            events,
            viewport,
        }
    }

    /// Returns the unscaled wall delta supplied to the frame.
    pub const fn elapsed(&self) -> Duration {
        self.elapsed
    }

    /// Returns the physical events supplied by the caller.
    ///
    /// The runner validates the complete slice against its frozen input limit
    /// before accepting the frame or changing logical input state.
    pub const fn events(&self) -> &'a [InputEvent] {
        self.events
    }

    /// Returns the validated logical viewport shared with a desktop adapter.
    pub const fn viewport(&self) -> LogicalViewport {
        self.viewport
    }
}

/// Failure before any World System or canonical time/input state was changed.
#[derive(Debug)]
pub enum BeginFrameRejection {
    /// The application-frame identity counter is exhausted.
    FrameIdentityExhausted,
    /// Physical input collection rejected the complete event batch.
    Input(InputCollectionError),
    /// Wall-time scaling or accumulation was not representable.
    Time(TimeAdvanceError),
}

impl fmt::Display for BeginFrameRejection {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrameIdentityExhausted => {
                formatter.write_str("application frame identity space is exhausted")
            }
            Self::Input(error) => write!(formatter, "frame input was rejected: {error}"),
            Self::Time(error) => write!(formatter, "frame time was rejected: {error}"),
        }
    }
}

impl Error for BeginFrameRejection {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::Time(error) => Some(error),
            Self::FrameIdentityExhausted => None,
        }
    }
}

/// Failure while preparing an isolated World candidate.
#[derive(Debug)]
pub enum CandidateFailure {
    /// The requested handle is not registered by this application.
    InvalidFactory,
    /// The next never-reused World generation cannot be issued.
    GenerationExhausted,
    /// The frozen component set could not be installed in the candidate.
    Components(ComponentApprovalError),
    /// The registered factory rejected candidate construction.
    Factory(WorldBuildError),
    /// A managed System used a forbidden Bevy access path.
    SystemSetup(SystemSetupError),
    /// Candidate Startup could not run.
    StartupSystem(SystemRunFailure),
    /// Candidate Startup's structural batch was rejected.
    StartupCommands(CommandBatchError),
    /// Candidate Startup requested another World before installation.
    StartupTransition,
    /// The resulting candidate was not a valid renderable World.
    Extraction(ExtractionError),
    /// A required private runtime Resource disappeared during preparation.
    RuntimeInvariant,
}

impl fmt::Display for CandidateFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFactory => formatter.write_str("World factory handle is not registered"),
            Self::GenerationExhausted => {
                formatter.write_str("World generation identity space is exhausted")
            }
            Self::Components(error) => write!(formatter, "component registry failed: {error}"),
            Self::Factory(error) => write!(formatter, "World factory failed: {error}"),
            Self::SystemSetup(error) => write!(formatter, "System setup failed: {error}"),
            Self::StartupSystem(error) => write!(formatter, "Startup failed: {error}"),
            Self::StartupCommands(error) => write!(formatter, "Startup Commands failed: {error}"),
            Self::StartupTransition => {
                formatter.write_str("Startup cannot request World replacement in this slice")
            }
            Self::Extraction(error) => write!(formatter, "candidate extraction failed: {error}"),
            Self::RuntimeInvariant => {
                formatter.write_str("candidate is missing a private runtime Resource")
            }
        }
    }
}

impl Error for CandidateFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Components(error) => Some(error),
            Self::Factory(error) => Some(error),
            Self::SystemSetup(error) => Some(error),
            Self::StartupSystem(error) => Some(error),
            Self::StartupCommands(error) => Some(error),
            Self::Extraction(error) => Some(error),
            Self::InvalidFactory
            | Self::GenerationExhausted
            | Self::StartupTransition
            | Self::RuntimeInvariant => None,
        }
    }
}

/// Failure to freeze an Application into a runnable headless core.
#[derive(Debug)]
pub enum RunnerBuildError {
    /// The initial factory handle is foreign, stale, or missing.
    InvalidInitialFactory,
    /// Input configuration could not initialize.
    Input(InputCollectionError),
    /// The bounded lifecycle trace could not reserve its frozen capacity.
    LifecycleTraceAllocation {
        /// Requested record capacity.
        limit: usize,
    },
    /// A FixedUpdate or FrameUpdate System requests a type that was not
    /// registered on the Application builder.
    MissingApplicationResource {
        /// Rust type name used only for diagnostics.
        resource: &'static str,
        /// Stage containing the first registered requirement.
        stage: Stage,
        /// Rust System type name used only for diagnostics.
        system: &'static str,
    },
    /// Private Application Resource staging failed an internal invariant.
    ApplicationResourceInvariant,
    /// Initial World preparation failed before an active World existed.
    InitialWorld(CandidateFailure),
}

impl fmt::Display for RunnerBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInitialFactory => {
                formatter.write_str("initial World factory is not registered by this application")
            }
            Self::Input(error) => write!(formatter, "input initialization failed: {error}"),
            Self::LifecycleTraceAllocation { limit } => write!(
                formatter,
                "failed to reserve lifecycle trace capacity of {limit} records"
            ),
            Self::MissingApplicationResource {
                resource,
                stage,
                system,
            } => write!(
                formatter,
                "{stage:?} System `{system}` requests unregistered Application Resource `{resource}`"
            ),
            Self::ApplicationResourceInvariant => formatter
                .write_str("Application Resource staging failed a private runtime invariant"),
            Self::InitialWorld(error) => write!(formatter, "initial World failed: {error}"),
        }
    }
}

impl Error for RunnerBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Input(error) => Some(error),
            Self::InitialWorld(error) => Some(error),
            Self::InvalidInitialFactory
            | Self::LifecycleTraceAllocation { .. }
            | Self::MissingApplicationResource { .. }
            | Self::ApplicationResourceInvariant => None,
        }
    }
}

/// An observable World lifecycle boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleEvent {
    /// A validated candidate became the active World.
    WorldEnter,
    /// An active World was logically retired before destruction.
    WorldExit,
}

/// One application-owned lifecycle trace record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LifecycleRecord {
    pub(super) event: LifecycleEvent,
    pub(super) generation: WorldGeneration,
    pub(super) factory: WorldFactoryId,
}

impl LifecycleRecord {
    /// Returns the recorded lifecycle boundary.
    pub const fn event(self) -> LifecycleEvent {
        self.event
    }

    /// Returns the World generation involved in the boundary.
    pub const fn generation(self) -> WorldGeneration {
        self.generation
    }

    /// Returns the factory that constructed the generation.
    pub const fn factory(self) -> WorldFactoryId {
        self.factory
    }
}

/// Invalid opaque data in a transition request batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionRequestFailure {
    /// The target factory is foreign, stale, or absent.
    InvalidFactory,
    /// The intent token is foreign, stale, already consumed, or not live in
    /// this delivery stage.
    InvalidIntent,
}

/// Transition result for one completed application frame.
#[derive(Debug)]
pub enum FrameTransition {
    /// No replacement was processed. This also applies when application exit
    /// suppressed replacement requests in the same committed command batch.
    None,
    /// Valid requests failed deterministic arbitration.
    Rejected(TransitionRejection),
    /// A request contained a stale or foreign opaque handle.
    Invalid(TransitionRequestFailure),
    /// The selected target failed while still isolated; the old World remains
    /// active and the reserved generation is burned.
    PreparationFailed {
        /// Selected target factory.
        target: WorldFactoryId,
        /// Exact preparation failure.
        error: CandidateFailure,
    },
    /// A validated candidate replaced the old World irreversibly.
    Committed {
        /// Retired generation.
        old: WorldGeneration,
        /// Installed generation.
        new: WorldGeneration,
        /// Factory used to construct `new`.
        target: WorldFactoryId,
        /// Diagnostic when independent intents converged on `target`.
        warning: Option<ConvergentTransitionWarning>,
    },
}

/// Failure after at least part of an application frame may have changed the
/// active World or consumed fixed time.
#[derive(Debug)]
pub enum FrameFailure {
    /// A FixedUpdate or FrameUpdate System could not run.
    System {
        /// Stage that stopped.
        stage: Stage,
        /// Exact managed System failure.
        error: SystemRunFailure,
    },
    /// A whole structural command batch was rejected.
    Commands {
        /// Stage whose batch was discarded.
        stage: Stage,
        /// Exact batch failure.
        error: CommandBatchError,
    },
    /// Fixed-time cleanup could not be represented.
    Time(TimeAdvanceError),
    /// CPU extraction rejected the new snapshot atomically.
    Extraction(ExtractionError),
    /// A required private runtime Resource disappeared.
    RuntimeInvariant,
}

impl fmt::Display for FrameFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::System { stage, error } => {
                write!(formatter, "{stage:?} System failed: {error}")
            }
            Self::Commands { stage, error } => {
                write!(formatter, "{stage:?} Commands failed: {error}")
            }
            Self::Time(error) => write!(formatter, "fixed-time cleanup failed: {error}"),
            Self::Extraction(error) => write!(formatter, "render extraction failed: {error}"),
            Self::RuntimeInvariant => {
                formatter.write_str("a private frame runtime invariant was violated")
            }
        }
    }
}

impl Error for FrameFailure {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::System { error, .. } => Some(error),
            Self::Commands { error, .. } => Some(error),
            Self::Time(error) => Some(error),
            Self::Extraction(error) => Some(error),
            Self::RuntimeInvariant => None,
        }
    }
}

/// Diagnostics for a completed application frame.
#[derive(Debug)]
pub struct LogicFrameReport {
    pub(super) frame_index: u64,
    pub(super) timing: FixedFramePlan,
    pub(super) fixed_ticks_attempted: u32,
    pub(super) spawned: usize,
    pub(super) despawned: usize,
    pub(super) exit_requested: bool,
    pub(super) transitions_suppressed_by_exit: usize,
    pub(super) dropped_for_transition: Option<DroppedFixedTime>,
    pub(super) transition: FrameTransition,
    pub(super) extracted_generation: Option<WorldGeneration>,
    pub(super) failure: Option<FrameFailure>,
}

impl LogicFrameReport {
    /// Returns the accepted application-frame index.
    pub const fn frame_index(&self) -> u64 {
        self.frame_index
    }

    /// Returns bounded fixed-work planning and catch-up drops.
    pub const fn timing(&self) -> FixedFramePlan {
        self.timing
    }

    /// Returns the number of fixed ticks that began, including a failed tick.
    pub const fn fixed_ticks_attempted(&self) -> u32 {
        self.fixed_ticks_attempted
    }

    /// Returns managed entities spawned by committed stage batches.
    pub const fn spawned(&self) -> usize {
        self.spawned
    }

    /// Returns managed entities despawned by committed stage batches.
    pub const fn despawned(&self) -> usize {
        self.despawned
    }

    /// Returns whether a committed stage batch requested application exit.
    ///
    /// Desktop honors this by ending its event loop without presenting another
    /// frame. A headless host decides when to stop its own driving loop.
    pub const fn exit_requested(&self) -> bool {
        self.exit_requested
    }

    /// Returns replacement requests ignored because exit took precedence.
    pub const fn transitions_suppressed_by_exit(&self) -> usize {
        self.transitions_suppressed_by_exit
    }

    /// Returns whole ticks discarded because a stopped transition was rejected.
    pub const fn dropped_for_transition(&self) -> Option<DroppedFixedTime> {
        self.dropped_for_transition
    }

    /// Returns the frame's transition result.
    pub const fn transition(&self) -> &FrameTransition {
        &self.transition
    }

    /// Returns the generation published by successful CPU extraction.
    pub const fn extracted_generation(&self) -> Option<WorldGeneration> {
        self.extracted_generation
    }

    /// Returns a post-begin failure, if the frame stopped early or extraction
    /// was rejected.
    pub const fn failure(&self) -> Option<&FrameFailure> {
        self.failure.as_ref()
    }
}

/// Result of one call to [`HeadlessRunner::advance_frame`](crate::headless::HeadlessRunner::advance_frame).
#[derive(Debug)]
#[expect(
    clippy::large_enum_variant,
    reason = "keeping the hot-path frame report inline avoids one allocation per frame"
)]
pub enum FrameOutcome {
    /// BeginFrame rejected all inputs before canonical runtime state changed.
    Rejected(BeginFrameRejection),
    /// BeginFrame succeeded; inspect the report because later failures do not
    /// imply rollback.
    Advanced(LogicFrameReport),
}

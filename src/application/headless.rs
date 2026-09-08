//! Shared headless application-frame driver.

use std::{error::Error, fmt, time::Duration};

use bevy_ecs::{
    entity::Entity,
    entity_disabling::Disabled,
    prelude::Mut,
    query::{Allow, QueryState, Without},
    world::World,
};
use sim_engine::LogicalViewport;

use crate::{
    ExtractedFrame, ExtractionError,
    app::{Application, RegisteredWorldFactory},
    commands::{
        CommandBatchError, CommandCommitError, CommandQueue, CommittedCommands, DirectIntentIssuer,
    },
    component::{ApprovedComponents, ComponentApprovalError, ComponentRegistry},
    events::{EventRegistry, begin_event_stage, end_event_stage},
    extraction::{
        CircleSource, ExtractionBuffers, ExtractionParameters, LineSource, RectangleSource,
        ScreenRectangleSource,
    },
    identity::{
        ApplicationId, IntentKind, LogicEntity, ManagedEntity, TransitionIntentToken,
        WorldFactoryId, WorldGeneration, WorldIdentityState,
    },
    input::{
        Action, FixedInputState, FrameInputState, InputCollectionError, InputEvent, InputState,
    },
    query::QueryEntityError,
    render::{FrameViewportState, RenderLimits},
    resources::ApplicationResourceRegistry,
    screen::ScreenRectangleVisual,
    system::{SequentialStage, Stage, StageFactories, SystemRunFailure, SystemSetupError},
    time::{
        DroppedFixedTime, FixedFramePlan, FixedTickError, FixedTimeState, FrameTimeState,
        TimeAdvanceError, TimeConfigurationError, TimeState,
    },
    transition::{
        ConvergentTransitionWarning, TransitionArbitration, TransitionRejection, TransitionRequest,
        arbitrate,
    },
    visual::{
        ActiveCamera2d, CircleVisual, LineVisual, RectangleVisual, Transform2d, WorldBackground,
    },
    world::{WorldBuildError, WorldBuilder},
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
    event: LifecycleEvent,
    generation: WorldGeneration,
    factory: WorldFactoryId,
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
    frame_index: u64,
    timing: FixedFramePlan,
    fixed_ticks_attempted: u32,
    spawned: usize,
    despawned: usize,
    exit_requested: bool,
    transitions_suppressed_by_exit: usize,
    dropped_for_transition: Option<DroppedFixedTime>,
    transition: FrameTransition,
    extracted_generation: Option<WorldGeneration>,
    failure: Option<FrameFailure>,
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

/// Result of one call to [`HeadlessRunner::advance_frame`].
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

struct RuntimeWorld {
    factory: WorldFactoryId,
    generation: WorldGeneration,
    world: World,
    approved: ApprovedComponents,
    managed_entities: usize,
    previous_translations: Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
    previous_camera_centers: Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
    interpolation_queries: InterpolationQueries,
    extraction_queries: ExtractionQueries,
    fixed: SequentialStage,
    frame: SequentialStage,
}

type CameraExtractionQuery =
    QueryState<&'static ActiveCamera2d, (Allow<Disabled>, Without<Disabled>)>;
type CircleExtractionQuery = QueryState<
    (
        Entity,
        &'static ManagedEntity,
        &'static Transform2d,
        &'static CircleVisual,
    ),
    (Allow<Disabled>, Without<Disabled>),
>;
type RectangleExtractionQuery = QueryState<
    (
        Entity,
        &'static ManagedEntity,
        &'static Transform2d,
        &'static RectangleVisual,
    ),
    (Allow<Disabled>, Without<Disabled>),
>;
type LineExtractionQuery = QueryState<
    (
        Entity,
        &'static ManagedEntity,
        &'static Transform2d,
        &'static LineVisual,
    ),
    (Allow<Disabled>, Without<Disabled>),
>;
type CameraInterpolationQuery =
    QueryState<(Entity, &'static ManagedEntity, &'static ActiveCamera2d), Allow<Disabled>>;
type ScreenRectangleExtractionQuery = QueryState<
    (
        Entity,
        &'static ManagedEntity,
        &'static ScreenRectangleVisual,
    ),
    (Allow<Disabled>, Without<Disabled>),
>;
type TransformInterpolationQuery =
    QueryState<(Entity, &'static ManagedEntity, &'static mut Transform2d), Allow<Disabled>>;
type CameraSnapQuery = QueryState<&'static mut ActiveCamera2d, Allow<Disabled>>;

struct InterpolationQueries {
    cameras: CameraInterpolationQuery,
    transforms: TransformInterpolationQuery,
    snap_cameras: CameraSnapQuery,
}

impl InterpolationQueries {
    fn new(world: &mut World) -> Self {
        Self {
            cameras: world.query_filtered(),
            transforms: world.query_filtered(),
            snap_cameras: world.query_filtered(),
        }
    }
}

struct ExtractionQueries {
    cameras: CameraExtractionQuery,
    circles: CircleExtractionQuery,
    rectangles: RectangleExtractionQuery,
    lines: LineExtractionQuery,
    screen_rectangles: ScreenRectangleExtractionQuery,
}

impl ExtractionQueries {
    fn new(world: &mut World) -> Self {
        Self {
            cameras: world.query_filtered(),
            circles: world.query_filtered(),
            rectangles: world.query_filtered(),
            lines: world.query_filtered(),
            screen_rectangles: world.query_filtered(),
        }
    }
}

enum StageExecutionFailure {
    System(SystemRunFailure),
    RuntimeInvariant,
}

fn run_stage(stage: &mut SequentialStage, world: &mut World) -> Result<(), StageExecutionFailure> {
    if !stage.writes_events() {
        return stage.run(world).map_err(StageExecutionFailure::System);
    }
    if !begin_event_stage(world) {
        return Err(StageExecutionFailure::RuntimeInvariant);
    }

    let result = stage.run(world);
    let events_ended = end_event_stage(world);
    match result {
        Err(error) => Err(StageExecutionFailure::System(error)),
        Ok(()) if !events_ended => Err(StageExecutionFailure::RuntimeInvariant),
        Ok(()) => Ok(()),
    }
}

/// Renderer-independent Sim;Logic runner used by tests and desktop adapters.
pub struct HeadlessRunner<A: Action> {
    application: ApplicationId,
    config: crate::app::AppConfig,
    components: ComponentRegistry,
    events: EventRegistry,
    application_resources: ApplicationResourceRegistry,
    factories: Vec<RegisteredWorldFactory>,
    startup_factories: StageFactories,
    fixed_factories: StageFactories,
    frame_factories: StageFactories,
    active: RuntimeWorld,
    input: InputState<A>,
    time: TimeState,
    frame_index: u64,
    next_generation: u64,
    extraction: ExtractionBuffers,
    lifecycle: Vec<LifecycleRecord>,
    dropped_lifecycle_records: u64,
}

impl<A: Action> HeadlessRunner<A> {
    pub(crate) fn from_application(
        application: Application<A>,
        initial: WorldFactoryId,
    ) -> Result<Self, RunnerBuildError> {
        if !factory_exists(application.application, &application.factories, initial) {
            return Err(RunnerBuildError::InvalidInitialFactory);
        }
        if let Some(system) = application.startup.first_application_resource_system() {
            return Err(RunnerBuildError::InitialWorld(
                CandidateFailure::SystemSetup(SystemSetupError::StartupApplicationResourceAccess {
                    system,
                }),
            ));
        }
        if let Some((resource, stage, system)) = application
            .application_resources
            .first_missing_requirement()
        {
            return Err(RunnerBuildError::MissingApplicationResource {
                resource,
                stage,
                system,
            });
        }

        let input = InputState::new(application.bindings, application.config.input_event_limit())
            .map_err(RunnerBuildError::Input)?;
        let mut lifecycle = Vec::new();
        lifecycle
            .try_reserve(application.config.lifecycle_trace_limit())
            .map_err(|_| RunnerBuildError::LifecycleTraceAllocation {
                limit: application.config.lifecycle_trace_limit(),
            })?;
        // `prepare_world` needs the registries already owned by a runner. Keep
        // one internally consistent, never-observable World in this slot until
        // initial candidate preparation succeeds.
        let placeholder_generation = WorldGeneration::new(application.application, 0);
        let mut placeholder_world = World::new();
        let placeholder_approved = ComponentRegistry::default()
            .install(&mut placeholder_world)
            .map_err(|error| RunnerBuildError::InitialWorld(CandidateFailure::Components(error)))?;
        let placeholder_fixed = StageFactories::default()
            .instantiate(&mut placeholder_world, Stage::FixedUpdate)
            .map_err(|error| {
                RunnerBuildError::InitialWorld(CandidateFailure::SystemSetup(error))
            })?;
        let placeholder_frame = StageFactories::default()
            .instantiate(&mut placeholder_world, Stage::FrameUpdate)
            .map_err(|error| {
                RunnerBuildError::InitialWorld(CandidateFailure::SystemSetup(error))
            })?;
        let placeholder_extraction_queries = ExtractionQueries::new(&mut placeholder_world);
        let placeholder_interpolation_queries = InterpolationQueries::new(&mut placeholder_world);
        let mut runner = Self {
            application: application.application,
            config: application.config,
            components: application.components,
            events: application.events,
            application_resources: application.application_resources,
            factories: application.factories,
            startup_factories: application.startup,
            fixed_factories: application.fixed,
            frame_factories: application.frame,
            active: RuntimeWorld {
                factory: initial,
                generation: placeholder_generation,
                world: placeholder_world,
                approved: placeholder_approved,
                managed_entities: 0,
                previous_translations: Vec::new(),
                previous_camera_centers: Vec::new(),
                interpolation_queries: placeholder_interpolation_queries,
                extraction_queries: placeholder_extraction_queries,
                fixed: placeholder_fixed,
                frame: placeholder_frame,
            },
            input,
            time: TimeState::new(application.config.time()),
            frame_index: 0,
            next_generation: 1,
            extraction: ExtractionBuffers::new(),
            lifecycle,
            dropped_lifecycle_records: 0,
        };

        let mut world = runner
            .prepare_world(initial)
            .map_err(RunnerBuildError::InitialWorld)?;
        if !runner
            .application_resources
            .install_initial(&mut world.world)
        {
            return Err(RunnerBuildError::ApplicationResourceInvariant);
        }
        runner.active = world;
        if runner.extraction.publish() != Some(runner.active.generation) {
            return Err(RunnerBuildError::InitialWorld(
                CandidateFailure::RuntimeInvariant,
            ));
        }
        runner.record_lifecycle(
            LifecycleEvent::WorldEnter,
            runner.active.generation,
            initial,
        );
        Ok(runner)
    }

    /// Advances the same BeginFrame-to-EndFrame lifecycle used by the desktop
    /// adapter, without GPU preparation or presentation.
    pub fn advance_frame(&mut self, request: FrameRequest<'_>) -> FrameOutcome {
        let Some(next_frame) = self.frame_index.checked_add(1) else {
            return FrameOutcome::Rejected(BeginFrameRejection::FrameIdentityExhausted);
        };

        let mut planned_time = self.time.clone();
        let timing = match planned_time.plan_frame(request.elapsed) {
            Ok(timing) => timing,
            Err(error) => return FrameOutcome::Rejected(BeginFrameRejection::Time(error)),
        };
        if let Err(error) = self.input.collect_frame(
            request.events,
            self.time.is_paused(),
            self.application,
            self.active.generation,
            self.frame_index,
        ) {
            return FrameOutcome::Rejected(BeginFrameRejection::Input(error));
        }
        self.time = planned_time;

        let mut report = LogicFrameReport {
            frame_index: self.frame_index,
            timing,
            fixed_ticks_attempted: 0,
            spawned: 0,
            despawned: 0,
            exit_requested: false,
            transitions_suppressed_by_exit: 0,
            dropped_for_transition: None,
            transition: FrameTransition::None,
            extracted_generation: None,
            failure: None,
        };
        if !self.update_common_resources() {
            report.failure = Some(FrameFailure::RuntimeInvariant);
        }

        let mut force_zero_extraction_alpha = false;
        let mut fixed_transition = None;
        for tick_number in 0..timing.ticks_to_attempt() {
            if report.failure.is_some() {
                break;
            }
            let fixed_time = match self.time.begin_fixed_tick() {
                Ok(time) => time,
                Err(error) => {
                    report.failure = Some(fixed_tick_failure(error));
                    break;
                }
            };
            report.fixed_ticks_attempted += 1;
            if self.active.fixed.is_empty() {
                self.active.previous_translations.clear();
                self.active.previous_camera_centers.clear();
            } else if !capture_and_begin_fixed_interpolation(
                &mut self.active.world,
                &mut self.active.interpolation_queries,
                &mut self.active.previous_translations,
                &mut self.active.previous_camera_centers,
            ) {
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }
            self.active.world.insert_resource(fixed_time);
            if !copy_fixed_snapshot(&mut self.active.world, &self.input) {
                restore_previous_translations(
                    &mut self.active.world,
                    &self.active.previous_translations,
                );
                restore_previous_camera_centers(
                    &mut self.active.world,
                    &self.active.previous_camera_centers,
                );
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }

            if let Err(error) = run_stage(&mut self.active.fixed, &mut self.active.world) {
                restore_previous_translations(
                    &mut self.active.world,
                    &self.active.previous_translations,
                );
                restore_previous_camera_centers(
                    &mut self.active.world,
                    &self.active.previous_camera_centers,
                );
                let commands_intact = self.consume_failed_stage_tokens();
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(if commands_intact {
                    match error {
                        StageExecutionFailure::System(error) => FrameFailure::System {
                            stage: Stage::FixedUpdate,
                            error,
                        },
                        StageExecutionFailure::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                    }
                } else {
                    FrameFailure::RuntimeInvariant
                });
                break;
            }

            let (failed_tokens, committed) = match apply_command_queue(
                &mut self.active.world,
                &mut self.active.approved,
                self.application,
                self.active.generation,
                self.active.managed_entities,
                self.config.entity_limit(),
            ) {
                Some(outcome) => outcome,
                None => {
                    restore_previous_translations(
                        &mut self.active.world,
                        &self.active.previous_translations,
                    );
                    restore_previous_camera_centers(
                        &mut self.active.world,
                        &self.active.previous_camera_centers,
                    );
                    self.input.consume_fixed_delivery();
                    self.time.stop_remaining_ticks();
                    report.failure = Some(FrameFailure::RuntimeInvariant);
                    break;
                }
            };
            let committed = match committed {
                Ok(committed) => committed,
                Err(error) => {
                    restore_previous_translations(
                        &mut self.active.world,
                        &self.active.previous_translations,
                    );
                    restore_previous_camera_centers(
                        &mut self.active.world,
                        &self.active.previous_camera_centers,
                    );
                    self.consume_tokens(&failed_tokens);
                    self.input.consume_fixed_delivery();
                    self.time.stop_remaining_ticks();
                    report.failure = Some(match error {
                        CommandCommitError::Batch(error) => FrameFailure::Commands {
                            stage: Stage::FixedUpdate,
                            error,
                        },
                        CommandCommitError::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                    });
                    break;
                }
            };
            self.active.managed_entities = committed.live_entities;
            add_committed_counts(&mut report, &committed);
            let pause = committed.pause;

            if committed.exit_requested {
                report.exit_requested = true;
                report.transitions_suppressed_by_exit = committed.transitions.len();
                self.input.clear_world_edges();
                snap_runtime_interpolation(&mut self.active);
                self.time.clear_accumulator();
                if !clear_fixed_snapshot::<A>(&mut self.active.world) {
                    report.failure = Some(FrameFailure::RuntimeInvariant);
                }
                break;
            }

            if !committed.transitions.is_empty() {
                fixed_transition = Some((committed.transitions, pause));
                self.time.stop_remaining_ticks();
                break;
            }

            self.input.consume_fixed_delivery();
            force_zero_extraction_alpha |= self.apply_committed_pause(pause);
            if self.time.is_paused() {
                break;
            }

            if tick_number + 1 == timing.ticks_to_attempt()
                && !clear_fixed_snapshot::<A>(&mut self.active.world)
            {
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }
        }

        if report.failure.is_none() && !report.exit_requested {
            if let Some((requests, pause)) = fixed_transition {
                self.resolve_transition(requests, true, &mut report);
                // Transition validation must happen while input-backed intent
                // tokens are still live. Once arbitration is complete, every
                // edge delivered to this fixed tick is consumed together.
                self.input.consume_fixed_delivery();
                force_zero_extraction_alpha |= self.apply_committed_pause(pause);
            } else {
                self.run_frame_update(
                    request.elapsed,
                    request.viewport,
                    &mut report,
                    &mut force_zero_extraction_alpha,
                );
            }
        }

        if report.failure.is_none()
            && !report.exit_requested
            && report.extracted_generation.is_none()
        {
            let alpha = if force_zero_extraction_alpha {
                Some(0.0)
            } else {
                self.time.interpolation_alpha()
            };
            match alpha {
                Some(alpha) => match stage_runtime_world(
                    &mut self.active,
                    alpha as f32,
                    self.config.render(),
                    &mut self.extraction,
                ) {
                    Ok(()) => {
                        report.extracted_generation = self.extraction.publish();
                        if report.extracted_generation.is_none() {
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                        }
                    }
                    Err(error) => report.failure = Some(FrameFailure::Extraction(error)),
                },
                None => report.failure = Some(FrameFailure::RuntimeInvariant),
            }
        }

        self.input.end_frame();
        if let Some(mut issuer) = self.active.world.get_resource_mut::<DirectIntentIssuer>() {
            issuer.disable();
        }
        // `bevy_ecs` is used without `bevy_app`, so its per-frame change and
        // removal trackers are our responsibility. Advancing them here keeps
        // removed temporary resources bounded and gives the next application
        // frame the same change-detection boundary as a Bevy App update.
        self.active.world.clear_trackers();
        self.frame_index = next_frame;
        FrameOutcome::Advanced(report)
    }

    /// Returns the last atomically published CPU snapshot.
    ///
    /// The value may belong to a retired generation and is then diagnostic
    /// history only. A renderer must compare its generation immediately before
    /// submission.
    pub const fn extracted_frame(&self) -> Option<&ExtractedFrame> {
        self.extraction.published()
    }

    /// Returns the currently active World generation.
    pub const fn world_generation(&self) -> WorldGeneration {
        self.active.generation
    }

    /// Returns application-owned WorldEnter/WorldExit trace records.
    pub fn lifecycle(&self) -> &[LifecycleRecord] {
        &self.lifecycle
    }

    /// Returns records evicted from the bounded lifecycle trace.
    pub const fn dropped_lifecycle_records(&self) -> u64 {
        self.dropped_lifecycle_records
    }

    /// Resolves a factory handle to its bounded human-readable route name.
    ///
    /// The name is diagnostic metadata only; transition identity continues to
    /// use the opaque [`WorldFactoryId`]. Foreign or stale handles return
    /// `None`.
    pub fn factory_name(&self, factory: WorldFactoryId) -> Option<&str> {
        self.factories
            .iter()
            .find(|registered| registered.id == factory)
            .map(|registered| registered.diagnostic_name.as_str())
    }

    /// Returns the diagnostic route name of the active World.
    pub fn active_world_name(&self) -> &str {
        self.factory_name(self.active.factory)
            .unwrap_or("<runtime-invariant-violation>")
    }

    /// Reads one typed Resource without exposing the raw Bevy World.
    pub fn resource<R: bevy_ecs::prelude::Resource>(&self) -> Option<&R> {
        self.active.world.get_resource::<R>()
    }

    /// Reads one Application Resource without exposing its private ECS slot.
    ///
    /// Unlike [`Self::resource`], this value survives successful World
    /// replacement. Inspection is read-only so tests cannot bypass normal
    /// System ordering and mutation rules.
    pub fn app_resource<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.application_resources.get::<T>(&self.active.world)
    }

    /// Iterates over one component type for read-only headless inspection.
    ///
    /// Results include only managed logical entities, including entities with
    /// the standard Bevy [`Disabled`] component. Their order is not a
    /// deterministic API; tests with order-dependent assertions must sort by
    /// their own stable application data.
    pub fn components<T: bevy_ecs::prelude::Component>(
        &self,
    ) -> impl Iterator<Item = (crate::identity::LogicEntity, &T)> {
        self.active.world.iter_entities().filter_map(|entity| {
            let identity = entity.get::<ManagedEntity>()?.handle(entity.id());
            let component = entity.get::<T>()?;
            Some((identity, component))
        })
    }

    /// Reads one component from a managed entity in the active World.
    ///
    /// Application and World-generation provenance are checked before the raw
    /// ECS slot is touched, and the private runtime identity in that slot must
    /// still equal `entity`. Disabled managed entities remain inspectable here,
    /// matching [`Self::components`]. A missing entity or component returns
    /// [`QueryEntityError::DoesNotMatch`].
    pub fn component<T: bevy_ecs::prelude::Component>(
        &self,
        entity: LogicEntity,
    ) -> Result<&T, QueryEntityError> {
        if entity.application() != self.application
            || entity.world_generation() != self.active.generation
        {
            return Err(QueryEntityError::ForeignWorld {
                entity,
                active: self.active.generation,
            });
        }

        let managed_matches = self
            .active
            .world
            .get::<ManagedEntity>(entity.entity())
            .is_some_and(|managed| managed.handle(entity.entity()) == entity);
        if !managed_matches {
            return Err(QueryEntityError::DoesNotMatch { entity });
        }
        self.active
            .world
            .get::<T>(entity.entity())
            .ok_or(QueryEntityError::DoesNotMatch { entity })
    }

    /// Pauses or resumes fixed simulation. FrameUpdate and FrameInput continue.
    ///
    /// Entering pause snaps visual interpolation to current canonical entity
    /// translations and camera centers, and discards undelivered fixed-input
    /// edges. Accumulated fixed time is preserved and becomes eligible again
    /// after resume; wall time supplied while paused is not accumulated.
    pub fn set_paused(&mut self, paused: bool) {
        let entering_pause = paused && !self.time.is_paused();
        self.time.set_paused(paused);
        if entering_pause {
            self.input.discard_fixed_edges();
            snap_runtime_interpolation(&mut self.active);
        }
    }

    /// Returns whether fixed simulation is currently paused.
    pub const fn is_paused(&self) -> bool {
        self.time.is_paused()
    }

    /// Changes the multiplier applied to future fixed-time accumulation.
    ///
    /// FrameUpdate continues to receive unscaled wall time.
    pub fn set_time_scale(&mut self, time_scale: f64) -> Result<(), TimeConfigurationError> {
        self.time.set_time_scale(time_scale)
    }

    fn update_common_resources(&mut self) -> bool {
        let frame_snapshot_present = clear_frame_snapshot::<A>(&mut self.active.world);
        self.active.world.remove_resource::<FrameTimeState>();
        self.active.world.remove_resource::<FrameViewportState>();
        let Some(mut issuer) = self.active.world.get_resource_mut::<DirectIntentIssuer>() else {
            return false;
        };
        issuer.set_frame(self.frame_index, self.active.generation);
        frame_snapshot_present
    }

    fn run_frame_update(
        &mut self,
        elapsed: Duration,
        viewport: sim_engine::LogicalViewport,
        report: &mut LogicFrameReport,
        force_zero_extraction_alpha: &mut bool,
    ) {
        if !prepare_frame_snapshots(&mut self.active.world, &self.input) {
            report.failure = Some(FrameFailure::RuntimeInvariant);
            return;
        }
        self.active.world.remove_resource::<FixedTimeState>();
        self.active
            .world
            .insert_resource(self.time.frame_time(elapsed));
        self.active
            .world
            .insert_resource(FrameViewportState::new(viewport));
        if let Err(error) = run_stage(&mut self.active.frame, &mut self.active.world) {
            let commands_intact = self.consume_failed_stage_tokens();
            report.failure = Some(if commands_intact {
                match error {
                    StageExecutionFailure::System(error) => FrameFailure::System {
                        stage: Stage::FrameUpdate,
                        error,
                    },
                    StageExecutionFailure::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                }
            } else {
                FrameFailure::RuntimeInvariant
            });
            return;
        }

        let Some((failed_tokens, committed)) = apply_command_queue(
            &mut self.active.world,
            &mut self.active.approved,
            self.application,
            self.active.generation,
            self.active.managed_entities,
            self.config.entity_limit(),
        ) else {
            report.failure = Some(FrameFailure::RuntimeInvariant);
            return;
        };
        let committed = match committed {
            Ok(committed) => committed,
            Err(error) => {
                self.consume_tokens(&failed_tokens);
                report.failure = Some(match error {
                    CommandCommitError::Batch(error) => FrameFailure::Commands {
                        stage: Stage::FrameUpdate,
                        error,
                    },
                    CommandCommitError::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                });
                return;
            }
        };
        self.active.managed_entities = committed.live_entities;
        add_committed_counts(report, &committed);
        let pause = committed.pause;
        if committed.exit_requested {
            report.exit_requested = true;
            report.transitions_suppressed_by_exit = committed.transitions.len();
            self.input.clear_world_edges();
            snap_runtime_interpolation(&mut self.active);
            self.time.clear_accumulator();
            return;
        }
        if !committed.transitions.is_empty() {
            self.resolve_transition(committed.transitions, false, report);
        }
        *force_zero_extraction_alpha |= self.apply_committed_pause(pause);
    }

    fn apply_committed_pause(&mut self, pause: Option<bool>) -> bool {
        let Some(paused) = pause else {
            return false;
        };
        let resuming = self.time.is_paused() && !paused;
        self.set_paused(paused);
        // Resume occurs after fixed execution. Retain any whole ticks for the
        // next frame, but present the snapped state now with a valid alpha.
        resuming && self.time.interpolation_alpha().is_none()
    }

    fn resolve_transition(
        &mut self,
        requests: Vec<TransitionRequest>,
        stopped_fixed: bool,
        report: &mut LogicFrameReport,
    ) {
        let validation = self.validate_transition_requests(&requests, stopped_fixed);
        let tokens: Vec<_> = requests.iter().map(|request| request.intent()).collect();
        self.consume_tokens(&tokens);

        if let Err(failure) = validation {
            report.transition = FrameTransition::Invalid(failure);
            self.finish_rejected_fixed_transition(stopped_fixed, report);
            return;
        }

        match arbitrate(&requests) {
            TransitionArbitration::None => {}
            TransitionArbitration::Rejected(rejection) => {
                report.transition = FrameTransition::Rejected(rejection);
                self.finish_rejected_fixed_transition(stopped_fixed, report);
            }
            TransitionArbitration::Replace { target, warning } => {
                match self.prepare_world(target) {
                    Ok(mut candidate) => {
                        if !self
                            .application_resources
                            .can_transfer(&self.active.world, &candidate.world)
                        {
                            self.extraction.discard_staged();
                            self.time.clear_accumulator();
                            snap_runtime_interpolation(&mut self.active);
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                            return;
                        }
                        let old = self.active.generation;
                        self.record_lifecycle(LifecycleEvent::WorldExit, old, self.active.factory);
                        self.application_resources
                            .transfer_prevalidated(&mut self.active.world, &mut candidate.world);
                        let new = candidate.generation;
                        self.active = candidate;
                        self.record_lifecycle(LifecycleEvent::WorldEnter, new, target);
                        snap_runtime_interpolation(&mut self.active);
                        self.time.clear_accumulator();
                        self.input.clear_world_edges();
                        report.transition = FrameTransition::Committed {
                            old,
                            new,
                            target,
                            warning,
                        };
                        report.extracted_generation = self.extraction.publish();
                        if report.extracted_generation != Some(new) {
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                        }
                    }
                    Err(error) => {
                        self.time.clear_accumulator();
                        snap_runtime_interpolation(&mut self.active);
                        report.transition = FrameTransition::PreparationFailed { target, error };
                    }
                }
            }
        }
    }

    fn finish_rejected_fixed_transition(
        &mut self,
        stopped_fixed: bool,
        report: &mut LogicFrameReport,
    ) {
        if stopped_fixed {
            match self.time.drop_remaining_whole_ticks() {
                Ok(dropped) => report.dropped_for_transition = Some(dropped),
                Err(error) => report.failure = Some(FrameFailure::Time(error)),
            }
        }
    }

    fn validate_transition_requests(
        &self,
        requests: &[TransitionRequest],
        from_fixed_update: bool,
    ) -> Result<(), TransitionRequestFailure> {
        for request in requests {
            if !factory_exists(self.application, &self.factories, request.target()) {
                return Err(TransitionRequestFailure::InvalidFactory);
            }
            let token = request.intent();
            if token.application() != self.application || token.origin() != self.active.generation {
                return Err(TransitionRequestFailure::InvalidIntent);
            }
            match token.kind() {
                IntentKind::Input
                    if from_fixed_update && !self.input.token_is_live_in_fixed(token) =>
                {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Input
                    if !from_fixed_update && !self.input.token_is_live_in_frame(token) =>
                {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Direct if token.issued_frame() != self.frame_index => {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Input | IntentKind::Direct => {}
            }
        }
        Ok(())
    }

    fn consume_tokens(&mut self, tokens: &[TransitionIntentToken]) {
        for token in tokens {
            if token.kind() == IntentKind::Input {
                self.input.consume_token_occurrence(*token);
            }
        }
    }

    fn consume_failed_stage_tokens(&mut self) -> bool {
        let Some(tokens) = discard_command_queue(&mut self.active.world) else {
            return false;
        };
        self.consume_tokens(&tokens);
        true
    }

    fn prepare_world(
        &mut self,
        factory_id: WorldFactoryId,
    ) -> Result<RuntimeWorld, CandidateFailure> {
        self.extraction.discard_staged();
        let factory = self
            .factories
            .iter()
            .find(|factory| factory.id == factory_id)
            .map(|factory| ArcFactory(factory.factory.clone()))
            .ok_or(CandidateFailure::InvalidFactory)?;
        let generation = self.reserve_generation()?;
        let mut world = World::new();
        let approved = self
            .components
            .install(&mut world)
            .map_err(CandidateFailure::Components)?;
        let mut builder = WorldBuilder::new(
            world,
            approved,
            self.application,
            generation,
            self.config.entity_limit(),
        );
        (factory.0)(&mut builder).map_err(CandidateFailure::Factory)?;
        let (mut world, mut approved, managed_entities) = builder.finish();

        world.insert_resource(CommandQueue::new(self.config.command_limit()));
        world.insert_resource(DirectIntentIssuer::new(self.application, generation));
        world.insert_resource(WorldIdentityState::new(self.application, generation));
        if !world.contains_resource::<WorldBackground>() {
            world.insert_resource(WorldBackground::default());
        }
        self.events.install(&mut world, self.config.event_limit());

        let mut startup = self
            .startup_factories
            .instantiate(&mut world, Stage::Startup)
            .map_err(CandidateFailure::SystemSetup)?;
        let fixed = self
            .fixed_factories
            .instantiate(&mut world, Stage::FixedUpdate)
            .map_err(CandidateFailure::SystemSetup)?;
        let frame = self
            .frame_factories
            .instantiate(&mut world, Stage::FrameUpdate)
            .map_err(CandidateFailure::SystemSetup)?;
        if let Err(error) = run_stage(&mut startup, &mut world) {
            if discard_command_queue(&mut world).is_none() {
                return Err(CandidateFailure::RuntimeInvariant);
            }
            return Err(match error {
                StageExecutionFailure::System(error) => CandidateFailure::StartupSystem(error),
                StageExecutionFailure::RuntimeInvariant => CandidateFailure::RuntimeInvariant,
            });
        }
        let (_failed_tokens, committed) = apply_command_queue(
            &mut world,
            &mut approved,
            self.application,
            generation,
            managed_entities,
            self.config.entity_limit(),
        )
        .ok_or(CandidateFailure::RuntimeInvariant)?;
        let committed = committed.map_err(|error| match error {
            CommandCommitError::Batch(error) => CandidateFailure::StartupCommands(error),
            CommandCommitError::RuntimeInvariant => CandidateFailure::RuntimeInvariant,
        })?;
        if !committed.transitions.is_empty() {
            return Err(CandidateFailure::StartupTransition);
        }
        let managed_entities = committed.live_entities;
        snap_interpolation(&mut world);
        world.insert_resource(FrameInputState::<A>::empty());
        world.insert_resource(FixedInputState::<A>::empty());
        world.insert_resource(FrameTimeState::new(Duration::ZERO, false));
        world.insert_resource(FixedTimeState::initial(self.config.time().fixed_step()));

        let extraction_queries = ExtractionQueries::new(&mut world);
        let interpolation_queries = InterpolationQueries::new(&mut world);
        let mut runtime_world = RuntimeWorld {
            factory: factory_id,
            generation,
            world,
            approved,
            managed_entities,
            previous_translations: Vec::new(),
            previous_camera_centers: Vec::new(),
            interpolation_queries,
            extraction_queries,
            fixed,
            frame,
        };
        stage_runtime_world(
            &mut runtime_world,
            0.0,
            self.config.render(),
            &mut self.extraction,
        )
        .map_err(CandidateFailure::Extraction)?;
        Ok(runtime_world)
    }

    fn reserve_generation(&mut self) -> Result<WorldGeneration, CandidateFailure> {
        let sequence = self.next_generation;
        self.next_generation = self
            .next_generation
            .checked_add(1)
            .ok_or(CandidateFailure::GenerationExhausted)?;
        Ok(WorldGeneration::new(self.application, sequence))
    }

    fn record_lifecycle(
        &mut self,
        event: LifecycleEvent,
        generation: WorldGeneration,
        factory: WorldFactoryId,
    ) {
        if self.lifecycle.len() == self.config.lifecycle_trace_limit() {
            self.lifecycle.remove(0);
            self.dropped_lifecycle_records = self.dropped_lifecycle_records.saturating_add(1);
        }
        self.lifecycle.push(LifecycleRecord {
            event,
            generation,
            factory,
        });
    }
}

struct ArcFactory(std::sync::Arc<crate::app::WorldFactory>);

fn factory_exists(
    application: ApplicationId,
    factories: &[RegisteredWorldFactory],
    factory: WorldFactoryId,
) -> bool {
    factory.application() == application
        && factories
            .get(factory.slot() as usize)
            .is_some_and(|registered| registered.id == factory)
}

fn apply_command_queue(
    world: &mut World,
    approved: &mut ApprovedComponents,
    application: ApplicationId,
    generation: WorldGeneration,
    managed_entities: usize,
    entity_limit: usize,
) -> Option<(
    Vec<TransitionIntentToken>,
    Result<CommittedCommands, CommandCommitError>,
)> {
    world.try_resource_scope(|world, mut queue: Mut<CommandQueue>| {
        let tokens = queue
            .transition_requests()
            .map(TransitionRequest::intent)
            .collect();
        let result = queue.validate_and_apply(
            world,
            approved,
            application,
            generation,
            managed_entities,
            entity_limit,
        );
        (tokens, result)
    })
}

fn discard_command_queue(world: &mut World) -> Option<Vec<TransitionIntentToken>> {
    world.get_resource_mut::<CommandQueue>().map(|mut queue| {
        let tokens = queue
            .transition_requests()
            .map(TransitionRequest::intent)
            .collect();
        queue.discard();
        tokens
    })
}

fn snap_interpolation(world: &mut World) {
    let mut transforms = world.query_filtered::<&mut Transform2d, Allow<Disabled>>();
    for mut transform in transforms.iter_mut(world) {
        transform.snap_interpolation();
    }
    let mut cameras = world.query_filtered::<&mut ActiveCamera2d, Allow<Disabled>>();
    for mut camera in cameras.iter_mut(world) {
        camera.snap_interpolation();
    }
}

fn snap_runtime_interpolation(runtime: &mut RuntimeWorld) {
    for (_raw, _entity, mut transform) in runtime
        .interpolation_queries
        .transforms
        .iter_mut(&mut runtime.world)
    {
        transform.snap_interpolation();
    }
    for mut camera in runtime
        .interpolation_queries
        .snap_cameras
        .iter_mut(&mut runtime.world)
    {
        camera.snap_interpolation();
    }
}

fn clear_frame_snapshot<A: Action>(world: &mut World) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FrameInputState<A>>() else {
        return false;
    };
    snapshot.clear_reusing_storage();
    true
}

fn clear_fixed_snapshot<A: Action>(world: &mut World) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FixedInputState<A>>() else {
        return false;
    };
    snapshot.clear_reusing_storage();
    true
}

fn copy_fixed_snapshot<A: Action>(world: &mut World, input: &InputState<A>) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FixedInputState<A>>() else {
        return false;
    };
    input.copy_fixed_snapshot_into(&mut snapshot);
    true
}

fn prepare_frame_snapshots<A: Action>(world: &mut World, input: &InputState<A>) -> bool {
    if !world.contains_resource::<FrameInputState<A>>()
        || !world.contains_resource::<FixedInputState<A>>()
    {
        return false;
    }

    {
        let Some(mut frame_snapshot) = world.get_resource_mut::<FrameInputState<A>>() else {
            return false;
        };
        input.copy_frame_snapshot_into(&mut frame_snapshot);
    }

    clear_fixed_snapshot::<A>(world)
}

fn capture_and_begin_fixed_interpolation(
    world: &mut World,
    queries: &mut InterpolationQueries,
    captured_translations: &mut Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
    captured_cameras: &mut Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
) -> bool {
    captured_translations.clear();
    captured_cameras.clear();

    {
        let cameras = queries.cameras.iter(world);
        let Some(maximum_count) = cameras.size_hint().1 else {
            return false;
        };
        if captured_cameras.try_reserve(maximum_count).is_err() {
            return false;
        }
        for (raw, entity, camera) in cameras {
            captured_cameras.push((entity.handle(raw), camera.previous_center()));
        }
    }

    {
        let mut transforms = queries.transforms.iter_mut(world);
        let Some(maximum_transform_count) = transforms.size_hint().1 else {
            return false;
        };
        if captured_translations
            .try_reserve(maximum_transform_count)
            .is_err()
        {
            return false;
        }
        for (raw, entity, mut transform) in &mut transforms {
            captured_translations.push((entity.handle(raw), transform.previous_translation()));
            transform.begin_fixed_tick();
        }
    }
    for (index, (entity, _previous)) in captured_cameras.iter().enumerate() {
        let Some(mut camera) = world.get_mut::<ActiveCamera2d>(entity.entity()) else {
            debug_assert!(
                false,
                "captured camera disappeared without a structural barrier"
            );
            restore_previous_translations(world, captured_translations);
            restore_previous_camera_centers(world, &captured_cameras[..index]);
            return false;
        };
        camera.begin_fixed_tick();
    }
    true
}

fn restore_previous_translations(
    world: &mut World,
    before: &[(crate::identity::LogicEntity, sim_engine::Vec2)],
) {
    for (entity, previous) in before {
        if let Some(mut transform) = world.get_mut::<Transform2d>(entity.entity()) {
            transform.restore_previous_translation(*previous);
        }
    }
}

fn restore_previous_camera_centers(
    world: &mut World,
    before: &[(crate::identity::LogicEntity, sim_engine::Vec2)],
) {
    for (entity, previous) in before {
        if let Some(mut camera) = world.get_mut::<ActiveCamera2d>(entity.entity()) {
            camera.restore_previous_center(*previous);
        }
    }
}

fn stage_runtime_world(
    runtime: &mut RuntimeWorld,
    alpha: f32,
    limits: RenderLimits,
    extraction: &mut ExtractionBuffers,
) -> Result<(), ExtractionError> {
    let background = runtime
        .world
        .get_resource::<WorldBackground>()
        .copied()
        .unwrap_or_default()
        .color();
    let cameras = runtime
        .extraction_queries
        .cameras
        .iter(&runtime.world)
        .copied()
        .take(2);
    let circles = runtime.extraction_queries.circles.iter(&runtime.world).map(
        |(raw, entity, transform, visual)| CircleSource::new(entity.handle(raw), transform, visual),
    );
    let rectangles = runtime
        .extraction_queries
        .rectangles
        .iter(&runtime.world)
        .map(|(raw, entity, transform, visual)| {
            RectangleSource::new(entity.handle(raw), transform, visual)
        });
    let lines = runtime.extraction_queries.lines.iter(&runtime.world).map(
        |(raw, entity, transform, visual)| LineSource::new(entity.handle(raw), transform, visual),
    );
    let screen_rectangles = runtime
        .extraction_queries
        .screen_rectangles
        .iter(&runtime.world)
        .map(|(raw, entity, visual)| ScreenRectangleSource::new(entity.handle(raw), visual));
    extraction.stage(
        ExtractionParameters::new(runtime.generation, background, alpha, limits),
        cameras,
        circles,
        rectangles,
        lines,
        screen_rectangles,
    )
}

fn add_committed_counts(report: &mut LogicFrameReport, committed: &CommittedCommands) {
    report.spawned = report.spawned.saturating_add(committed.spawned);
    report.despawned = report.despawned.saturating_add(committed.despawned);
}

fn fixed_tick_failure(error: FixedTickError) -> FrameFailure {
    match error {
        FixedTickError::TickIndexExhausted
        | FixedTickError::Paused
        | FixedTickError::NoPlannedTick => FrameFailure::RuntimeInvariant,
    }
}

#[cfg(test)]
mod tests {
    use std::{
        any::type_name,
        error::Error,
        sync::{
            Arc, Mutex,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use bevy_ecs::{
        entity_disabling::{DefaultQueryFilters, Disabled},
        prelude::{Component, Res, ResMut, Resource, With},
    };
    use sim_engine::{Camera2d, Color, LogicalViewport, Vec2};

    use crate::{
        app::{AppConfig, Application},
        commands::{CommandEnqueueError, CommandQueue, DirectIntentIssuer, LogicCommands},
        events::{EventReader, EventSendError, EventWriter},
        identity::{
            LogicEntity, LogicEntityRef, ManagedEntity, TransitionIntentToken, WorldGeneration,
        },
        input::{
            ButtonState, FixedInput, FixedInputState, FrameInput, FrameInputState,
            InputCollectionError, InputEvent, PhysicalKeyCode,
        },
        query::{Query, QueryEntityError, Single},
        render::FrameViewport,
        system::Stage,
        time::{FixedTime, FrameTime, TimeConfig},
        transition::TransitionRejection,
        visual::{ActiveCamera2d, CircleVisual, Transform2d},
        world::WorldBuildError,
    };

    use super::{
        BeginFrameRejection, CandidateFailure, FrameFailure, FrameOutcome, FrameRequest,
        FrameTransition, LifecycleEvent, RunnerBuildError, TransitionRequestFailure,
        capture_and_begin_fixed_interpolation,
    };

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {
        MoveLeft,
        MoveRight,
        MoveDown,
        MoveUp,
        Enter,
    }

    const TEST_MOVEMENT: crate::input::DigitalAxis2d<TestAction> = crate::input::DigitalAxis2d::new(
        TestAction::MoveLeft,
        TestAction::MoveRight,
        TestAction::MoveDown,
        TestAction::MoveUp,
    );

    #[derive(Component)]
    struct Ball;

    #[derive(Component)]
    struct Ephemeral;

    #[derive(Component)]
    struct SameStageValue(u8);

    #[derive(Resource)]
    struct NextWorld(crate::identity::WorldFactoryId);

    #[derive(Resource)]
    struct MissingFixedResource;

    #[derive(Resource)]
    struct ResourceOnlyComponent;

    #[derive(Resource, Default)]
    struct ManagedQueryCounts {
        all: usize,
        resource_components: usize,
    }

    #[derive(Resource, Default)]
    struct SameStageObservation(Option<u8>);

    #[derive(Debug, Clone, Copy)]
    struct PrimaryEvent(u8);

    #[derive(Debug, Clone, Copy)]
    struct SecondaryEvent(u8);

    #[derive(Resource, Default)]
    struct EventObservation {
        startup_first: Vec<u8>,
        startup_second: Vec<u8>,
        secondary: Vec<u8>,
        fixed_before_writer: Vec<usize>,
        fixed_after_writer: Vec<Vec<u8>>,
        frame_counts: Vec<usize>,
    }

    #[derive(Resource, Default)]
    struct EventFailureObservation {
        attempts: usize,
        later_runs: usize,
    }

    #[derive(Resource, Default)]
    struct StartupEventCount(usize);

    fn count_managed_query_entities(
        all: Query<()>,
        resource_components: Query<&ResourceOnlyComponent>,
        mut counts: ResMut<ManagedQueryCounts>,
    ) {
        counts.all = all.iter().count();
        counts.resource_components = resource_components.iter().count();
    }

    fn write_same_stage_component(mut values: Query<&mut SameStageValue>) {
        for mut value in &mut values {
            value.0 = 7;
        }
    }

    fn observe_same_stage_component(
        values: Query<&SameStageValue>,
        mut observation: ResMut<SameStageObservation>,
    ) {
        observation.0 = values.iter().next().map(|value| value.0);
    }

    fn send_startup_events(
        mut primary: EventWriter<PrimaryEvent>,
        mut secondary: EventWriter<SecondaryEvent>,
    ) {
        assert!(primary.send(PrimaryEvent(3)).is_ok());
        assert_eq!(primary.iter().map(|event| event.0).collect::<Vec<_>>(), [3]);
        assert!(primary.send(PrimaryEvent(4)).is_ok());
        assert!(secondary.send(SecondaryEvent(9)).is_ok());
    }

    fn read_startup_events_first(
        primary: EventReader<PrimaryEvent>,
        secondary: EventReader<SecondaryEvent>,
        mut observation: ResMut<EventObservation>,
    ) {
        observation.startup_first = primary.iter().map(|event| event.0).collect();
        observation.secondary = secondary.iter().map(|event| event.0).collect();
    }

    fn read_startup_events_second(
        primary: EventReader<PrimaryEvent>,
        mut observation: ResMut<EventObservation>,
    ) {
        observation.startup_second = (&primary).into_iter().map(|event| event.0).collect();
    }

    fn read_fixed_before_writer(
        events: EventReader<PrimaryEvent>,
        mut observation: ResMut<EventObservation>,
    ) {
        observation.fixed_before_writer.push(events.len());
    }

    fn send_fixed_event(time: FixedTime, mut events: EventWriter<PrimaryEvent>) {
        let value = u8::try_from(time.tick_index()).expect("test tick index should fit in u8");
        assert!(events.send(PrimaryEvent(value)).is_ok());
    }

    fn read_fixed_after_writer(
        events: EventReader<PrimaryEvent>,
        mut observation: ResMut<EventObservation>,
    ) {
        observation
            .fixed_after_writer
            .push(events.iter().map(|event| event.0).collect());
    }

    fn read_frame_events(
        events: EventReader<PrimaryEvent>,
        mut observation: ResMut<EventObservation>,
    ) {
        observation.frame_counts.push(events.len());
    }

    #[derive(Resource)]
    struct TargetWorld;

    #[derive(Default, Resource)]
    struct EnabledBallCount(usize);

    #[derive(Resource)]
    struct StoredFrameIntent(Option<TransitionIntentToken>);

    #[derive(Resource)]
    struct StartupIntentObservation {
        rejected_as_unavailable: bool,
    }

    #[derive(Resource)]
    struct StartupExitObservation {
        rejected_as_unavailable: bool,
    }

    #[derive(Resource)]
    struct StartupPauseObservation {
        rejected_as_unavailable: bool,
    }

    #[derive(Default, Resource)]
    struct SpawnThenDespawn(bool);

    #[derive(Default, Resource)]
    struct RejectSecondFixedBatch(u8);

    #[derive(Default, Resource)]
    struct CameraRepairPhase(u8);

    #[derive(Resource)]
    struct InsertTarget(LogicEntity);

    #[derive(Default, Resource)]
    struct InsertObservation {
        queued: bool,
        same_stage_counts: Vec<usize>,
    }

    #[derive(Default, Resource)]
    struct RemoveObservation {
        queued: bool,
        same_stage_counts: Vec<usize>,
    }

    #[derive(Default, Resource)]
    struct StageInputObservation {
        fixed_actual_edges: usize,
        fixed_foreign_edges: usize,
        fixed_foreign_held: bool,
        frame_actual_edges: usize,
        frame_foreign_edges: usize,
        frame_foreign_held: bool,
    }

    #[derive(Default, Resource)]
    struct RetainedEdgeObservation {
        pressed: usize,
        released: usize,
        held: bool,
    }

    #[derive(Default, Resource)]
    struct FallibleObservation {
        attempts: usize,
        later_runs: usize,
        frame_runs: usize,
    }

    #[derive(Default, Resource)]
    struct FloatTimeObservation {
        fixed_bits: Option<u32>,
        frame_bits: Option<u32>,
    }

    #[derive(Resource)]
    struct RejectStartup;

    fn move_ball(
        input: FixedInput<TestAction>,
        time: FixedTime,
        mut balls: Query<&mut Transform2d, With<Ball>>,
    ) {
        let direction = input.normalized_digital_axis(TEST_MOVEMENT);
        let delta = direction * (60.0 * time.seconds_f32());
        for mut transform in &mut balls {
            assert!(transform.translate_by(delta).is_ok());
        }
    }

    fn move_insert_target(mut target: Single<&mut Transform2d, With<Ball>>) {
        assert!(target.translate_by(Vec2::new(2.0, 0.0)).is_ok());
    }

    fn observe_insert_visibility(
        visuals: Query<&CircleVisual, With<Ball>>,
        mut observation: ResMut<InsertObservation>,
    ) {
        observation.same_stage_counts.push(visuals.iter().count());
    }

    fn follow_ball(
        balls: Query<&Transform2d, With<Ball>>,
        mut cameras: Query<&mut ActiveCamera2d>,
    ) -> Result<(), Box<dyn Error>> {
        let target = balls.single()?.translation();
        cameras.single_mut()?.set_center(target)?;
        Ok(())
    }

    fn request_next_world(
        input: FixedInput<TestAction>,
        next: Option<Res<NextWorld>>,
        mut commands: LogicCommands,
    ) {
        let Some(next) = next else {
            return;
        };
        for edge in input.pressed(TestAction::Enter) {
            assert!(commands.replace_world(edge.intent(), next.0).is_ok());
        }
    }

    fn require_missing_fixed_resource(_missing: Res<MissingFixedResource>) {}

    fn fixed_reads_frame_time(_time: FrameTime) {}

    fn fixed_reads_frame_viewport(_viewport: FrameViewport) {}

    fn frame_reads_fixed_time(_time: FixedTime) {}

    fn startup_reads_frame_input(_input: FrameInput<TestAction>) {}

    fn startup_reads_fixed_time(_time: FixedTime) {}

    fn observe_startup_intent(
        mut observation: ResMut<StartupIntentObservation>,
        mut commands: LogicCommands,
    ) {
        observation.rejected_as_unavailable = matches!(
            commands.new_transition_intent(),
            Err(CommandEnqueueError::IntentUnavailable)
        );
    }

    fn observe_startup_exit(
        mut observation: ResMut<StartupExitObservation>,
        mut commands: LogicCommands,
    ) {
        observation.rejected_as_unavailable = matches!(
            commands.request_exit(),
            Err(CommandEnqueueError::ExitUnavailable)
        );
    }

    fn observe_startup_pause(
        mut observation: ResMut<StartupPauseObservation>,
        mut commands: LogicCommands,
    ) {
        observation.rejected_as_unavailable = matches!(
            commands.set_paused(true),
            Err(CommandEnqueueError::PauseUnavailable)
        );
    }

    fn observe_fixed_stage_input(
        fixed: FixedInput<TestAction>,
        frame: FrameInput<TestAction>,
        mut observation: ResMut<StageInputObservation>,
    ) {
        observation.fixed_actual_edges = fixed.pressed(TestAction::MoveRight).count();
        observation.fixed_foreign_edges = frame.pressed(TestAction::MoveRight).count();
        observation.fixed_foreign_held = frame.held(TestAction::MoveRight);
    }

    fn observe_frame_stage_input(
        frame: FrameInput<TestAction>,
        fixed: FixedInput<TestAction>,
        mut observation: ResMut<StageInputObservation>,
    ) {
        observation.frame_actual_edges = frame.pressed(TestAction::MoveRight).count();
        observation.frame_foreign_edges = fixed.pressed(TestAction::MoveRight).count();
        observation.frame_foreign_held = fixed.held(TestAction::MoveRight);
    }

    fn observe_retained_edges(
        input: FixedInput<TestAction>,
        mut observation: ResMut<RetainedEdgeObservation>,
    ) {
        observation.pressed = input.pressed(TestAction::MoveRight).count();
        observation.released = input.released(TestAction::MoveRight).count();
        observation.held = input.held(TestAction::MoveRight);
    }

    fn reject_marked_startup(reject: Option<Res<RejectStartup>>) -> Result<(), &'static str> {
        if reject.is_some() {
            Err("candidate startup rejected")
        } else {
            Ok(())
        }
    }

    fn spawn_then_despawn(
        mut phase: ResMut<SpawnThenDespawn>,
        entities: Query<LogicEntityRef, With<Ephemeral>>,
        mut commands: LogicCommands,
    ) {
        if !phase.0 {
            assert!(commands.spawn(Ephemeral).is_ok());
            phase.0 = true;
            return;
        }

        for entity in &entities {
            assert!(commands.despawn(entity.handle()).is_ok());
        }
    }

    fn reject_second_fixed_batch(
        mut phase: ResMut<RejectSecondFixedBatch>,
        mut commands: LogicCommands,
    ) {
        if phase.0 == 0 {
            phase.0 = 1;
            return;
        }
        if phase.0 > 1 {
            return;
        }
        phase.0 = 2;
        assert!(commands.spawn(Ephemeral).is_ok());
        assert!(matches!(
            commands.spawn(Ephemeral),
            Err(CommandEnqueueError::LimitExceeded { limit: 1 })
        ));
    }

    fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
        Ok(LogicalViewport::new(800.0, 600.0)?)
    }

    fn camera() -> Result<ActiveCamera2d, Box<dyn Error>> {
        Ok(ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?))
    }

    fn circle(color: Color) -> Result<CircleVisual, Box<dyn Error>> {
        Ok(CircleVisual::new(1.0, color)?)
    }

    #[test]
    fn advanced_frames_advance_standalone_ecs_trackers() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("tracker-world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        let before = runner.active.world.last_change_tick();

        let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            first,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        let after_first = runner.active.world.last_change_tick();
        assert_ne!(after_first, before);

        let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            second,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        assert_ne!(runner.active.world.last_change_tick(), after_first);
        Ok(())
    }

    #[test]
    fn managed_queries_never_visit_internal_resource_entities() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("managed-query-world", move |world| {
            world.insert_resource(ResourceOnlyComponent)?;
            world.insert_resource(ManagedQueryCounts::default())?;
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, count_managed_query_entities);

        let runner = application.build_headless(world)?;
        let counts = runner
            .resource::<ManagedQueryCounts>()
            .ok_or("Startup count resource should remain available")?;

        assert_eq!(counts.all, 1);
        assert_eq!(counts.resource_components, 0);
        Ok(())
    }

    #[test]
    fn headless_component_lookup_rejects_another_application() -> Result<(), Box<dyn Error>> {
        let mut first_application = Application::<TestAction>::new(AppConfig::default())?;
        let first_camera = camera()?;
        let first_world = first_application.register_world("first-app", move |world| {
            world.spawn(first_camera)?;
            Ok(())
        })?;
        let first_runner = first_application.build_headless(first_world)?;

        let mut second_application = Application::<TestAction>::new(AppConfig::default())?;
        let second_camera = camera()?;
        let second_world = second_application.register_world("second-app", move |world| {
            world.spawn(second_camera)?;
            Ok(())
        })?;
        let second_runner = second_application.build_headless(second_world)?;
        let foreign = second_runner
            .components::<ActiveCamera2d>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("second application should contain its camera")?;

        assert!(matches!(
            first_runner.component::<ActiveCamera2d>(foreign),
            Err(QueryEntityError::ForeignWorld { .. })
        ));
        Ok(())
    }

    #[test]
    fn headless_component_lookup_rejects_mismatched_private_provenance()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("private-provenance", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        let entity = runner
            .components::<ActiveCamera2d>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("World should contain its camera")?;
        let corrupted = WorldGeneration::new(runner.application, 99);
        runner
            .active
            .world
            .entity_mut(entity.entity())
            .insert(ManagedEntity::for_generation(corrupted));

        assert!(matches!(
            runner.component::<ActiveCamera2d>(entity),
            Err(QueryEntityError::DoesNotMatch { entity: rejected }) if rejected == entity
        ));
        Ok(())
    }

    #[test]
    fn later_system_observes_component_write_in_the_same_stage() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<SameStageValue>()?;
        let camera = camera()?;
        let world = application.register_world("same-stage-world", move |world| {
            world.insert_resource(SameStageObservation::default())?;
            world.spawn(camera)?;
            world.spawn(SameStageValue(0))?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, write_same_stage_component);
        application.add_system(Stage::Startup, observe_same_stage_component);

        let runner = application.build_headless(world)?;

        assert_eq!(
            runner
                .resource::<SameStageObservation>()
                .and_then(|observation| observation.0),
            Some(7)
        );
        Ok(())
    }

    #[test]
    fn typed_events_are_ordered_repeatable_and_scoped_to_one_stage_invocation()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        let mut application = Application::<TestAction>::new(config)?;
        let camera = camera()?;
        let world = application.register_world("event-world", move |world| {
            world.spawn(camera)?;
            world.insert_resource(EventObservation::default())?;
            Ok(())
        })?;

        application.add_system(Stage::Startup, send_startup_events);
        application.add_system(Stage::Startup, read_startup_events_first);
        application.add_system(Stage::Startup, read_startup_events_second);
        application.add_system(Stage::FixedUpdate, read_fixed_before_writer);
        application.add_system(Stage::FixedUpdate, send_fixed_event);
        application.add_system(Stage::FixedUpdate, read_fixed_after_writer);
        application.add_system(Stage::FrameUpdate, read_frame_events);

        let mut runner = application.build_headless(world)?;
        let startup = runner
            .resource::<EventObservation>()
            .ok_or("event observation should exist")?;
        assert_eq!(startup.startup_first, [3, 4]);
        assert_eq!(startup.startup_second, [3, 4]);
        assert_eq!(startup.secondary, [9]);

        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(20),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 2);

        let observation = runner
            .resource::<EventObservation>()
            .ok_or("event observation should remain available")?;
        assert_eq!(observation.fixed_before_writer, [0, 0]);
        assert_eq!(observation.fixed_after_writer, [vec![0], vec![1]]);
        assert_eq!(observation.frame_counts, [0]);
        Ok(())
    }

    #[test]
    fn ignored_event_limit_error_stops_stage_and_discards_commands() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_event_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;
        let camera = camera()?;
        let world = application.register_world("event-limit", move |world| {
            world.spawn(camera)?;
            world.insert_resource(EventFailureObservation::default())?;
            Ok(())
        })?;

        application.add_system(
            Stage::FixedUpdate,
            |mut observation: ResMut<EventFailureObservation>,
             mut events: EventWriter<PrimaryEvent>,
             mut secondary: EventWriter<SecondaryEvent>,
             mut commands: LogicCommands| {
                if observation.attempts > 0 {
                    return;
                }
                observation.attempts += 1;
                assert!(commands.request_exit().is_ok());
                assert!(commands.spawn(Ephemeral).is_ok());
                assert!(events.send(PrimaryEvent(1)).is_ok());
                let first = events
                    .send(PrimaryEvent(2))
                    .expect_err("second primary event should exceed the limit");
                let repeated = secondary
                    .send(SecondaryEvent(8))
                    .expect_err("poisoned stage should reject another event type");
                assert!(matches!(
                    first,
                    EventSendError::LimitExceeded { event, limit: 1 }
                        if event == type_name::<PrimaryEvent>()
                ));
                assert_eq!(repeated, EventSendError::StageAlreadyFailed);
            },
        );
        application.add_system(
            Stage::FixedUpdate,
            |mut observation: ResMut<EventFailureObservation>| {
                observation.later_runs += 1;
            },
        );

        let mut runner = application.build_headless(world)?;
        let failed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };
        let Some(FrameFailure::System { stage, error }) = failed_report.failure() else {
            panic!("ignored event overflow should poison the fixed stage");
        };
        assert_eq!(*stage, Stage::FixedUpdate);
        assert!(error.reason().contains("exceeded its limit of 1"));
        assert!(error.reason().contains(type_name::<PrimaryEvent>()));
        assert!(!failed_report.exit_requested());
        assert_eq!(failed_report.spawned(), 0);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);
        let observation = runner
            .resource::<EventFailureObservation>()
            .ok_or("failure observation should exist")?;
        assert_eq!(observation.attempts, 1);
        assert_eq!(observation.later_runs, 0);

        let recovered = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert_eq!(
            runner
                .resource::<EventFailureObservation>()
                .ok_or("failure observation should remain available")?
                .later_runs,
            1
        );
        Ok(())
    }

    #[test]
    fn startup_event_failure_discards_the_isolated_candidate() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_event_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        let camera = camera()?;
        let world = application.register_world("startup-event-limit", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, |mut events: EventWriter<PrimaryEvent>| {
            assert!(events.send(PrimaryEvent(1)).is_ok());
            let _ = events.send(PrimaryEvent(2));
        });

        let error = match application.build_headless(world) {
            Ok(_) => panic!("poisoned Startup should reject the candidate"),
            Err(error) => error,
        };
        let RunnerBuildError::InitialWorld(CandidateFailure::StartupSystem(error)) = error else {
            panic!("event failure should be reported as a Startup System failure");
        };
        assert!(error.reason().contains("exceeded its limit of 1"));
        Ok(())
    }

    #[test]
    fn explicit_system_error_takes_priority_over_event_poison() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_event_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        let camera = camera()?;
        let world = application.register_world("event-error-priority", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_fallible_system(
            Stage::FixedUpdate,
            |mut events: EventWriter<PrimaryEvent>| -> Result<(), &'static str> {
                events
                    .send(PrimaryEvent(1))
                    .expect("first event should fit");
                let _ = events.send(PrimaryEvent(2));
                Err("explicit event system failure")
            },
        );

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        let Some(FrameFailure::System { error, .. }) = report.failure() else {
            panic!("fallible System should stop the fixed stage");
        };
        assert_eq!(
            error.reason(),
            "returned an error: explicit event system failure"
        );
        Ok(())
    }

    #[test]
    fn frame_event_poison_stops_later_systems_and_discards_commands() -> Result<(), Box<dyn Error>>
    {
        let mut config = AppConfig::default();
        config.set_event_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;
        let camera = camera()?;
        let world = application.register_world("frame-event-limit", move |world| {
            world.spawn(camera)?;
            world.insert_resource(EventFailureObservation::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |mut observation: ResMut<EventFailureObservation>,
             mut events: EventWriter<PrimaryEvent>,
             mut commands: LogicCommands| {
                if observation.attempts > 0 {
                    return;
                }
                observation.attempts += 1;
                assert!(commands.spawn(Ephemeral).is_ok());
                assert!(events.send(PrimaryEvent(1)).is_ok());
                let _ = events.send(PrimaryEvent(2));
            },
        );
        application.add_system(
            Stage::FrameUpdate,
            |mut observation: ResMut<EventFailureObservation>| {
                observation.later_runs += 1;
            },
        );

        let mut runner = application.build_headless(world)?;
        let failed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            failed_report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FrameUpdate,
                ..
            })
        ));
        assert!(!failed_report.exit_requested());
        assert_eq!(failed_report.spawned(), 0);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);
        assert_eq!(
            runner
                .resource::<EventFailureObservation>()
                .ok_or("frame failure observation should exist")?
                .later_runs,
            0
        );

        let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert_eq!(
            runner
                .resource::<EventFailureObservation>()
                .ok_or("frame failure observation should remain available")?
                .later_runs,
            1
        );
        Ok(())
    }

    #[test]
    fn event_poison_discards_transition_and_consumes_its_input_intent() -> Result<(), Box<dyn Error>>
    {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_event_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("event-poison-target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("event-poison-source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FixedUpdate,
            |input: FixedInput<TestAction>,
             next: Option<Res<NextWorld>>,
             mut events: EventWriter<PrimaryEvent>,
             mut commands: LogicCommands| {
                let Some(next) = next else {
                    return;
                };
                for edge in input.pressed(TestAction::Enter) {
                    assert!(commands.replace_world(edge.intent(), next.0).is_ok());
                    assert!(events.send(PrimaryEvent(1)).is_ok());
                    let _ = events.send(PrimaryEvent(2));
                }
            },
        );

        let mut runner = application.build_headless(source)?;
        let source_generation = runner.world_generation();
        let pressed = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let failed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &pressed,
            viewport()?,
        ));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            failed_report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FixedUpdate,
                ..
            })
        ));
        assert!(matches!(failed_report.transition(), FrameTransition::None));
        assert_eq!(runner.world_generation(), source_generation);

        let recovered = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert!(matches!(
            recovered_report.transition(),
            FrameTransition::None
        ));
        assert_eq!(runner.world_generation(), source_generation);
        Ok(())
    }

    #[test]
    fn command_poison_rejects_transition_consumes_intent_and_reuses_queue()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_command_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("command-poison-target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("command-poison-source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FixedUpdate,
            |input: FixedInput<TestAction>,
             next: Option<Res<NextWorld>>,
             mut commands: LogicCommands| {
                let mut requested = false;
                if let Some(next) = next {
                    for edge in input.pressed(TestAction::Enter) {
                        requested = true;
                        assert!(commands.replace_world(edge.intent(), next.0).is_ok());
                        assert!(matches!(
                            commands.spawn(Ephemeral),
                            Err(CommandEnqueueError::LimitExceeded { limit: 1 })
                        ));
                    }
                }
                if !requested {
                    assert!(commands.spawn(Ephemeral).is_ok());
                }
            },
        );

        let mut runner = application.build_headless(source)?;
        let source_generation = runner.world_generation();
        let pressed = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let failed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &pressed,
            viewport()?,
        ));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded command-poison frame should be accepted");
        };
        assert!(matches!(
            failed_report.failure(),
            Some(FrameFailure::Commands {
                stage: Stage::FixedUpdate,
                error: crate::commands::CommandBatchError::LimitExceeded { limit: 1 },
            })
        ));
        assert!(matches!(failed_report.transition(), FrameTransition::None));
        assert_eq!(runner.world_generation(), source_generation);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);

        let recovered = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert!(matches!(
            recovered_report.transition(),
            FrameTransition::None
        ));
        assert_eq!(recovered_report.spawned(), 1);
        assert_eq!(runner.world_generation(), source_generation);
        assert_eq!(runner.components::<Ephemeral>().count(), 1);
        Ok(())
    }

    #[test]
    fn committed_world_starts_with_fresh_event_channels() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        let mut application = Application::<TestAction>::new(config)?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("fresh-event-target", move |world| {
            world.spawn(target_camera)?;
            world.insert_resource(StartupEventCount::default())?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("fresh-event-source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::Startup,
            |events: EventReader<PrimaryEvent>, count: Option<ResMut<StartupEventCount>>| {
                if let Some(mut count) = count {
                    count.0 = events.len();
                }
            },
        );
        application.add_system(
            Stage::FixedUpdate,
            |input: FixedInput<TestAction>,
             next: Option<Res<NextWorld>>,
             mut events: EventWriter<PrimaryEvent>,
             mut commands: LogicCommands| {
                let Some(next) = next else {
                    return;
                };
                for edge in input.pressed(TestAction::Enter) {
                    assert!(events.send(PrimaryEvent(7)).is_ok());
                    assert!(commands.replace_world(edge.intent(), next.0).is_ok());
                }
            },
        );

        let mut runner = application.build_headless(source)?;
        let pressed = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &pressed,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert!(matches!(
            report.transition(),
            FrameTransition::Committed { .. }
        ));
        assert_eq!(runner.active_world_name(), "fresh-event-target");
        assert_eq!(
            runner
                .resource::<StartupEventCount>()
                .ok_or("target startup observation should exist")?
                .0,
            0
        );
        Ok(())
    }

    #[test]
    fn fallible_fixed_system_stops_stage_without_rolling_back_direct_writes()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_command_limit(2)?;

        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ball>()?;
        application.approve_component::<Ephemeral>()?;

        let target_camera = camera()?;
        let target = application.register_world("fallible-target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("fallible-source", move |world| {
            world.spawn(source_camera)?;
            world.spawn((Ball, Transform2d::default()))?;
            world.insert_resource(NextWorld(target))?;
            world.insert_resource(FallibleObservation::default())?;
            Ok(())
        })?;

        application.add_system(Stage::FixedUpdate, |mut commands: LogicCommands| {
            commands
                .spawn(Ephemeral)
                .expect("first System should queue within the stage limit");
        });

        application.add_fallible_system(
            Stage::FixedUpdate,
            |mut observation: ResMut<FallibleObservation>,
             next: Res<NextWorld>,
             mut balls: Query<&mut Transform2d, With<Ball>>,
             mut commands: LogicCommands|
             -> Result<(), CommandEnqueueError> {
                if observation.attempts > 0 {
                    return Ok(());
                }
                observation.attempts += 1;
                for mut transform in &mut balls {
                    transform
                        .translate_by(Vec2::new(2.0, 0.0))
                        .expect("finite test translation should remain valid");
                }

                let intent = commands.new_transition_intent()?;
                commands.replace_world(intent, next.0)?;
                commands.spawn(Ephemeral)?;
                Ok(())
            },
        );
        application.add_system(
            Stage::FixedUpdate,
            |mut observation: ResMut<FallibleObservation>| {
                observation.later_runs += 1;
            },
        );
        application.add_system(
            Stage::FrameUpdate,
            |mut observation: ResMut<FallibleObservation>| {
                observation.frame_runs += 1;
            },
        );

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let failed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };

        assert_eq!(failed_report.fixed_ticks_attempted(), 1);
        let Some(FrameFailure::System { stage, error }) = failed_report.failure() else {
            panic!("fallible fixed System should report a System failure");
        };
        assert_eq!(*stage, Stage::FixedUpdate);
        assert!(error.reason().starts_with("returned an error: "));
        assert!(error.reason().contains("command limit of 2"));
        assert_eq!(runner.world_generation(), old_generation);
        assert!(matches!(failed_report.transition(), FrameTransition::None));
        assert_eq!(failed_report.spawned(), 0);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);

        let observation = runner
            .resource::<FallibleObservation>()
            .ok_or("fallible observation should remain available")?;
        assert_eq!(observation.attempts, 1);
        assert_eq!(observation.later_runs, 0);
        assert_eq!(observation.frame_runs, 0);
        let (_, transform) = runner
            .components::<Transform2d>()
            .find(|(_, transform)| transform.translation() == Vec2::new(2.0, 0.0))
            .ok_or("direct Transform write should not be rolled back")?;
        assert_eq!(transform.previous_translation(), Vec2::ZERO);

        let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(recovered_report.spawned(), 1);
        assert_eq!(runner.components::<Ephemeral>().count(), 1);
        let observation = runner
            .resource::<FallibleObservation>()
            .ok_or("fallible observation should remain available")?;
        assert!(observation.later_runs > 0);
        assert_eq!(observation.frame_runs, 1);
        Ok(())
    }

    #[test]
    fn fallible_frame_system_discards_commands_and_can_run_again() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_command_limit(1)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;

        let camera = camera()?;
        let world = application.register_world("fallible-frame", move |world| {
            world.spawn(camera)?;
            world.insert_resource(FallibleObservation::default())?;
            Ok(())
        })?;
        application.add_fallible_system(
            Stage::FrameUpdate,
            |mut observation: ResMut<FallibleObservation>,
             mut commands: LogicCommands|
             -> Result<(), CommandEnqueueError> {
                if observation.attempts > 0 {
                    return Ok(());
                }
                observation.attempts += 1;
                commands.spawn(Ephemeral)?;
                commands.spawn(Ephemeral)?;
                Ok(())
            },
        );
        application.add_system(
            Stage::FrameUpdate,
            |mut observation: ResMut<FallibleObservation>| {
                observation.later_runs += 1;
            },
        );

        let mut runner = application.build_headless(world)?;
        let failed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            failed_report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FrameUpdate,
                ..
            })
        ));
        assert_eq!(failed_report.extracted_generation(), None);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);
        let observation = runner
            .resource::<FallibleObservation>()
            .ok_or("fallible observation should remain available")?;
        assert_eq!(observation.attempts, 1);
        assert_eq!(observation.later_runs, 0);

        let recovered = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(recovered_report) = recovered else {
            panic!("bounded recovery frame should be accepted");
        };
        assert!(recovered_report.failure().is_none());
        assert_eq!(runner.components::<Ephemeral>().count(), 0);
        assert_eq!(
            runner
                .resource::<FallibleObservation>()
                .ok_or("fallible observation should remain available")?
                .later_runs,
            1
        );
        Ok(())
    }

    #[test]
    fn fallible_startup_error_rejects_initial_world() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("rejected-initial", move |world| {
            world.spawn(camera)?;
            world.insert_resource(RejectStartup)?;
            Ok(())
        })?;
        application.add_fallible_system(Stage::Startup, reject_marked_startup);

        let error = match application.build_headless(world) {
            Ok(_) => panic!("fallible Startup should reject the initial candidate"),
            Err(error) => error,
        };
        let super::RunnerBuildError::InitialWorld(super::CandidateFailure::StartupSystem(error)) =
            error
        else {
            panic!("initial build should retain the typed Startup failure path");
        };
        assert!(error.system().ends_with("reject_marked_startup"));
        assert_eq!(
            error.reason(),
            "returned an error: candidate startup rejected"
        );
        Ok(())
    }

    #[test]
    fn fallible_startup_error_discards_replacement_candidate() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("rejected-target", move |world| {
            world.spawn(target_camera)?;
            world.insert_resource(RejectStartup)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("retained-source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_fallible_system(Stage::Startup, reject_marked_startup);
        application.add_system(Stage::FixedUpdate, request_next_world);

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded transition frame should be accepted");
        };

        let FrameTransition::PreparationFailed {
            target: failed,
            error,
        } = report.transition()
        else {
            panic!("fallible Startup should reject only the isolated candidate");
        };
        assert_eq!(*failed, target);
        let super::CandidateFailure::StartupSystem(error) = error else {
            panic!("candidate should retain the typed Startup failure path");
        };
        assert!(
            error.system().ends_with("reject_marked_startup"),
            "unexpected System name: {}",
            error.system()
        );
        assert_eq!(
            error.reason(),
            "returned an error: candidate startup rejected"
        );
        assert!(report.failure().is_none());
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(runner.active_world_name(), "retained-source");
        Ok(())
    }

    #[test]
    fn fixed_input_moves_and_interpolates_a_circle() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world_a = application.register_world("world-a", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Transform2d::default(), ball))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);

        let mut runner = application.build_headless(world_a)?;
        let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert!(report.failure().is_none());
        assert!(matches!(report.transition(), FrameTransition::None));
        let extracted = runner
            .extracted_frame()
            .ok_or("successful frame should publish extraction")?;
        let [resolved] = extracted.resolved_circles() else {
            panic!("test World should extract exactly one circle");
        };
        assert!((resolved.position().x() - 0.5).abs() < 0.001);
        Ok(())
    }

    fn read_transform_for_empty_stage_boundary(transforms: Query<&Transform2d, With<Ball>>) {
        assert_eq!(transforms.iter().count(), 1);
    }

    #[test]
    fn only_an_exactly_empty_fixed_stage_skips_interpolation_capture() -> Result<(), Box<dyn Error>>
    {
        for has_fixed_system in [false, true] {
            let mut application = Application::<TestAction>::new(AppConfig::default())?;
            application.approve_component::<Ball>()?;
            if has_fixed_system {
                application.add_fixed_system(read_transform_for_empty_stage_boundary);
            }
            let camera = camera()?;
            let transform = Transform2d::from_xy(3.0, -2.0)?;
            let world = application.register_world("capture-boundary", move |world| {
                world.spawn(camera)?;
                world.spawn((Ball, transform))?;
                Ok(())
            })?;
            let mut runner = application.build_headless(world)?;

            let outcome = runner.advance_frame(FrameRequest::new(
                Duration::from_millis(17),
                &[],
                viewport()?,
            ));
            let FrameOutcome::Advanced(report) = outcome else {
                panic!("bounded capture-boundary frame should be accepted");
            };
            assert!(report.failure().is_none());
            assert_eq!(report.fixed_ticks_attempted(), 1);
            assert_eq!(
                runner.active.previous_translations.len(),
                usize::from(has_fixed_system)
            );
            assert_eq!(
                runner.active.previous_camera_centers.len(),
                usize::from(has_fixed_system)
            );
            let (_, retained) = runner
                .components::<Transform2d>()
                .next()
                .expect("test Transform should remain managed");
            assert_eq!(retained.translation(), transform.translation());
            assert_eq!(retained.previous_translation(), transform.translation());
        }
        Ok(())
    }

    #[test]
    fn fixed_camera_follow_matches_visual_interpolation_across_catch_up_and_zero_tick_frames()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world = application.register_world("camera-follow", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Transform2d::default(), ball))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);
        application.add_fallible_system(Stage::FixedUpdate, follow_ball);

        let mut runner = application.build_headless(world)?;
        let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let catch_up = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &movement,
            viewport()?,
        ));
        let FrameOutcome::Advanced(catch_up_report) = catch_up else {
            panic!("bounded catch-up frame should be accepted");
        };
        assert_eq!(catch_up_report.fixed_ticks_attempted(), 2);
        assert!(catch_up_report.failure().is_none());

        let (_, camera) = runner
            .components::<ActiveCamera2d>()
            .next()
            .ok_or("active camera should remain inspectable")?;
        assert!((camera.previous_center().x() - 0.6).abs() < 0.001);
        assert!((camera.center().x() - 1.2).abs() < 0.001);
        let catch_up_frame = runner
            .extracted_frame()
            .ok_or("catch-up frame should publish extraction")?;
        assert!((catch_up_frame.camera().center().x() - 0.9).abs() < 0.001);
        assert!((catch_up_frame.resolved_circles()[0].position().x() - 0.9).abs() < 0.001);

        let zero_tick = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(2),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(zero_tick_report) = zero_tick else {
            panic!("bounded zero-tick frame should be accepted");
        };
        assert_eq!(zero_tick_report.fixed_ticks_attempted(), 0);
        assert!(zero_tick_report.failure().is_none());

        let (_, unchanged) = runner
            .components::<ActiveCamera2d>()
            .next()
            .ok_or("active camera should remain inspectable")?;
        assert!((unchanged.previous_center().x() - 0.6).abs() < 0.001);
        assert!((unchanged.center().x() - 1.2).abs() < 0.001);
        let zero_tick_frame = runner
            .extracted_frame()
            .ok_or("zero-tick frame should publish extraction")?;
        assert!((zero_tick_frame.camera().center().x() - 1.02).abs() < 0.001);
        assert!((zero_tick_frame.resolved_circles()[0].position().x() - 1.02).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn managed_time_parameters_expose_direct_f32_duration_conversion() -> Result<(), Box<dyn Error>>
    {
        let duration = Duration::new(0, 16_874_317);
        assert_ne!(
            duration.as_secs_f32().to_bits(),
            (duration.as_secs_f64() as f32).to_bits()
        );
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(duration, 1)?);
        let mut application = Application::<TestAction>::new(config)?;
        let camera = camera()?;
        let world = application.register_world("f32-time", move |world| {
            world.spawn(camera)?;
            world.insert_resource(FloatTimeObservation::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FixedUpdate,
            |time: FixedTime, mut observation: ResMut<FloatTimeObservation>| {
                observation.fixed_bits = Some(time.seconds_f32().to_bits());
            },
        );
        application.add_system(
            Stage::FrameUpdate,
            |time: FrameTime, mut observation: ResMut<FloatTimeObservation>| {
                observation.frame_bits = Some(time.seconds_f32().to_bits());
            },
        );

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(duration, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded f32-time frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        let observation = runner
            .resource::<FloatTimeObservation>()
            .ok_or("f32 time observation should remain available")?;
        let expected = duration.as_secs_f32().to_bits();
        assert_eq!(observation.fixed_bits, Some(expected));
        assert_eq!(observation.frame_bits, Some(expected));
        Ok(())
    }

    #[test]
    fn diagonal_wasd_movement_is_normalized() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        application.bind_key(PhysicalKeyCode::KeyW, TestAction::MoveUp)?;

        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world = application.register_world("diagonal-world", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Transform2d::default(), ball))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);

        let mut runner = application.build_headless(world)?;
        let events = [
            InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
        ];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded diagonal frame should be accepted");
        };
        assert!(report.failure().is_none());

        let ball_entity = runner
            .components::<Ball>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("moving ball should remain available")?;
        let (_, transform) = runner
            .components::<Transform2d>()
            .find(|(entity, _)| *entity == ball_entity)
            .ok_or("moving ball transform should remain available")?;
        let translation = transform.translation();
        let distance =
            (translation.x() * translation.x() + translation.y() * translation.y()).sqrt();
        assert!((distance - 1.0).abs() < 0.000_1);
        assert!((translation.x() - translation.y()).abs() < 0.000_1);
        Ok(())
    }

    #[test]
    fn command_spawn_snaps_supplied_interpolation_history() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut copied_history = Transform2d::default();
        copied_history.set_translation(Vec2::new(10.0, 0.0))?;
        application.add_system(Stage::FrameUpdate, move |mut commands: LogicCommands| {
            assert!(commands.spawn((Ball, copied_history, circle)).is_ok());
        });

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(8),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert!(report.failure().is_none());
        let extracted = runner
            .extracted_frame()
            .ok_or("successful frame should publish extraction")?;
        let [resolved] = extracted.resolved_circles() else {
            panic!("command should spawn exactly one circle");
        };
        assert_eq!(resolved.position(), Vec2::new(10.0, 0.0));
        Ok(())
    }

    #[test]
    fn command_insert_crosses_the_stage_barrier_and_preserves_existing_motion()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(100), 1)?);
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("insert-world", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, Transform2d::default()))?;
            world.insert_resource(InsertTarget(target))?;
            world.insert_resource(InsertObservation::default())?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_insert_target);
        application.add_system(
            Stage::FrameUpdate,
            move |target: Res<InsertTarget>,
                  mut observation: ResMut<InsertObservation>,
                  mut commands: LogicCommands| {
                if !observation.queued {
                    assert!(commands.insert(target.0, circle).is_ok());
                    observation.queued = true;
                }
            },
        );
        application.add_system(Stage::FrameUpdate, observe_insert_visibility);

        let mut runner = application.build_headless(world)?;
        let inserted = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(150),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(inserted_report) = inserted else {
            panic!("bounded insert frame should be accepted");
        };
        assert!(inserted_report.failure().is_none());
        assert_eq!(inserted_report.fixed_ticks_attempted(), 1);
        assert_eq!(inserted_report.spawned(), 0);
        assert_eq!(inserted_report.despawned(), 0);
        assert_eq!(
            runner
                .resource::<InsertObservation>()
                .ok_or("insert observation should exist")?
                .same_stage_counts,
            [0]
        );

        let (target, _) = runner
            .components::<Ball>()
            .next()
            .ok_or("insert target should remain live")?;
        assert!(runner.component::<CircleVisual>(target).is_ok());
        let transform = runner.component::<Transform2d>(target)?;
        assert_eq!(transform.previous_translation(), Vec2::ZERO);
        assert_eq!(transform.translation(), Vec2::new(2.0, 0.0));
        let [resolved] = runner
            .extracted_frame()
            .ok_or("insert frame should publish extraction")?
            .resolved_circles()
        else {
            panic!("inserted visual should extract in the barrier frame");
        };
        assert_eq!(resolved.source(), target);
        assert_eq!(resolved.position(), Vec2::new(1.0, 0.0));

        let observed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            observed,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        assert_eq!(
            runner
                .resource::<InsertObservation>()
                .ok_or("insert observation should remain")?
                .same_stage_counts,
            [0, 1]
        );
        Ok(())
    }

    #[test]
    fn command_insert_can_disable_after_the_barrier_without_losing_identity()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_components::<(Ball, Disabled)>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("disable-world", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle))?;
            world.insert_resource(InsertTarget(target))?;
            world.insert_resource(InsertObservation::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |target: Res<InsertTarget>,
             mut observation: ResMut<InsertObservation>,
             mut commands: LogicCommands| {
                if !observation.queued {
                    assert!(commands.insert(target.0, Disabled).is_ok());
                    observation.queued = true;
                }
            },
        );
        application.add_system(Stage::FrameUpdate, observe_insert_visibility);

        let mut runner = application.build_headless(world)?;
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("initial extraction should exist")?
                .resolved_circles()
                .len(),
            1
        );
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("insert target should exist")?
            .0;
        let disabled = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            disabled,
            FrameOutcome::Advanced(ref report)
                if report.failure().is_none()
                    && report.spawned() == 0
                    && report.despawned() == 0
        ));
        assert!(runner.component::<Disabled>(target).is_ok());
        assert!(runner.component::<Ball>(target).is_ok());
        assert!(
            runner
                .extracted_frame()
                .ok_or("disabled frame should publish extraction")?
                .resolved_circles()
                .is_empty()
        );
        assert_eq!(
            runner
                .resource::<InsertObservation>()
                .ok_or("insert observation should exist")?
                .same_stage_counts,
            [1]
        );

        let next = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            next,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        assert_eq!(
            runner
                .resource::<InsertObservation>()
                .ok_or("insert observation should remain")?
                .same_stage_counts,
            [1, 0]
        );
        Ok(())
    }

    #[test]
    fn command_remove_crosses_each_fixed_barrier_before_extraction() -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 4)?);
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("remove-world", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle))?;
            world.insert_resource(InsertTarget(target))?;
            world.insert_resource(RemoveObservation::default())?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_insert_target);
        application.add_system(
            Stage::FixedUpdate,
            |target: Res<InsertTarget>,
             mut observation: ResMut<RemoveObservation>,
             mut commands: LogicCommands| {
                if !observation.queued {
                    assert!(commands.remove::<CircleVisual>(target.0).is_ok());
                    observation.queued = true;
                }
            },
        );
        application.add_system(
            Stage::FixedUpdate,
            |visuals: Query<&CircleVisual, With<Ball>>,
             mut observation: ResMut<RemoveObservation>| {
                observation.same_stage_counts.push(visuals.iter().count());
            },
        );

        let mut runner = application.build_headless(world)?;
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("remove target should exist")?
            .0;
        let removed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = removed else {
            panic!("bounded remove frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 2);
        assert_eq!(report.spawned(), 0);
        assert_eq!(report.despawned(), 0);
        assert_eq!(
            runner
                .resource::<RemoveObservation>()
                .ok_or("remove observation should exist")?
                .same_stage_counts,
            [1, 0]
        );
        assert!(matches!(
            runner.component::<CircleVisual>(target),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert!(runner.component::<Transform2d>(target).is_ok());
        let transform = runner.component::<Transform2d>(target)?;
        assert_eq!(transform.previous_translation(), Vec2::new(2.0, 0.0));
        assert_eq!(transform.translation(), Vec2::new(4.0, 0.0));
        assert!(runner.component::<Ball>(target).is_ok());
        assert!(
            runner
                .extracted_frame()
                .ok_or("remove frame should publish extraction")?
                .resolved_circles()
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn startup_remove_shapes_the_candidate_before_its_first_extraction()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("startup-remove", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle))?;
            world.insert_resource(InsertTarget(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::Startup,
            |target: Res<InsertTarget>, mut commands: LogicCommands| {
                assert!(commands.remove::<CircleVisual>(target.0).is_ok());
            },
        );

        let runner = application.build_headless(world)?;
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("startup remove target should exist")?
            .0;
        assert!(matches!(
            runner.component::<CircleVisual>(target),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert!(runner.component::<Transform2d>(target).is_ok());
        assert!(
            runner
                .extracted_frame()
                .ok_or("startup removal should still permit initial extraction")?
                .resolved_circles()
                .is_empty()
        );
        Ok(())
    }

    #[test]
    fn command_remove_disabled_enables_after_the_barrier_without_losing_identity()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_components::<(Ball, Disabled)>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("enable-world", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle, Disabled))?;
            world.insert_resource(InsertTarget(target))?;
            world.insert_resource(RemoveObservation::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |target: Res<InsertTarget>,
             mut observation: ResMut<RemoveObservation>,
             mut commands: LogicCommands| {
                if !observation.queued {
                    assert!(commands.remove::<Disabled>(target.0).is_ok());
                    observation.queued = true;
                }
            },
        );
        application.add_system(
            Stage::FrameUpdate,
            |balls: Query<&Ball>, mut observation: ResMut<RemoveObservation>| {
                observation.same_stage_counts.push(balls.iter().count());
            },
        );

        let mut runner = application.build_headless(world)?;
        assert!(
            runner
                .extracted_frame()
                .ok_or("initial extraction should exist")?
                .resolved_circles()
                .is_empty()
        );
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("enable target should exist")?
            .0;
        let enabled = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            enabled,
            FrameOutcome::Advanced(ref report)
                if report.failure().is_none()
                    && report.spawned() == 0
                    && report.despawned() == 0
        ));
        assert!(matches!(
            runner.component::<Disabled>(target),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert!(runner.component::<Ball>(target).is_ok());
        let [resolved] = runner
            .extracted_frame()
            .ok_or("enabled frame should publish extraction")?
            .resolved_circles()
        else {
            panic!("enabled entity should extract after the barrier");
        };
        assert_eq!(resolved.source(), target);
        assert_eq!(
            runner
                .resource::<RemoveObservation>()
                .ok_or("remove observation should exist")?
                .same_stage_counts,
            [0]
        );

        let next = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            next,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        assert_eq!(
            runner
                .resource::<RemoveObservation>()
                .ok_or("remove observation should remain")?
                .same_stage_counts,
            [0, 1]
        );
        Ok(())
    }

    #[test]
    fn invalid_required_component_removal_rejects_exit_and_preserves_the_entity()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("invalid-remove", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle))?;
            world.insert_resource(InsertTarget(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |target: Res<InsertTarget>, mut commands: LogicCommands| {
                assert!(commands.remove::<Transform2d>(target.0).is_ok());
                assert!(commands.request_exit().is_ok());
            },
        );

        let mut runner = application.build_headless(world)?;
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("invalid-remove target should exist")?
            .0;
        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded invalid-remove frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::Commands {
                stage: Stage::FrameUpdate,
                error: crate::commands::CommandBatchError::RequiredComponentWouldBeMissing {
                    entity,
                    ..
                },
            }) if *entity == target
        ));
        assert!(!report.exit_requested());
        assert!(runner.component::<Ball>(target).is_ok());
        assert!(runner.component::<CircleVisual>(target).is_ok());
        assert!(runner.component::<Transform2d>(target).is_ok());
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("rejected removal should preserve the prior publication")?
                .resolved_circles()
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn successful_remove_commits_before_application_exit() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("remove-exit", move |world| {
            world.spawn(camera)?;
            let target = world.spawn((Ball, circle))?;
            world.insert_resource(InsertTarget(target))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |target: Res<InsertTarget>, mut commands: LogicCommands| {
                assert!(commands.remove::<CircleVisual>(target.0).is_ok());
                assert!(commands.request_exit().is_ok());
            },
        );

        let mut runner = application.build_headless(world)?;
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("remove-exit target should exist")?
            .0;
        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded remove-exit frame should be accepted");
        };
        assert!(report.failure().is_none());
        assert!(report.exit_requested());
        assert!(report.extracted_generation().is_none());
        assert!(matches!(
            runner.component::<CircleVisual>(target),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert!(runner.component::<Transform2d>(target).is_ok());
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("exit should retain only the previous publication")?
                .resolved_circles()
                .len(),
            1
        );
        Ok(())
    }

    #[test]
    fn cached_interpolation_query_discovers_command_spawned_archetype() -> Result<(), Box<dyn Error>>
    {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            move |balls: Query<&Ball>, mut commands: LogicCommands| {
                if balls.iter().next().is_none() {
                    assert!(
                        commands
                            .spawn((Ball, Transform2d::default(), circle))
                            .is_ok()
                    );
                }
            },
        );
        application.add_system(Stage::FixedUpdate, move_ball);

        let mut runner = application.build_headless(world)?;
        let spawned = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        assert!(matches!(
            spawned,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));

        let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        for events in [&movement[..], &[][..]] {
            let moved = runner.advance_frame(FrameRequest::new(
                Duration::from_millis(17),
                events,
                viewport()?,
            ));
            assert!(matches!(
                moved,
                FrameOutcome::Advanced(ref report)
                    if report.failure().is_none() && report.fixed_ticks_attempted() == 1
            ));
        }

        let (_, transform) = runner
            .components::<Transform2d>()
            .next()
            .ok_or("command-spawned transform should remain inspectable")?;
        assert!((transform.previous_translation().x() - 1.0).abs() < 0.000_1);
        assert!((transform.translation().x() - 2.0).abs() < 0.000_1);
        Ok(())
    }

    #[test]
    fn cached_interpolation_query_advances_disabled_transform_history() -> Result<(), Box<dyn Error>>
    {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_components::<(Ball, Disabled)>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let initial = Transform2d::new(Vec2::new(3.0, 0.0))?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Disabled, initial, circle))?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        let entity = runner
            .components::<Ball>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("disabled transform should remain inspectable")?;
        runner
            .active
            .world
            .get_mut::<Transform2d>(entity.entity())
            .ok_or("disabled transform should exist")?
            .set_translation(Vec2::new(7.0, 0.0))?;

        let active = &mut runner.active;
        assert!(capture_and_begin_fixed_interpolation(
            &mut active.world,
            &mut active.interpolation_queries,
            &mut active.previous_translations,
            &mut active.previous_camera_centers,
        ));
        let transform = active
            .world
            .get::<Transform2d>(entity.entity())
            .ok_or("disabled transform should survive capture")?;
        assert_eq!(transform.previous_translation(), Vec2::new(7.0, 0.0));
        Ok(())
    }

    #[test]
    fn command_spawn_snaps_supplied_camera_history() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let initial_camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(initial_camera)?;
            Ok(())
        })?;
        let mut copied_history = camera()?;
        copied_history.set_center(Vec2::new(10.0, 4.0))?;
        application.add_system(
            Stage::FrameUpdate,
            move |cameras: Query<LogicEntityRef, With<ActiveCamera2d>>,
                  mut commands: LogicCommands| {
                let current = cameras
                    .single()
                    .expect("the valid World should have one active camera")
                    .handle();
                assert!(commands.despawn(current).is_ok());
                assert!(commands.spawn(copied_history).is_ok());
            },
        );

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(8),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert!(report.failure().is_none());
        let (_, camera) = runner
            .components::<ActiveCamera2d>()
            .next()
            .ok_or("replacement camera should be inspectable")?;
        assert_eq!(camera.previous_center(), Vec2::new(10.0, 4.0));
        assert_eq!(camera.center(), Vec2::new(10.0, 4.0));
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("successful frame should publish extraction")?
                .camera()
                .center(),
            Vec2::new(10.0, 4.0)
        );
        Ok(())
    }

    #[test]
    fn multiple_active_cameras_can_be_repaired_after_extraction_failure()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let initial_camera = camera()?;
        let extra_camera = ActiveCamera2d::new(Camera2d::new(Vec2::new(5.0, 0.0), 32.0)?);
        let world = application.register_world("repairable-camera", move |world| {
            world.spawn(initial_camera)?;
            world.insert_resource(CameraRepairPhase::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            move |cameras: Query<LogicEntityRef, With<ActiveCamera2d>>,
                  mut phase: ResMut<CameraRepairPhase>,
                  mut commands: LogicCommands| {
                if phase.0 == 0 {
                    assert!(commands.spawn(extra_camera).is_ok());
                    phase.0 = 1;
                } else if phase.0 == 1 {
                    let extra = cameras
                        .iter()
                        .nth(1)
                        .expect("failed extraction should retain two cameras")
                        .handle();
                    assert!(commands.despawn(extra).is_ok());
                    phase.0 = 2;
                }
            },
        );

        let mut runner = application.build_headless(world)?;
        let broken = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(broken_report) = broken else {
            panic!("bounded camera-break frame should be accepted");
        };
        assert!(matches!(
            broken_report.failure(),
            Some(FrameFailure::Extraction(
                crate::ExtractionError::MultipleActiveCameras
            ))
        ));
        assert_eq!(runner.components::<ActiveCamera2d>().count(), 2);

        let repaired = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(repaired_report) = repaired else {
            panic!("bounded camera-repair frame should be accepted");
        };
        assert!(repaired_report.failure().is_none());
        assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("repaired World should publish extraction")?
                .world_generation(),
            runner.world_generation()
        );
        Ok(())
    }

    #[test]
    fn removed_active_camera_commits_and_can_be_reinserted_after_extraction_failure()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let initial_camera = camera()?;
        let world = application.register_world("removable-camera", move |world| {
            let target = world.spawn(initial_camera)?;
            world.insert_resource(InsertTarget(target))?;
            world.insert_resource(CameraRepairPhase::default())?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |target: Res<InsertTarget>,
             mut phase: ResMut<CameraRepairPhase>,
             mut commands: LogicCommands| {
                if phase.0 == 0 {
                    assert!(commands.remove::<ActiveCamera2d>(target.0).is_ok());
                    phase.0 = 1;
                } else if phase.0 == 1 {
                    let replacement = ActiveCamera2d::new(
                        Camera2d::new(Vec2::new(7.0, -3.0), 32.0)
                            .expect("replacement camera should be valid"),
                    );
                    assert!(commands.insert(target.0, replacement).is_ok());
                    phase.0 = 2;
                }
            },
        );

        let mut runner = application.build_headless(world)?;
        let published_before = runner
            .extracted_frame()
            .ok_or("initial camera extraction should exist")?
            .camera()
            .center();
        let target = runner
            .resource::<InsertTarget>()
            .ok_or("camera target should exist")?
            .0;

        let broken = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(broken_report) = broken else {
            panic!("bounded camera-removal frame should be accepted");
        };
        assert!(matches!(
            broken_report.failure(),
            Some(FrameFailure::Extraction(
                crate::ExtractionError::MissingActiveCamera
            ))
        ));
        assert!(matches!(
            runner.component::<ActiveCamera2d>(target),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("failed extraction should preserve the old publication")?
                .camera()
                .center(),
            published_before
        );

        let repaired = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(repaired_report) = repaired else {
            panic!("bounded camera-repair frame should be accepted");
        };
        assert!(repaired_report.failure().is_none());
        let camera = runner.component::<ActiveCamera2d>(target)?;
        assert_eq!(camera.previous_center(), Vec2::new(7.0, -3.0));
        assert_eq!(camera.center(), Vec2::new(7.0, -3.0));
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("repaired camera should publish")?
                .camera()
                .center(),
            Vec2::new(7.0, -3.0)
        );
        Ok(())
    }

    #[test]
    fn one_input_intent_commits_world_a_to_b_during_catch_up() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let mut camera_b = camera()?;
        camera_b.set_center(Vec2::new(7.0, 3.0))?;
        let circle_b = circle(Color::rgb8(32, 96, 240))?;
        let transform_b = Transform2d::new(Vec2::new(7.0, 3.0))?;
        let world_b = application.register_world("world-b", move |world| {
            world.spawn(camera_b)?;
            world.spawn((Ball, transform_b, circle_b))?;
            world.insert_resource(RetainedEdgeObservation::default())?;
            Ok(())
        })?;

        let camera_a = camera()?;
        let circle_a = circle(Color::rgb8(240, 96, 32))?;
        let world_a = application.register_world("world-a", move |world| {
            world.spawn(camera_a)?;
            world.spawn((Ball, Transform2d::default(), circle_a))?;
            world.insert_resource(NextWorld(world_b))?;
            Ok(())
        })?;

        // Two systems react to the same causal edge. Arbitration must coalesce
        // their identical requests instead of treating the catch-up frame as a
        // conflict or allowing a second fixed tick to repeat the request.
        application.add_system(Stage::FixedUpdate, request_next_world);
        application.add_system(Stage::FixedUpdate, request_next_world);
        application.add_system(
            Stage::FixedUpdate,
            |input: FixedInput<TestAction>,
             observation: Option<ResMut<RetainedEdgeObservation>>| {
                let Some(mut observation) = observation else {
                    return;
                };
                observation.pressed = input.pressed(TestAction::Enter).count();
                observation.released = input.released(TestAction::Enter).count();
                observation.held = input.held(TestAction::Enter);
            },
        );

        let mut runner = application.build_headless(world_a)?;
        assert_eq!(runner.factory_name(world_a), Some("world-a"));
        assert_eq!(runner.factory_name(world_b), Some("world-b"));
        assert_eq!(runner.active_world_name(), "world-a");
        let old_generation = runner.world_generation();
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(50),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        let FrameTransition::Committed {
            old,
            new,
            target,
            warning,
        } = report.transition()
        else {
            panic!("input edge should commit World B");
        };
        assert_eq!(*old, old_generation);
        assert_eq!(*new, runner.world_generation());
        assert_eq!(*target, world_b);
        assert_eq!(runner.active_world_name(), "world-b");
        assert!(warning.is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert!(report.failure().is_none());
        assert_eq!(report.extracted_generation(), Some(*new));

        let lifecycle = runner.lifecycle();
        assert_eq!(lifecycle.len(), 3);
        assert_eq!(lifecycle[0].event(), LifecycleEvent::WorldEnter);
        assert_eq!(lifecycle[0].generation(), *old);
        assert_eq!(lifecycle[1].event(), LifecycleEvent::WorldExit);
        assert_eq!(lifecycle[1].generation(), *old);
        assert_eq!(lifecycle[2].event(), LifecycleEvent::WorldEnter);
        assert_eq!(lifecycle[2].generation(), *new);

        let extracted = runner
            .extracted_frame()
            .ok_or("committed World should be extracted in the same frame")?;
        assert_eq!(extracted.world_generation(), *new);
        assert_eq!(extracted.camera().center(), Vec2::new(7.0, 3.0));
        let [resolved] = extracted.resolved_circles() else {
            panic!("World B should extract exactly one circle");
        };
        assert_eq!(resolved.position(), Vec2::new(7.0, 3.0));

        let frame_snapshot = runner
            .active
            .world
            .get_resource::<FrameInputState<TestAction>>()
            .ok_or("committed World should own a frame snapshot")?;
        let fixed_snapshot = runner
            .active
            .world
            .get_resource::<FixedInputState<TestAction>>()
            .ok_or("committed World should own a fixed snapshot")?;
        assert!(!frame_snapshot.held(TestAction::Enter));
        assert_eq!(frame_snapshot.pressed(TestAction::Enter).count(), 0);
        assert!(!fixed_snapshot.held(TestAction::Enter));
        assert_eq!(fixed_snapshot.pressed(TestAction::Enter).count(), 0);

        let next = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        assert!(matches!(
            next,
            FrameOutcome::Advanced(ref report)
                if report.failure().is_none() && report.fixed_ticks_attempted() == 1
        ));
        let observation = runner
            .resource::<RetainedEdgeObservation>()
            .ok_or("target input observation should remain available")?;
        assert!(observation.held);
        assert_eq!(observation.pressed, 0);
        assert_eq!(observation.released, 0);
        Ok(())
    }

    #[test]
    fn entering_pause_revokes_an_undelivered_fixed_transition_edge() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let camera_b = camera()?;
        let world_b = application.register_world("world-b", move |world| {
            world.spawn(camera_b)?;
            Ok(())
        })?;
        let camera_a = camera()?;
        let world_a = application.register_world("world-a", move |world| {
            world.spawn(camera_a)?;
            world.insert_resource(NextWorld(world_b))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, request_next_world);

        let mut runner = application.build_headless(world_a)?;
        let original_generation = runner.world_generation();
        let press = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &press, viewport()?));
        let FrameOutcome::Advanced(first) = first else {
            panic!("zero-tick press frame should be accepted");
        };
        assert_eq!(first.fixed_ticks_attempted(), 0);
        assert!(matches!(first.transition(), FrameTransition::None));

        runner.set_paused(true);
        let release = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Released,
        )];
        let paused = runner.advance_frame(FrameRequest::new(
            Duration::from_secs(1),
            &release,
            viewport()?,
        ));
        let FrameOutcome::Advanced(paused) = paused else {
            panic!("paused release frame should be accepted");
        };
        assert_eq!(paused.fixed_ticks_attempted(), 0);

        runner.set_paused(false);
        let resumed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(resumed) = resumed else {
            panic!("resumed frame should be accepted");
        };

        assert_eq!(resumed.fixed_ticks_attempted(), 1);
        assert!(matches!(resumed.transition(), FrameTransition::None));
        assert_eq!(runner.world_generation(), original_generation);
        assert_eq!(runner.active_world_name(), "world-a");
        Ok(())
    }

    #[test]
    fn retired_logic_entity_cannot_alias_a_reused_bevy_slot() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let camera_b = camera()?;
        let circle_b = circle(Color::WHITE)?;
        let world_b = application.register_world("world-b", move |world| {
            world.insert_resource(TargetWorld)?;
            world.spawn(camera_b)?;
            world.spawn((Ball, Transform2d::default(), circle_b))?;
            Ok(())
        })?;
        let camera_a = camera()?;
        let circle_a = circle(Color::WHITE)?;
        let world_a = application.register_world("world-a", move |world| {
            world.insert_resource(NextWorld(world_b))?;
            world.spawn(camera_a)?;
            world.spawn((Ball, Transform2d::default(), circle_a))?;
            Ok(())
        })?;

        let retained = Arc::new(Mutex::new(None));
        let retained_for_system = Arc::clone(&retained);
        let rejected = Arc::new(AtomicUsize::new(0));
        let rejected_by_system = Arc::clone(&rejected);
        application.add_system(Stage::FixedUpdate, request_next_world);
        application.add_system(
            Stage::FixedUpdate,
            move |target: Option<Res<TargetWorld>>, mut balls: Query<&mut Transform2d>| {
                if target.is_none() {
                    return;
                }
                let entity = retained_for_system.lock().ok().and_then(|stored| *stored);
                let Some(entity) = entity else {
                    return;
                };
                if matches!(
                    balls.get_mut(entity),
                    Err(QueryEntityError::ForeignWorld { .. })
                ) {
                    rejected_by_system.store(1, Ordering::SeqCst);
                }
            },
        );

        let mut runner = application.build_headless(world_a)?;
        let old_entity = runner
            .components::<Ball>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("World A should contain a ball")?;
        let mut stored = retained.lock().map_err(|_| "test lock was poisoned")?;
        *stored = Some(old_entity);
        drop(stored);

        let enter = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let first = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &enter,
            viewport()?,
        ));
        assert!(matches!(
            first,
            FrameOutcome::Advanced(ref report)
                if matches!(report.transition(), FrameTransition::Committed { .. })
        ));
        assert!(matches!(
            runner.component::<Transform2d>(old_entity),
            Err(QueryEntityError::ForeignWorld { .. })
        ));
        let second = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        assert!(matches!(
            second,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        assert_eq!(rejected.load(Ordering::SeqCst), 1);
        Ok(())
    }

    #[test]
    fn extraction_excludes_disabled_even_if_bevy_defaults_are_replaced()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.approve_component::<Disabled>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let world = application.register_world("world", move |world| {
            world.insert_resource(DefaultQueryFilters::empty())?;
            world.insert_resource(EnabledBallCount::default())?;
            world.spawn(camera)?;
            world.spawn((Disabled, camera))?;
            world.spawn((Ball, Disabled, Transform2d::default(), circle))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            |balls: Query<&Ball>, mut count: ResMut<EnabledBallCount>| {
                count.0 = balls.iter().count();
            },
        );

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        assert!(matches!(
            outcome,
            FrameOutcome::Advanced(ref report) if report.failure().is_none()
        ));
        let extracted = runner
            .extracted_frame()
            .ok_or("initial World should publish extraction")?;
        assert!(extracted.resolved_circles().is_empty());
        assert_eq!(runner.components::<ActiveCamera2d>().count(), 2);
        assert_eq!(
            runner
                .resource::<EnabledBallCount>()
                .ok_or("observation should remain present")?
                .0,
            0
        );
        let disabled_ball = runner
            .components::<Ball>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("headless inspection should include the disabled ball")?;
        assert!(runner.component::<Ball>(disabled_ball).is_ok());
        assert!(runner.component::<Transform2d>(disabled_ball).is_ok());
        assert!(matches!(
            runner.component::<Ephemeral>(disabled_ball),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn failed_candidate_preserves_old_generation_and_snaps_interpolation()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let failing_world = application.register_world("failing-world", |_world| {
            Err(WorldBuildError::user("deliberate candidate failure"))
        })?;
        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world_a = application.register_world("world-a", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Transform2d::default(), ball))?;
            world.insert_resource(NextWorld(failing_world))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);
        application.add_fallible_system(Stage::FixedUpdate, follow_ball);
        application.add_system(Stage::FixedUpdate, request_next_world);

        let mut runner = application.build_headless(world_a)?;
        let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let first = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &movement,
            viewport()?,
        ));
        let FrameOutcome::Advanced(first_report) = first else {
            panic!("bounded movement frame should be accepted");
        };
        assert!(first_report.failure().is_none());
        let first_extracted = runner
            .extracted_frame()
            .ok_or("movement frame should publish an extraction")?;
        let [first_circle] = first_extracted.resolved_circles() else {
            panic!("test World should extract exactly one circle");
        };
        assert!(first_circle.position().x() > 0.0);
        assert!(first_circle.position().x() < 1.0);

        let old_generation = runner.world_generation();
        let transition = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let second = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(9),
            &transition,
            viewport()?,
        ));
        let FrameOutcome::Advanced(second_report) = second else {
            panic!("bounded transition frame should be accepted");
        };

        assert!(matches!(
            second_report.transition(),
            FrameTransition::PreparationFailed { target, .. } if *target == failing_world
        ));
        assert!(second_report.failure().is_none());
        assert_eq!(second_report.fixed_ticks_attempted(), 1);
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(runner.lifecycle().len(), 1);
        assert!(runner.active.world.contains_resource::<CommandQueue>());

        let Some((_, transform)) = runner.components::<Transform2d>().next() else {
            panic!("old World should retain its ball");
        };
        assert_eq!(transform.previous_translation(), transform.translation());
        assert!(transform.translation().x() > 1.9);
        let Some((_, camera)) = runner.components::<ActiveCamera2d>().next() else {
            panic!("old World should retain its camera");
        };
        assert_eq!(camera.previous_center(), camera.center());
        assert!(camera.center().x() > 1.9);

        let extracted = runner
            .extracted_frame()
            .ok_or("old World should be extracted after failed preparation")?;
        assert_eq!(extracted.world_generation(), old_generation);
        let [resolved] = extracted.resolved_circles() else {
            panic!("old World should still extract exactly one circle");
        };
        assert_eq!(resolved.position(), transform.translation());
        assert_eq!(extracted.camera().center(), camera.center());
        Ok(())
    }

    #[test]
    fn fixed_failure_restores_previous_endpoint_but_keeps_current_mutation()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let initial_transform = Transform2d::new(Vec2::new(1.0, 0.0))?;
        let world_a = application.register_world("world-a", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, initial_transform, ball))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);
        application.add_fallible_system(Stage::FixedUpdate, follow_ball);
        application.add_system(Stage::FixedUpdate, require_missing_fixed_resource);

        let mut runner = application.build_headless(world_a)?;

        let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FixedUpdate,
                ..
            })
        ));

        let Some((_, transform)) = runner.components::<Transform2d>().next() else {
            panic!("partially updated ball should remain alive");
        };
        assert_eq!(transform.previous_translation(), Vec2::new(1.0, 0.0));
        assert!(transform.translation().x() > 1.9);
        assert_eq!(transform.translation().y(), 0.0);
        let Some((_, camera)) = runner.components::<ActiveCamera2d>().next() else {
            panic!("partially updated camera should remain alive");
        };
        assert_eq!(camera.previous_center(), Vec2::ZERO);
        assert!(camera.center().x() > 1.9);
        assert_eq!(camera.center().y(), 0.0);
        Ok(())
    }

    #[test]
    fn failed_fixed_tick_can_pause_extract_and_resume_retained_work() -> Result<(), Box<dyn Error>>
    {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        config.set_command_limit(1)?;

        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ball>()?;
        application.approve_component::<Ephemeral>()?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;

        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, Transform2d::default(), ball))?;
            world.insert_resource(RejectSecondFixedBatch::default())?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move_ball);
        application.add_fallible_system(Stage::FixedUpdate, follow_ball);
        application.add_system(Stage::FixedUpdate, reject_second_fixed_batch);

        let mut runner = application.build_headless(world)?;
        let movement = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let first = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &movement,
            viewport()?,
        ));
        let FrameOutcome::Advanced(first_report) = first else {
            panic!("first bounded frame should be accepted");
        };
        assert_eq!(first_report.fixed_ticks_attempted(), 1);
        assert!(first_report.failure().is_none());

        let failed = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(25),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(failed_report) = failed else {
            panic!("bounded frame should be accepted");
        };
        assert_eq!(failed_report.fixed_ticks_attempted(), 1);
        assert!(matches!(
            failed_report.failure(),
            Some(FrameFailure::Commands {
                stage: Stage::FixedUpdate,
                ..
            })
        ));
        let (_, failed_camera) = runner
            .components::<ActiveCamera2d>()
            .next()
            .ok_or("failed World should retain its camera")?;
        assert!((failed_camera.previous_center().x() - 0.0).abs() < 0.001);
        assert!((failed_camera.center().x() - 1.2).abs() < 0.001);

        runner.set_paused(true);
        let paused =
            runner.advance_frame(FrameRequest::new(Duration::from_secs(1), &[], viewport()?));
        let FrameOutcome::Advanced(paused_report) = paused else {
            panic!("paused frame should be accepted");
        };
        assert_eq!(paused_report.fixed_ticks_attempted(), 0);
        assert!(paused_report.failure().is_none());
        let paused_frame = runner
            .extracted_frame()
            .ok_or("paused frame should publish an extraction")?;
        let [paused_circle] = paused_frame.resolved_circles() else {
            panic!("paused World should extract exactly one circle");
        };
        assert!((paused_circle.position().x() - 1.2).abs() < 0.001);
        assert!((paused_frame.camera().center().x() - 1.2).abs() < 0.001);

        runner.set_paused(false);
        let resumed = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(resumed_report) = resumed else {
            panic!("resumed frame should be accepted");
        };
        assert_eq!(resumed_report.fixed_ticks_attempted(), 1);
        assert!(resumed_report.failure().is_none());
        let resumed_frame = runner
            .extracted_frame()
            .ok_or("resumed frame should publish an extraction")?;
        let [resumed_circle] = resumed_frame.resolved_circles() else {
            panic!("resumed World should extract exactly one circle");
        };
        assert!((resumed_circle.position().x() - 1.5).abs() < 0.001);
        assert!((resumed_frame.camera().center().x() - 1.5).abs() < 0.001);

        let (_, transform) = runner
            .components::<Transform2d>()
            .next()
            .ok_or("resumed World should retain its transform")?;
        assert!((transform.previous_translation().x() - 1.2).abs() < 0.001);
        assert!((transform.translation().x() - 1.8).abs() < 0.001);
        Ok(())
    }

    #[test]
    fn old_frame_token_is_invalid_while_waiting_for_fixed_delivery() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(StoredFrameIntent(None))?;
            Ok(())
        })?;
        application.add_system(
            Stage::FrameUpdate,
            move |input: FrameInput<TestAction>,
                  mut stored: ResMut<StoredFrameIntent>,
                  mut commands: LogicCommands| {
                if let Some(edge) = input.pressed(TestAction::Enter).next() {
                    stored.0 = Some(edge.intent());
                    return;
                }
                if let Some(intent) = stored.0.take() {
                    assert!(commands.replace_world(intent, target).is_ok());
                }
            },
        );

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport()?));
        let FrameOutcome::Advanced(first_report) = first else {
            panic!("bounded frame should be accepted");
        };
        assert_eq!(first_report.fixed_ticks_attempted(), 0);
        assert!(matches!(first_report.transition(), FrameTransition::None));

        let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(second_report) = second else {
            panic!("bounded frame should be accepted");
        };
        assert_eq!(second_report.fixed_ticks_attempted(), 0);
        assert!(matches!(
            second_report.transition(),
            FrameTransition::Invalid(TransitionRequestFailure::InvalidIntent)
        ));
        assert!(second_report.failure().is_none());
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(runner.lifecycle().len(), 1);
        Ok(())
    }

    #[test]
    fn startup_cannot_issue_application_control_commands_or_a_direct_intent()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.insert_resource(StartupIntentObservation {
                rejected_as_unavailable: false,
            })?;
            world.insert_resource(StartupExitObservation {
                rejected_as_unavailable: false,
            })?;
            world.insert_resource(StartupPauseObservation {
                rejected_as_unavailable: false,
            })?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, observe_startup_intent);
        application.add_system(Stage::Startup, observe_startup_exit);
        application.add_system(Stage::Startup, observe_startup_pause);

        let runner = application.build_headless(world)?;
        let observation = runner
            .resource::<StartupIntentObservation>()
            .ok_or("Startup observation should remain in the active World")?;
        assert!(observation.rejected_as_unavailable);
        let exit_observation = runner
            .resource::<StartupExitObservation>()
            .ok_or("Startup exit observation should remain in the active World")?;
        assert!(exit_observation.rejected_as_unavailable);
        let pause_observation = runner
            .resource::<StartupPauseObservation>()
            .ok_or("Startup pause observation should remain in the active World")?;
        assert!(pause_observation.rejected_as_unavailable);
        Ok(())
    }

    #[test]
    fn startup_cannot_read_frame_input() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, startup_reads_frame_input);

        let result = application.build_headless(world);
        assert!(matches!(
            result,
            Err(super::RunnerBuildError::InitialWorld(
                super::CandidateFailure::StartupSystem(_)
            ))
        ));
        Ok(())
    }

    #[test]
    fn startup_cannot_read_fixed_time() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, startup_reads_fixed_time);

        let result = application.build_headless(world);
        assert!(matches!(
            result,
            Err(super::RunnerBuildError::InitialWorld(
                super::CandidateFailure::StartupSystem(_)
            ))
        ));
        Ok(())
    }

    #[test]
    fn update_stages_only_receive_their_own_input_snapshot() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.insert_resource(StageInputObservation::default())?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, observe_fixed_stage_input);
        application.add_system(Stage::FrameUpdate, observe_frame_stage_input);

        let mut runner = application.build_headless(world)?;
        let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(report.failure().is_none());

        let observation = runner
            .resource::<StageInputObservation>()
            .ok_or("stage input observation should remain available")?;
        assert_eq!(observation.fixed_actual_edges, 1);
        assert_eq!(observation.fixed_foreign_edges, 0);
        assert!(!observation.fixed_foreign_held);
        assert_eq!(observation.frame_actual_edges, 1);
        assert_eq!(observation.frame_foreign_edges, 0);
        assert!(!observation.frame_foreign_held);
        Ok(())
    }

    #[test]
    fn missing_frame_snapshot_fails_closed_without_replacement() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        assert!(
            runner
                .active
                .world
                .remove_resource::<FrameInputState<TestAction>>()
                .is_some()
        );

        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted before invariant validation");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        assert_eq!(report.fixed_ticks_attempted(), 0);
        assert!(
            !runner
                .active
                .world
                .contains_resource::<FrameInputState<TestAction>>()
        );
        Ok(())
    }

    #[test]
    fn missing_direct_intent_issuer_fails_closed_before_update_and_extraction()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        let generation = runner.active.generation;
        assert!(
            runner
                .active
                .world
                .remove_resource::<DirectIntentIssuer>()
                .is_some()
        );

        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted before invariant validation");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        assert_eq!(report.fixed_ticks_attempted(), 0);
        assert_eq!(report.extracted_generation(), None);
        assert_eq!(runner.active.generation, generation);
        Ok(())
    }

    #[test]
    fn missing_command_queue_overrides_fixed_system_failure_as_runtime_invariant()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_time(TimeConfig::new(Duration::from_millis(10), 8)?);
        let mut application = Application::<TestAction>::new(config)?;
        let camera = camera()?;
        let world = application.register_world("missing-fixed-commands", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_fallible_system(Stage::FixedUpdate, || -> Result<(), &'static str> {
            Err("deliberate fixed failure")
        });
        let mut runner = application.build_headless(world)?;
        assert!(
            runner
                .active
                .world
                .remove_resource::<CommandQueue>()
                .is_some()
        );

        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(10),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        assert_eq!(report.fixed_ticks_attempted(), 1);
        Ok(())
    }

    #[test]
    fn missing_command_queue_overrides_frame_system_failure_as_runtime_invariant()
    -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("missing-frame-commands", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_fallible_system(Stage::FrameUpdate, || -> Result<(), &'static str> {
            Err("deliberate frame failure")
        });
        let mut runner = application.build_headless(world)?;
        assert!(
            runner
                .active
                .world
                .remove_resource::<CommandQueue>()
                .is_some()
        );

        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        assert_eq!(report.fixed_ticks_attempted(), 0);
        Ok(())
    }

    #[test]
    fn missing_fixed_snapshot_preflights_frame_publication() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        assert!(
            runner
                .active
                .world
                .remove_resource::<FixedInputState<TestAction>>()
                .is_some()
        );
        let events = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];

        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted before invariant validation");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        let snapshot = runner
            .active
            .world
            .get_resource::<FrameInputState<TestAction>>()
            .ok_or("frame snapshot should still exist")?;
        assert!(!snapshot.held(TestAction::MoveRight));
        assert_eq!(snapshot.pressed(TestAction::MoveRight).count(), 0);
        Ok(())
    }

    #[test]
    fn missing_fixed_snapshot_restores_interpolation_capture() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let circle = circle(Color::WHITE)?;
        let transform = Transform2d::new(Vec2::new(3.0, 0.0))?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, transform, circle))?;
            Ok(())
        })?;
        let mut runner = application.build_headless(world)?;
        let entity = runner
            .components::<Ball>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("ball should be inspectable")?;
        runner
            .active
            .world
            .get_mut::<Transform2d>(entity.entity())
            .ok_or("ball transform should exist")?
            .set_translation(Vec2::new(7.0, 0.0))?;
        assert!(
            runner
                .active
                .world
                .remove_resource::<FixedInputState<TestAction>>()
                .is_some()
        );

        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted before invariant validation");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::RuntimeInvariant)
        ));
        assert_eq!(report.fixed_ticks_attempted(), 1);
        let transform = runner
            .active
            .world
            .get::<Transform2d>(entity.entity())
            .ok_or("ball transform should survive failed delivery")?;
        assert_eq!(transform.previous_translation(), Vec2::new(3.0, 0.0));
        assert_eq!(transform.translation(), Vec2::new(7.0, 0.0));
        Ok(())
    }

    #[test]
    fn frame_update_does_not_run_after_fixed_transition_request() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let target_camera = camera()?;
        let target = application.register_world("target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, request_next_world);
        let frame_runs = Arc::new(AtomicUsize::new(0));
        let observed_frame_runs = Arc::clone(&frame_runs);
        application.add_system(Stage::FrameUpdate, move || {
            observed_frame_runs.fetch_add(1, Ordering::SeqCst);
        });

        let mut runner = application.build_headless(source)?;
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(50),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert!(matches!(
            report.transition(),
            FrameTransition::Committed { .. }
        ));
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert_eq!(frame_runs.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[test]
    fn fixed_exit_stops_catch_up_frame_update_and_extraction() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ball>()?;
        let camera = camera()?;
        let ball = circle(Color::WHITE)?;
        let world = application.register_world("exit-world", move |world| {
            world.spawn(camera)?;
            world.spawn((Ball, ball))?;
            Ok(())
        })?;
        let fixed_runs = Arc::new(AtomicUsize::new(0));
        let observed_fixed_runs = Arc::clone(&fixed_runs);
        application.add_fallible_system(
            Stage::FixedUpdate,
            move |mut balls: Query<&mut Transform2d, With<Ball>>,
                  mut commands: LogicCommands|
                  -> Result<(), Box<dyn Error>> {
                observed_fixed_runs.fetch_add(1, Ordering::SeqCst);
                for mut transform in &mut balls {
                    transform.translate_by(Vec2::new(2.0, 0.0))?;
                }
                commands.request_exit()?;
                Ok(())
            },
        );
        let later_fixed_runs = Arc::new(AtomicUsize::new(0));
        let observed_later_fixed_runs = Arc::clone(&later_fixed_runs);
        application.add_system(Stage::FixedUpdate, move || {
            observed_later_fixed_runs.fetch_add(1, Ordering::SeqCst);
        });
        let frame_runs = Arc::new(AtomicUsize::new(0));
        let observed_frame_runs = Arc::clone(&frame_runs);
        application.add_system(Stage::FrameUpdate, move || {
            observed_frame_runs.fetch_add(1, Ordering::SeqCst);
        });

        let mut runner = application.build_headless(world)?;
        let published_before = runner
            .extracted_frame()
            .ok_or("initial extraction should be published")?
            .world_generation();
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(100),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded exit frame should be accepted");
        };

        assert!(report.exit_requested());
        assert_eq!(report.transitions_suppressed_by_exit(), 0);
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert_eq!(fixed_runs.load(Ordering::SeqCst), 1);
        assert_eq!(later_fixed_runs.load(Ordering::SeqCst), 1);
        assert_eq!(frame_runs.load(Ordering::SeqCst), 0);
        assert_eq!(report.extracted_generation(), None);
        let canonical = runner
            .components::<Transform2d>()
            .find_map(|(_, transform)| (transform.translation().x() == 2.0).then_some(transform))
            .ok_or("exit tick should keep the moved canonical Transform")?;
        assert_eq!(canonical.previous_translation(), canonical.translation());
        assert_eq!(
            runner
                .extracted_frame()
                .ok_or("old extraction should remain diagnostic history")?
                .world_generation(),
            published_before
        );

        let ignored = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(ignored_report) = ignored else {
            panic!("a headless host may deliberately keep driving the runner");
        };
        assert!(!ignored_report.exit_requested());
        assert_eq!(ignored_report.fixed_ticks_attempted(), 0);
        assert_eq!(frame_runs.load(Ordering::SeqCst), 1);
        assert!(ignored_report.extracted_generation().is_some());
        let [circle] = runner
            .extracted_frame()
            .ok_or("continued headless frame should publish")?
            .resolved_circles()
        else {
            panic!("continued headless frame should retain one circle");
        };
        assert_eq!(circle.position(), Vec2::new(2.0, 0.0));
        Ok(())
    }

    #[test]
    fn exit_suppresses_replacement_and_consumes_its_input_edge() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;
        let target_builds = Arc::new(AtomicUsize::new(0));
        let observed_target_builds = Arc::clone(&target_builds);
        let target_camera = camera()?;
        let target = application.register_world("suppressed-target", move |world| {
            observed_target_builds.fetch_add(1, Ordering::SeqCst);
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("exit-source", move |world| {
            world.spawn(source_camera)?;
            world.insert_resource(NextWorld(target))?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, request_next_world);
        let first_tick = Arc::new(AtomicUsize::new(0));
        let observed_first_tick = Arc::clone(&first_tick);
        application.add_fallible_system(
            Stage::FixedUpdate,
            move |mut commands: LogicCommands| -> Result<(), CommandEnqueueError> {
                if observed_first_tick.fetch_add(1, Ordering::SeqCst) == 0 {
                    commands.request_exit()?;
                }
                Ok(())
            },
        );

        let mut runner = application.build_headless(source)?;
        let generation = runner.world_generation();
        let lifecycle_len = runner.lifecycle().len();
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded exit frame should be accepted");
        };

        assert!(report.exit_requested());
        assert_eq!(report.transitions_suppressed_by_exit(), 1);
        assert!(matches!(report.transition(), FrameTransition::None));
        assert_eq!(target_builds.load(Ordering::SeqCst), 0);
        assert_eq!(runner.world_generation(), generation);
        assert_eq!(runner.lifecycle().len(), lifecycle_len);

        let next = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(next_report) = next else {
            panic!("headless host should be able to inspect a later frame");
        };
        assert!(!next_report.exit_requested());
        assert!(matches!(next_report.transition(), FrameTransition::None));
        assert_eq!(target_builds.load(Ordering::SeqCst), 0);
        assert_eq!(runner.world_generation(), generation);
        Ok(())
    }

    #[test]
    fn paused_frame_exit_commits_structure_but_poison_discards_later_exit()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_command_limit(2)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;
        let camera = camera()?;
        let world = application.register_world("frame-exit", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let phase = Arc::new(AtomicUsize::new(0));
        let observed_phase = Arc::clone(&phase);
        application.add_system(Stage::FrameUpdate, move |mut commands: LogicCommands| {
            assert!(commands.request_exit().is_ok());
            assert!(commands.spawn(Ephemeral).is_ok());
            if observed_phase.fetch_add(1, Ordering::SeqCst) != 0 {
                assert!(matches!(
                    commands.spawn(Ephemeral),
                    Err(CommandEnqueueError::LimitExceeded { limit: 2 })
                ));
            }
        });

        let mut runner = application.build_headless(world)?;
        runner.set_paused(true);
        let first =
            runner.advance_frame(FrameRequest::new(Duration::from_secs(1), &[], viewport()?));
        let FrameOutcome::Advanced(first_report) = first else {
            panic!("paused exit frame should be accepted");
        };
        assert!(first_report.exit_requested());
        assert_eq!(first_report.fixed_ticks_attempted(), 0);
        assert_eq!(first_report.spawned(), 1);
        assert_eq!(first_report.extracted_generation(), None);
        assert_eq!(runner.components::<Ephemeral>().count(), 1);

        let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(second_report) = second else {
            panic!("poisoned headless frame should still complete diagnostically");
        };
        assert!(!second_report.exit_requested());
        assert_eq!(second_report.spawned(), 0);
        assert!(matches!(
            second_report.failure(),
            Some(FrameFailure::Commands {
                stage: Stage::FrameUpdate,
                error: crate::commands::CommandBatchError::LimitExceeded { limit: 2 },
            })
        ));
        assert_eq!(runner.components::<Ephemeral>().count(), 1);
        Ok(())
    }

    #[test]
    fn frame_exit_discards_an_edge_not_yet_delivered_to_fixed_update() -> Result<(), Box<dyn Error>>
    {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;
        let camera = camera()?;
        let world = application.register_world("edge-exit", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        let fixed_presses = Arc::new(AtomicUsize::new(0));
        let observed_fixed_presses = Arc::clone(&fixed_presses);
        application.add_system(Stage::FixedUpdate, move |input: FixedInput<TestAction>| {
            observed_fixed_presses
                .fetch_add(input.pressed(TestAction::Enter).count(), Ordering::SeqCst);
        });
        application.add_fallible_system(
            Stage::FrameUpdate,
            |input: FrameInput<TestAction>,
             mut commands: LogicCommands|
             -> Result<(), CommandEnqueueError> {
                if input.pressed(TestAction::Enter).next().is_some() {
                    commands.request_exit()?;
                }
                Ok(())
            },
        );

        let mut runner = application.build_headless(world)?;
        let pressed = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let exit = runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?));
        let FrameOutcome::Advanced(exit_report) = exit else {
            panic!("zero-tick exit frame should be accepted");
        };
        assert!(exit_report.exit_requested());
        assert_eq!(fixed_presses.load(Ordering::SeqCst), 0);

        let continued = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(continued_report) = continued else {
            panic!("continued fixed frame should be accepted");
        };
        assert!(!continued_report.exit_requested());
        assert_eq!(continued_report.fixed_ticks_attempted(), 1);
        assert_eq!(fixed_presses.load(Ordering::SeqCst), 0);
        Ok(())
    }

    #[test]
    fn command_spawned_entity_can_be_resolved_and_despawned_later() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.approve_component::<Ephemeral>()?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            world.insert_resource(SpawnThenDespawn::default())?;
            Ok(())
        })?;
        application.add_system(Stage::FrameUpdate, spawn_then_despawn);

        let mut runner = application.build_headless(world)?;
        let first = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(first_report) = first else {
            panic!("bounded frame should be accepted");
        };
        assert_eq!(first_report.spawned(), 1);
        assert_eq!(runner.components::<Ephemeral>().count(), 1);
        let spawned = runner
            .components::<Ephemeral>()
            .next()
            .map(|(entity, _)| entity)
            .ok_or("first frame should expose the spawned entity")?;
        assert!(runner.component::<Ephemeral>(spawned).is_ok());

        let second = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(second_report) = second else {
            panic!("bounded frame should be accepted");
        };
        assert_eq!(second_report.despawned(), 1);
        assert_eq!(runner.components::<Ephemeral>().count(), 0);
        assert!(matches!(
            runner.component::<Ephemeral>(spawned),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        Ok(())
    }

    #[test]
    fn managed_entity_limit_stays_exact_across_factory_and_stage_barriers()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_entity_limit(3)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.approve_component::<Ephemeral>()?;

        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::Startup, |mut commands: LogicCommands| {
            assert!(commands.spawn(Ephemeral).is_ok());
        });
        application.add_system(
            Stage::FixedUpdate,
            |entities: Query<LogicEntityRef, With<Ephemeral>>, mut commands: LogicCommands| {
                for entity in &entities {
                    assert!(commands.despawn(entity.handle()).is_ok());
                }
                assert!(commands.spawn(Ephemeral).is_ok());
                assert!(commands.spawn(Ephemeral).is_ok());
            },
        );
        application.add_system(Stage::FrameUpdate, |mut commands: LogicCommands| {
            assert!(commands.spawn(Ephemeral).is_ok());
        });

        let mut runner = application.build_headless(world)?;
        assert_eq!(runner.components::<Ephemeral>().count(), 1);

        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert_eq!(report.spawned(), 2);
        assert_eq!(report.despawned(), 1);
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::Commands {
                stage: Stage::FrameUpdate,
                error: crate::commands::CommandBatchError::EntityLimitExceeded {
                    limit: 3,
                    requested: 4,
                },
            })
        ));
        assert_eq!(runner.components::<Ephemeral>().count(), 2);
        assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);
        Ok(())
    }

    #[test]
    fn same_intent_with_different_targets_is_malformed_at_runtime() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Enter)?;

        let first_camera = camera()?;
        let first_target = application.register_world("first-target", move |world| {
            world.spawn(first_camera)?;
            Ok(())
        })?;
        let second_camera = camera()?;
        let second_target = application.register_world("second-target", move |world| {
            world.spawn(second_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("source", move |world| {
            world.spawn(source_camera)?;
            Ok(())
        })?;
        application.add_system(
            Stage::FixedUpdate,
            move |input: FixedInput<TestAction>, mut commands: LogicCommands| {
                for edge in input.pressed(TestAction::Enter) {
                    assert!(commands.replace_world(edge.intent(), first_target).is_ok());
                    assert!(commands.replace_world(edge.intent(), second_target).is_ok());
                }
            },
        );

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let events = [InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )];
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(50),
            &events,
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert!(matches!(
            report.transition(),
            FrameTransition::Rejected(TransitionRejection::MalformedIntent)
        ));
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert!(report.dropped_for_transition().is_some());
        assert_eq!(report.extracted_generation(), Some(old_generation));
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(runner.lifecycle().len(), 1);
        Ok(())
    }

    #[test]
    fn different_intents_with_same_target_commit_with_runtime_warning() -> Result<(), Box<dyn Error>>
    {
        let mut config = AppConfig::default();
        config.set_lifecycle_trace_limit(2)?;
        let mut application = Application::<TestAction>::new(config)?;

        let target_camera = camera()?;
        let target = application.register_world("target", move |world| {
            world.spawn(target_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("source", move |world| {
            world.spawn(source_camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move |mut commands: LogicCommands| {
            let first = commands.new_transition_intent();
            let second = commands.new_transition_intent();
            let (Ok(first), Ok(second)) = (first, second) else {
                panic!("fixed stage should issue two bounded direct intents");
            };
            assert!(commands.replace_world(first, target).is_ok());
            assert!(commands.replace_world(second, target).is_ok());
        });

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        let FrameTransition::Committed {
            old,
            new,
            target: committed_target,
            warning: Some(warning),
        } = report.transition()
        else {
            panic!("convergent direct intents should commit with a warning");
        };
        assert_eq!(*old, old_generation);
        assert_eq!(*new, runner.world_generation());
        assert_eq!(*committed_target, target);
        assert_eq!(warning.distinct_intents(), 2);
        assert!(report.failure().is_none());
        assert_eq!(runner.lifecycle().len(), 2);
        assert_eq!(runner.lifecycle()[0].event(), LifecycleEvent::WorldExit);
        assert_eq!(runner.lifecycle()[1].event(), LifecycleEvent::WorldEnter);
        assert_eq!(runner.dropped_lifecycle_records(), 1);
        Ok(())
    }

    #[test]
    fn different_intents_with_different_targets_conflict_at_runtime() -> Result<(), Box<dyn Error>>
    {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;

        let first_camera = camera()?;
        let first_target = application.register_world("first-target", move |world| {
            world.spawn(first_camera)?;
            Ok(())
        })?;
        let second_camera = camera()?;
        let second_target = application.register_world("second-target", move |world| {
            world.spawn(second_camera)?;
            Ok(())
        })?;
        let source_camera = camera()?;
        let source = application.register_world("source", move |world| {
            world.spawn(source_camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, move |mut commands: LogicCommands| {
            let first = commands.new_transition_intent();
            let second = commands.new_transition_intent();
            let (Ok(first), Ok(second)) = (first, second) else {
                panic!("fixed stage should issue two bounded direct intents");
            };
            assert!(commands.replace_world(first, first_target).is_ok());
            assert!(commands.replace_world(second, second_target).is_ok());
        });

        let mut runner = application.build_headless(source)?;
        let old_generation = runner.world_generation();
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(50),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };

        assert!(matches!(
            report.transition(),
            FrameTransition::Rejected(TransitionRejection::Conflict)
        ));
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert!(report.dropped_for_transition().is_some());
        assert_eq!(report.extracted_generation(), Some(old_generation));
        assert_eq!(runner.world_generation(), old_generation);
        assert_eq!(runner.lifecycle().len(), 1);
        Ok(())
    }

    #[test]
    fn fixed_update_cannot_read_frame_time() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, fixed_reads_frame_time);

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FixedUpdate,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn fixed_update_cannot_read_frame_viewport() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, fixed_reads_frame_viewport);

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FixedUpdate,
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn zero_tick_frames_reject_retained_fixed_edge_overflow_atomically()
    -> Result<(), Box<dyn Error>> {
        let mut config = AppConfig::default();
        config.set_input_event_limit(2)?;
        let mut application = Application::<TestAction>::new(config)?;
        application.bind_key(PhysicalKeyCode::KeyD, TestAction::MoveRight)?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.insert_resource(RetainedEdgeObservation::default())?;
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FixedUpdate, observe_retained_edges);

        let mut runner = application.build_headless(world)?;
        let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
        let released = [InputEvent::key(
            PhysicalKeyCode::KeyD,
            ButtonState::Released,
        )];

        assert!(matches!(
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?)),
            FrameOutcome::Advanced(_)
        ));
        assert!(matches!(
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &released, viewport()?)),
            FrameOutcome::Advanced(_)
        ));

        let rejected =
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?));
        assert!(matches!(
            rejected,
            FrameOutcome::Rejected(BeginFrameRejection::Input(
                InputCollectionError::RetainedFixedEdgeLimitExceeded {
                    limit: 2,
                    retained: 2,
                    incoming: 1,
                }
            ))
        ));

        let delivered = runner.advance_frame(FrameRequest::new(
            Duration::from_millis(17),
            &[],
            viewport()?,
        ));
        let FrameOutcome::Advanced(report) = delivered else {
            panic!("bounded frame should be accepted");
        };
        assert!(report.failure().is_none());
        let Some(observation) = runner.resource::<RetainedEdgeObservation>() else {
            panic!("retained-edge observation should exist");
        };
        assert_eq!(observation.pressed, 1);
        assert_eq!(observation.released, 1);
        assert!(!observation.held);

        assert!(matches!(
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &pressed, viewport()?)),
            FrameOutcome::Advanced(_)
        ));
        Ok(())
    }

    #[test]
    fn frame_update_cannot_read_fixed_time() -> Result<(), Box<dyn Error>> {
        let mut application = Application::<TestAction>::new(AppConfig::default())?;
        let camera = camera()?;
        let world = application.register_world("world", move |world| {
            world.spawn(camera)?;
            Ok(())
        })?;
        application.add_system(Stage::FrameUpdate, frame_reads_fixed_time);

        let mut runner = application.build_headless(world)?;
        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("bounded frame should be accepted");
        };
        assert!(matches!(
            report.failure(),
            Some(FrameFailure::System {
                stage: Stage::FrameUpdate,
                ..
            })
        ));
        Ok(())
    }
}

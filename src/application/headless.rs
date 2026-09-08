//! Renderer-independent application runner and its public frame diagnostics.
//!
//! The runner owns application state; private runtime modules coordinate frame
//! stages, World lifecycle, input publication, and CPU rendering snapshots.

use crate::{
    ExtractedFrame,
    app::RegisteredWorldFactory,
    component::ComponentRegistry,
    events::EventRegistry,
    extraction::ExtractionBuffers,
    identity::{ApplicationId, LogicEntity, ManagedEntity, WorldFactoryId, WorldGeneration},
    input::{Action, InputState},
    query::QueryEntityError,
    resources::ApplicationResourceRegistry,
    system::StageFactories,
    time::{TimeConfigurationError, TimeState},
};

#[path = "runtime/mod.rs"]
mod runtime;

use runtime::RuntimeWorld;
pub use runtime::reports::{
    BeginFrameRejection, CandidateFailure, FrameFailure, FrameOutcome, FrameRequest,
    FrameTransition, LifecycleEvent, LifecycleRecord, LogicFrameReport, RunnerBuildError,
    TransitionRequestFailure,
};

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
    /// the standard Bevy [`Disabled`](bevy_ecs::entity_disabling::Disabled)
    /// component. Their order is not a
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
            self.active.snap_interpolation();
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
}

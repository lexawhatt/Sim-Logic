//! Runner construction, isolated World candidates, and bounded lifecycle history.

use std::time::Duration;

use bevy_ecs::world::World;

use crate::{
    app::{Application, RegisteredWorldFactory},
    commands::{CommandCommitError, CommandQueue, DirectIntentIssuer},
    component::ComponentRegistry,
    extraction::ExtractionBuffers,
    headless::HeadlessRunner,
    identity::{ApplicationId, WorldFactoryId, WorldGeneration, WorldIdentityState},
    input::{Action, FixedInputState, FrameInputState, InputState},
    system::{Stage, StageFactories, SystemSetupError},
    time::{FixedTimeState, FrameTimeState, TimeState},
    visual::WorldBackground,
    world::WorldBuilder,
};

use super::{
    RuntimeWorld,
    barriers::{StageExecutionFailure, apply_command_queue, discard_command_queue, run_stage},
    rendering::{ExtractionQueries, InterpolationQueries, snap_interpolation, stage_runtime_world},
    reports::{CandidateFailure, LifecycleEvent, LifecycleRecord, RunnerBuildError},
};

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
            images: application.images,
            #[cfg(feature = "text")]
            texts: application.texts,
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

    pub(super) fn prepare_world(
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
            &self.images,
            #[cfg(feature = "text")]
            &self.texts,
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

    pub(super) fn record_lifecycle(
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

pub(super) fn factory_exists(
    application: ApplicationId,
    factories: &[RegisteredWorldFactory],
    factory: WorldFactoryId,
) -> bool {
    factory.application() == application
        && factories
            .get(factory.slot() as usize)
            .is_some_and(|registered| registered.id == factory)
}

//! Shared event lifetime and atomic command boundaries for every stage.

use bevy_ecs::{prelude::Mut, world::World};

use crate::{
    commands::{CommandCommitError, CommandQueue, CommittedCommands},
    component::ApprovedComponents,
    events::{begin_event_stage, end_event_stage},
    identity::{ApplicationId, TransitionIntentToken, WorldGeneration},
    system::{SequentialStage, SystemRunFailure},
    transition::TransitionRequest,
};

pub(super) enum StageExecutionFailure {
    System(SystemRunFailure),
    RuntimeInvariant,
}

pub(super) fn run_stage(
    stage: &mut SequentialStage,
    world: &mut World,
) -> Result<(), StageExecutionFailure> {
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

pub(super) fn apply_command_queue(
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

pub(super) fn discard_command_queue(world: &mut World) -> Option<Vec<TransitionIntentToken>> {
    world.get_resource_mut::<CommandQueue>().map(|mut queue| {
        let tokens = queue
            .transition_requests()
            .map(TransitionRequest::intent)
            .collect();
        queue.discard();
        tokens
    })
}

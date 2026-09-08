//! Bounded managed structural, World-transition, and application-control commands.

use std::{
    collections::{HashMap, HashSet},
    error::Error,
    fmt,
};

use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    component::ComponentId,
    entity::Entity,
    entity_disabling::Disabled,
    prelude::{ResMut, Resource},
    system::SystemParam,
    world::World,
};

use crate::{
    component::{ApprovedComponents, ComponentApprovalError},
    identity::{
        ApplicationId, LogicEntity, ManagedEntity, TransitionIntentToken, WorldFactoryId,
        WorldGeneration,
    },
    transition::TransitionRequest,
    visual::{ActiveCamera2d, Transform2d},
};

/// Failure to create an intent or request another bounded stage command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandEnqueueError {
    /// The configured command-count limit has been reached.
    LimitExceeded {
        /// Frozen maximum command count for one stage.
        limit: usize,
    },
    /// The queue could not reserve storage for one more command.
    AllocationFailed,
    /// The direct intent occurrence counter is exhausted.
    IntentIdentityExhausted,
    /// Direct intents are unavailable outside an active application frame.
    IntentUnavailable,
    /// Application exit cannot be requested during isolated World Startup.
    ExitUnavailable,
    /// Fixed simulation pause cannot be changed during isolated World Startup.
    PauseUnavailable,
}

impl fmt::Display for CommandEnqueueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { limit } => {
                write!(formatter, "stage command limit of {limit} was exceeded")
            }
            Self::AllocationFailed => {
                formatter.write_str("failed to reserve storage for a stage command")
            }
            Self::IntentIdentityExhausted => {
                formatter.write_str("direct transition intent identity space is exhausted")
            }
            Self::IntentUnavailable => {
                formatter.write_str("direct transition intents are unavailable in this stage")
            }
            Self::ExitUnavailable => {
                formatter.write_str("application exit is unavailable during World Startup")
            }
            Self::PauseUnavailable => {
                formatter.write_str("fixed simulation pause is unavailable during World Startup")
            }
        }
    }
}

impl Error for CommandEnqueueError {}

/// Reason an entire structural stage batch was rejected before entity mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CommandBatchError {
    /// At least one System attempted to exceed the configured command limit.
    LimitExceeded {
        /// Frozen maximum command count for one stage.
        limit: usize,
    },
    /// Command queue storage could not be reserved.
    AllocationFailed,
    /// A structural-command target belongs to another application or World generation.
    ForeignEntity {
        /// Rejected opaque entity handle.
        entity: LogicEntity,
    },
    /// A structural-command target no longer names a live entity.
    MissingEntity {
        /// Rejected opaque entity handle.
        entity: LogicEntity,
    },
    /// The same entity was scheduled for despawn more than once.
    DuplicateDespawn {
        /// Duplicated opaque entity handle.
        entity: LogicEntity,
    },
    /// One stage batch attempted to structurally change and despawn the same entity.
    ConflictingEntityCommands {
        /// Conflicted opaque entity handle.
        entity: LogicEntity,
    },
    /// Applying every spawn would exceed the active-entity limit.
    EntityLimitExceeded {
        /// Frozen maximum live entity count.
        limit: usize,
        /// Live entity count the batch would produce.
        requested: usize,
    },
    /// A retained component would lose one of its registered required components.
    RequiredComponentWouldBeMissing {
        /// Rejected opaque entity handle.
        entity: LogicEntity,
        /// Diagnostic name of the component that would be absent.
        required: String,
        /// Diagnostic name of the retained component that requires it.
        required_by: String,
    },
    /// A structural bundle fell outside the frozen approved component set.
    Component(ComponentApprovalError),
}

impl fmt::Display for CommandBatchError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { limit } => {
                write!(formatter, "stage command limit of {limit} was exceeded")
            }
            Self::AllocationFailed => {
                formatter.write_str("failed to reserve command validation storage")
            }
            Self::ForeignEntity { entity } => {
                write!(formatter, "entity {entity:?} belongs to another World")
            }
            Self::MissingEntity { entity } => {
                write!(formatter, "entity {entity:?} is not alive")
            }
            Self::DuplicateDespawn { entity } => {
                write!(
                    formatter,
                    "entity {entity:?} was scheduled for despawn twice"
                )
            }
            Self::ConflictingEntityCommands { entity } => write!(
                formatter,
                "entity {entity:?} cannot be structurally changed and despawned in one stage"
            ),
            Self::EntityLimitExceeded { limit, requested } => write!(
                formatter,
                "structural batch would create {requested} live entities, above limit {limit}"
            ),
            Self::RequiredComponentWouldBeMissing {
                entity,
                required,
                required_by,
            } => write!(
                formatter,
                "entity {entity:?} would retain {required_by} without required component {required}"
            ),
            Self::Component(error) => write!(formatter, "structural bundle was rejected: {error}"),
        }
    }
}

impl Error for CommandBatchError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Component(error) => Some(error),
            _ => None,
        }
    }
}

trait ErasedSpawn: Send + Sync + 'static {
    fn validate(
        &self,
        world: &mut World,
        approved: &mut ApprovedComponents,
    ) -> Result<(), ComponentApprovalError>;

    fn apply(self: Box<Self>, world: &mut World, generation: WorldGeneration);
}

trait ErasedInsert: Send + Sync + 'static {
    fn validate(
        &self,
        world: &mut World,
        approved: &mut ApprovedComponents,
    ) -> Result<(), ComponentApprovalError>;

    fn record_presence(
        &self,
        world: &mut World,
        entity: Entity,
        overrides: &mut PresenceOverrides,
        inserted_components: &mut Vec<(Entity, ComponentId)>,
    ) -> Result<(), CommandBatchError>;

    fn apply(self: Box<Self>, world: &mut World, entity: Entity);
}

type PresenceOverrides = HashMap<(Entity, ComponentId), bool>;
type RemoveValidation =
    fn(&mut World, &mut ApprovedComponents) -> Result<(), ComponentApprovalError>;
type PresenceRecorder = fn(
    &mut World,
    Entity,
    &mut PresenceOverrides,
    &mut Vec<(Entity, ComponentId)>,
) -> Result<(), CommandBatchError>;
type RemoveApply = fn(&mut World, Entity);

#[derive(Clone, Copy)]
struct RemoveDescriptor {
    validate: RemoveValidation,
    record_presence: PresenceRecorder,
    apply: RemoveApply,
}

impl RemoveDescriptor {
    const fn new<B>() -> Self
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        Self {
            validate: validate_remove_bundle::<B>,
            record_presence: record_removed_bundle::<B>,
            apply: apply_remove_bundle::<B>,
        }
    }
}

fn validate_remove_bundle<B>(
    world: &mut World,
    approved: &mut ApprovedComponents,
) -> Result<(), ComponentApprovalError>
where
    B: Bundle<Effect: NoBundleEffect>,
{
    approved.validate_bundle::<B>(world)
}

fn record_removed_bundle<B>(
    world: &mut World,
    entity: Entity,
    overrides: &mut PresenceOverrides,
    _inserted_components: &mut Vec<(Entity, ComponentId)>,
) -> Result<(), CommandBatchError>
where
    B: Bundle<Effect: NoBundleEffect>,
{
    let bundle = world.register_bundle::<B>();
    overrides
        .try_reserve(bundle.explicit_components().len())
        .map_err(|_| CommandBatchError::AllocationFailed)?;
    for component in bundle.explicit_components() {
        overrides.insert((entity, *component), false);
    }
    Ok(())
}

fn apply_remove_bundle<B>(world: &mut World, entity: Entity)
where
    B: Bundle<Effect: NoBundleEffect>,
{
    world.entity_mut(entity).remove::<B>();
}

struct TypedSpawn<B>(B);

impl<B> ErasedSpawn for TypedSpawn<B>
where
    B: Bundle<Effect: NoBundleEffect>,
{
    fn validate(
        &self,
        world: &mut World,
        approved: &mut ApprovedComponents,
    ) -> Result<(), ComponentApprovalError> {
        approved.validate_bundle::<B>(world)
    }

    fn apply(self: Box<Self>, world: &mut World, generation: WorldGeneration) {
        let _identity = apply_typed_spawn(world, self.0, generation);
    }
}

struct TypedSpawnCopies<B> {
    bundle: B,
    count: usize,
}

impl<B> ErasedSpawn for TypedSpawnCopies<B>
where
    B: Bundle<Effect: NoBundleEffect> + Copy,
{
    fn validate(
        &self,
        world: &mut World,
        approved: &mut ApprovedComponents,
    ) -> Result<(), ComponentApprovalError> {
        approved.validate_bundle::<B>(world)
    }

    fn apply(self: Box<Self>, world: &mut World, generation: WorldGeneration) {
        for _ in 0..self.count {
            let _identity = apply_typed_spawn(world, self.bundle, generation);
        }
    }
}

fn apply_typed_spawn<B>(world: &mut World, bundle: B, generation: WorldGeneration) -> LogicEntity
where
    B: Bundle<Effect: NoBundleEffect>,
{
    let managed = ManagedEntity::for_generation(generation);
    let entity = world.spawn((bundle, managed)).id();
    let identity = managed.handle(entity);
    let mut entity = world.entity_mut(entity);
    if let Some(mut transform) = entity.get_mut::<Transform2d>() {
        transform.snap_interpolation();
    }
    if let Some(mut camera) = entity.get_mut::<ActiveCamera2d>() {
        camera.snap_interpolation();
    }
    identity
}

struct TypedInsert<B>(B);

impl<B> ErasedInsert for TypedInsert<B>
where
    B: Bundle<Effect: NoBundleEffect>,
{
    fn validate(
        &self,
        world: &mut World,
        approved: &mut ApprovedComponents,
    ) -> Result<(), ComponentApprovalError> {
        approved.validate_bundle::<B>(world)
    }

    fn record_presence(
        &self,
        world: &mut World,
        entity: Entity,
        overrides: &mut PresenceOverrides,
        inserted_components: &mut Vec<(Entity, ComponentId)>,
    ) -> Result<(), CommandBatchError> {
        let bundle = world.register_bundle::<B>();
        overrides
            .try_reserve(bundle.contributed_components().len())
            .map_err(|_| CommandBatchError::AllocationFailed)?;
        inserted_components
            .try_reserve(bundle.contributed_components().len())
            .map_err(|_| CommandBatchError::AllocationFailed)?;
        for component in bundle.contributed_components() {
            overrides.insert((entity, *component), true);
            inserted_components.push((entity, *component));
        }
        Ok(())
    }

    fn apply(self: Box<Self>, world: &mut World, entity: Entity) {
        let transform = world.component_id::<Transform2d>();
        let camera = world.component_id::<ActiveCamera2d>();
        let (explicit_transform, required_transform, explicit_camera, required_camera) = {
            let info = world.register_bundle::<B>();
            (
                transform.is_some_and(|id| info.explicit_components().contains(&id)),
                transform.is_some_and(|id| info.required_components().contains(&id)),
                camera.is_some_and(|id| info.explicit_components().contains(&id)),
                camera.is_some_and(|id| info.required_components().contains(&id)),
            )
        };
        let target = world.entity(entity);
        let snap_transform = explicit_transform
            || required_transform && transform.is_some_and(|id| !target.contains_id(id));
        let snap_camera =
            explicit_camera || required_camera && camera.is_some_and(|id| !target.contains_id(id));

        let mut target = world.entity_mut(entity);
        target.insert(self.0);
        if snap_transform && let Some(mut transform) = target.get_mut::<Transform2d>() {
            transform.snap_interpolation();
        }
        if snap_camera && let Some(mut camera) = target.get_mut::<ActiveCamera2d>() {
            camera.snap_interpolation();
        }
    }
}

enum QueuedCommand {
    Spawn {
        count: usize,
        operation: Box<dyn ErasedSpawn>,
    },
    Insert {
        entity: LogicEntity,
        bundle: Box<dyn ErasedInsert>,
    },
    Remove {
        entity: LogicEntity,
        descriptor: RemoveDescriptor,
    },
    Despawn(LogicEntity),
    ReplaceWorld(TransitionRequest),
    SetPaused(bool),
    Exit,
}

impl QueuedCommand {
    fn spawn<B>(bundle: B) -> Self
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        Self::Spawn {
            count: 1,
            operation: Box::new(TypedSpawn(bundle)),
        }
    }
}

#[derive(Default)]
struct CommandCounts {
    spawns: usize,
    removals: usize,
    despawns: usize,
    transitions: usize,
    pause: Option<bool>,
    exit_requested: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QueuePoison {
    LimitExceeded,
    AllocationFailed,
}

#[derive(Resource)]
pub(crate) struct CommandQueue {
    commands: Vec<QueuedCommand>,
    used_slots: usize,
    despawns: HashSet<LogicEntity>,
    component_targets: HashSet<Entity>,
    component_target_order: Vec<Entity>,
    inserted_components: Vec<(Entity, ComponentId)>,
    presence_overrides: PresenceOverrides,
    limit: usize,
    poison: Option<QueuePoison>,
}

impl CommandQueue {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            commands: Vec::new(),
            used_slots: 0,
            despawns: HashSet::new(),
            component_targets: HashSet::new(),
            component_target_order: Vec::new(),
            inserted_components: Vec::new(),
            presence_overrides: HashMap::new(),
            limit,
            poison: None,
        }
    }

    fn push(&mut self, command: QueuedCommand) -> Result<(), CommandEnqueueError> {
        self.push_weighted(command, 1)
    }

    fn push_weighted(
        &mut self,
        command: QueuedCommand,
        slots: usize,
    ) -> Result<(), CommandEnqueueError> {
        let requested = self.prepare_push(slots)?;
        self.commands.push(command);
        self.used_slots = requested;
        Ok(())
    }

    fn push_spawn_copies<B>(&mut self, bundle: B, count: usize) -> Result<(), CommandEnqueueError>
    where
        B: Bundle<Effect: NoBundleEffect> + Copy,
    {
        let requested = self.prepare_push(count)?;
        self.commands.push(QueuedCommand::Spawn {
            count,
            operation: Box::new(TypedSpawnCopies { bundle, count }),
        });
        self.used_slots = requested;
        Ok(())
    }

    fn prepare_push(&mut self, slots: usize) -> Result<usize, CommandEnqueueError> {
        debug_assert_ne!(slots, 0);
        let Some(requested) = self.used_slots.checked_add(slots) else {
            self.poison = Some(QueuePoison::LimitExceeded);
            return Err(CommandEnqueueError::LimitExceeded { limit: self.limit });
        };
        if requested > self.limit {
            self.poison = Some(QueuePoison::LimitExceeded);
            return Err(CommandEnqueueError::LimitExceeded { limit: self.limit });
        }
        if self.commands.try_reserve(1).is_err() {
            self.poison = Some(QueuePoison::AllocationFailed);
            return Err(CommandEnqueueError::AllocationFailed);
        }
        Ok(requested)
    }

    pub(crate) fn transition_requests(&self) -> impl Iterator<Item = TransitionRequest> + '_ {
        self.commands.iter().filter_map(|command| match command {
            QueuedCommand::ReplaceWorld(request) => Some(*request),
            QueuedCommand::Spawn { .. }
            | QueuedCommand::Insert { .. }
            | QueuedCommand::Remove { .. }
            | QueuedCommand::Despawn(_)
            | QueuedCommand::SetPaused(_)
            | QueuedCommand::Exit => None,
        })
    }

    pub(crate) fn discard(&mut self) {
        self.clear_reusing_storage();
    }

    pub(crate) fn validate_and_apply(
        &mut self,
        world: &mut World,
        approved: &mut ApprovedComponents,
        application: ApplicationId,
        generation: WorldGeneration,
        managed_entities: usize,
        entity_limit: usize,
    ) -> Result<CommittedCommands, CommandCommitError> {
        let result = self.validate_and_apply_inner(
            world,
            approved,
            application,
            generation,
            managed_entities,
            entity_limit,
        );
        self.clear_reusing_storage();
        result
    }

    fn validate_and_apply_inner(
        &mut self,
        world: &mut World,
        approved: &mut ApprovedComponents,
        application: ApplicationId,
        generation: WorldGeneration,
        managed_entities: usize,
        entity_limit: usize,
    ) -> Result<CommittedCommands, CommandCommitError> {
        match self.poison {
            Some(QueuePoison::LimitExceeded) => {
                return Err(CommandBatchError::LimitExceeded { limit: self.limit }.into());
            }
            Some(QueuePoison::AllocationFailed) => {
                return Err(CommandBatchError::AllocationFailed.into());
            }
            None => {}
        }

        let counts = self
            .commands
            .iter()
            .fold(CommandCounts::default(), |mut counts, command| {
                match command {
                    QueuedCommand::Spawn { count, .. } => counts.spawns += count,
                    QueuedCommand::Insert { .. } => {}
                    QueuedCommand::Remove { .. } => counts.removals += 1,
                    QueuedCommand::Despawn(_) => counts.despawns += 1,
                    QueuedCommand::ReplaceWorld(_) => counts.transitions += 1,
                    QueuedCommand::SetPaused(paused) => counts.pause = Some(*paused),
                    QueuedCommand::Exit => counts.exit_requested = true,
                }
                counts
            });

        if counts.despawns != 0 {
            self.despawns
                .try_reserve(counts.despawns)
                .map_err(|_| CommandBatchError::AllocationFailed)?;

            for command in &self.commands {
                if let QueuedCommand::Despawn(entity) = command {
                    validate_target(world, application, generation, *entity)?;
                    if !self.despawns.insert(*entity) {
                        return Err(CommandBatchError::DuplicateDespawn { entity: *entity }.into());
                    }
                }
            }
        }

        for command in &self.commands {
            match command {
                QueuedCommand::Spawn { operation, .. } => operation
                    .validate(world, approved)
                    .map_err(CommandBatchError::Component)?,
                QueuedCommand::Insert { entity, bundle } => {
                    validate_target(world, application, generation, *entity)?;
                    if self.despawns.contains(entity) {
                        return Err(CommandBatchError::ConflictingEntityCommands {
                            entity: *entity,
                        }
                        .into());
                    }
                    bundle
                        .validate(world, approved)
                        .map_err(CommandBatchError::Component)?;
                }
                QueuedCommand::Remove { entity, descriptor } => {
                    validate_target(world, application, generation, *entity)?;
                    if self.despawns.contains(entity) {
                        return Err(CommandBatchError::ConflictingEntityCommands {
                            entity: *entity,
                        }
                        .into());
                    }
                    (descriptor.validate)(world, approved).map_err(CommandBatchError::Component)?;
                }
                QueuedCommand::Despawn(_) => {}
                QueuedCommand::ReplaceWorld(_) => {}
                QueuedCommand::SetPaused(_) => {}
                QueuedCommand::Exit => {}
            }
        }

        if counts.removals != 0 {
            self.plan_component_presence(world, counts.removals)?;
            validate_required_components(
                world,
                application,
                generation,
                &self.component_target_order,
                &self.inserted_components,
                &self.presence_overrides,
            )?;
        }

        let retained = managed_entities
            .checked_sub(counts.despawns)
            .ok_or(CommandCommitError::RuntimeInvariant)?;
        let requested =
            retained
                .checked_add(counts.spawns)
                .ok_or(CommandBatchError::EntityLimitExceeded {
                    limit: entity_limit,
                    requested: usize::MAX,
                })?;
        if requested > entity_limit {
            return Err(CommandBatchError::EntityLimitExceeded {
                limit: entity_limit,
                requested,
            }
            .into());
        }

        let mut spawned = 0;
        let mut despawned = 0;
        let mut transitions = Vec::new();
        transitions
            .try_reserve(counts.transitions)
            .map_err(|_| CommandBatchError::AllocationFailed)?;

        for command in self.commands.drain(..) {
            match command {
                QueuedCommand::Spawn { count, operation } => {
                    operation.apply(world, generation);
                    spawned += count;
                }
                QueuedCommand::Insert { entity, bundle } => {
                    bundle.apply(world, entity.entity());
                }
                QueuedCommand::Remove { entity, descriptor } => {
                    (descriptor.apply)(world, entity.entity());
                }
                QueuedCommand::Despawn(entity) => {
                    let removed = world.despawn(entity.entity());
                    debug_assert!(
                        removed,
                        "validated entity disappeared during command commit"
                    );
                    despawned += 1;
                }
                QueuedCommand::ReplaceWorld(request) => transitions.push(request),
                QueuedCommand::SetPaused(_) => {}
                QueuedCommand::Exit => {}
            }
        }

        Ok(CommittedCommands {
            spawned,
            despawned,
            live_entities: requested,
            transitions,
            pause: counts.pause,
            exit_requested: counts.exit_requested,
        })
    }

    fn plan_component_presence(
        &mut self,
        world: &mut World,
        removal_count: usize,
    ) -> Result<(), CommandBatchError> {
        debug_assert_ne!(removal_count, 0);
        self.component_targets
            .try_reserve(removal_count)
            .map_err(|_| CommandBatchError::AllocationFailed)?;
        self.component_target_order
            .try_reserve(removal_count)
            .map_err(|_| CommandBatchError::AllocationFailed)?;

        for command in &self.commands {
            if let QueuedCommand::Remove { entity, .. } = command {
                let raw = entity.entity();
                if self.component_targets.insert(raw) {
                    self.component_target_order.push(raw);
                }
            }
        }

        for command in &self.commands {
            match command {
                QueuedCommand::Insert { entity, bundle }
                    if self.component_targets.contains(&entity.entity()) =>
                {
                    let raw = entity.entity();
                    bundle.record_presence(
                        world,
                        raw,
                        &mut self.presence_overrides,
                        &mut self.inserted_components,
                    )?;
                }
                QueuedCommand::Remove { entity, descriptor } => {
                    let raw = entity.entity();
                    (descriptor.record_presence)(
                        world,
                        raw,
                        &mut self.presence_overrides,
                        &mut self.inserted_components,
                    )?;
                }
                QueuedCommand::Insert { .. }
                | QueuedCommand::Spawn { .. }
                | QueuedCommand::Despawn(_)
                | QueuedCommand::ReplaceWorld(_)
                | QueuedCommand::SetPaused(_)
                | QueuedCommand::Exit => {}
            }
        }
        Ok(())
    }

    fn clear_reusing_storage(&mut self) {
        self.commands.clear();
        self.used_slots = 0;
        self.despawns.clear();
        self.component_targets.clear();
        self.component_target_order.clear();
        self.inserted_components.clear();
        self.presence_overrides.clear();
        self.poison = None;
    }

    #[cfg(test)]
    fn command_capacity(&self) -> usize {
        self.commands.capacity()
    }

    #[cfg(test)]
    fn despawn_capacity(&self) -> usize {
        self.despawns.capacity()
    }

    #[cfg(test)]
    fn component_target_capacity(&self) -> usize {
        self.component_targets.capacity()
    }

    #[cfg(test)]
    fn presence_override_capacity(&self) -> usize {
        self.presence_overrides.capacity()
    }

    #[cfg(test)]
    fn is_clean(&self) -> bool {
        self.commands.is_empty()
            && self.used_slots == 0
            && self.despawns.is_empty()
            && self.component_targets.is_empty()
            && self.component_target_order.is_empty()
            && self.inserted_components.is_empty()
            && self.presence_overrides.is_empty()
            && self.poison.is_none()
    }
}

fn validate_required_components(
    world: &World,
    application: ApplicationId,
    generation: WorldGeneration,
    component_targets: &[Entity],
    inserted_components: &[(Entity, ComponentId)],
    overrides: &PresenceOverrides,
) -> Result<(), CommandBatchError> {
    for entity in component_targets {
        let target = world.entity(*entity);
        for component in target.archetype().iter_components() {
            if final_component_presence(world, *entity, component, overrides) {
                validate_retained_requirements(
                    world,
                    application,
                    generation,
                    *entity,
                    component,
                    overrides,
                )?;
            }
        }
    }

    for &(entity, component) in inserted_components {
        if final_component_presence(world, entity, component, overrides) {
            validate_retained_requirements(
                world,
                application,
                generation,
                entity,
                component,
                overrides,
            )?;
        }
    }
    Ok(())
}

fn validate_retained_requirements(
    world: &World,
    application: ApplicationId,
    generation: WorldGeneration,
    entity: Entity,
    component: ComponentId,
    overrides: &PresenceOverrides,
) -> Result<(), CommandBatchError> {
    let Some(required) = world.get_required_components_by_id(component) else {
        return Ok(());
    };
    let Some(missing) = required
        .iter_ids()
        .find(|required| !final_component_presence(world, entity, *required, overrides))
    else {
        return Ok(());
    };

    Err(CommandBatchError::RequiredComponentWouldBeMissing {
        entity: LogicEntity::new(application, generation, entity),
        required: component_diagnostic_name(world, missing),
        required_by: component_diagnostic_name(world, component),
    })
}

fn final_component_presence(
    world: &World,
    entity: Entity,
    component: ComponentId,
    overrides: &PresenceOverrides,
) -> bool {
    overrides
        .get(&(entity, component))
        .copied()
        .unwrap_or_else(|| world.entity(entity).contains_id(component))
}

fn component_diagnostic_name(world: &World, component: ComponentId) -> String {
    world
        .components()
        .get_info(component)
        .map(|info| info.name().to_string())
        .unwrap_or_else(|| format!("{component:?}"))
}

fn validate_target(
    world: &World,
    application: ApplicationId,
    generation: WorldGeneration,
    entity: LogicEntity,
) -> Result<(), CommandBatchError> {
    if entity.application() != application || entity.world_generation() != generation {
        return Err(CommandBatchError::ForeignEntity { entity });
    }
    if !world
        .get::<ManagedEntity>(entity.entity())
        .is_some_and(|managed| managed.handle(entity.entity()) == entity)
    {
        return Err(CommandBatchError::MissingEntity { entity });
    }
    Ok(())
}

#[derive(Resource)]
pub(crate) struct DirectIntentIssuer {
    application: ApplicationId,
    generation: WorldGeneration,
    frame: u64,
    next_occurrence: u64,
    enabled: bool,
}

impl DirectIntentIssuer {
    pub(crate) const fn new(application: ApplicationId, generation: WorldGeneration) -> Self {
        Self {
            application,
            generation,
            frame: 0,
            next_occurrence: 1,
            enabled: false,
        }
    }

    pub(crate) fn set_frame(&mut self, frame: u64, generation: WorldGeneration) {
        self.frame = frame;
        self.generation = generation;
        self.enabled = true;
    }

    pub(crate) fn disable(&mut self) {
        self.enabled = false;
    }

    fn issue(&mut self) -> Result<TransitionIntentToken, CommandEnqueueError> {
        if !self.enabled {
            return Err(CommandEnqueueError::IntentUnavailable);
        }
        let occurrence = self.next_occurrence;
        self.next_occurrence = self
            .next_occurrence
            .checked_add(1)
            .ok_or(CommandEnqueueError::IntentIdentityExhausted)?;
        Ok(TransitionIntentToken::direct(
            self.application,
            self.generation,
            self.frame,
            occurrence,
        ))
    }

    const fn application_control_is_available(&self) -> bool {
        self.enabled
    }
}

/// Managed, stage-deferred command access for ordinary Sim;Logic Systems.
///
/// Commands are retained in deterministic call order and become visible only
/// after the current stage passes whole-batch validation.
#[derive(SystemParam)]
pub struct LogicCommands<'w> {
    queue: ResMut<'w, CommandQueue>,
    intents: ResMut<'w, DirectIntentIssuer>,
}

impl LogicCommands<'_> {
    /// Queues one approved, side-effect-free bundle for deferred spawn.
    ///
    /// Approved required components are materialized at the stage barrier. An
    /// explicit value in `bundle` takes precedence over its required default.
    ///
    /// No entity handle is returned. The runtime assigns it at the stage
    /// barrier after the complete batch succeeds.
    pub fn spawn<B>(&mut self, bundle: B) -> Result<(), CommandEnqueueError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.queue.push(QueuedCommand::spawn(bundle))
    }

    /// Queues `count` copies of one approved bundle for consecutive deferred spawn.
    ///
    /// Every eventual entity consumes one slot from the bounded stage command
    /// budget even though the runtime retains only one erased bundle template.
    /// A request that does not fit poisons the complete stage batch just like
    /// repeated [`Self::spawn`] calls that cross the limit, and no prefix from
    /// this call is retained. At the stage barrier the bundle type is validated
    /// once, the entity limit is checked for every copy before mutation, and
    /// then distinct managed entities are created consecutively at this
    /// command's position in queue order.
    ///
    /// `count == 0` is an immediate successful no-op: it occupies no command
    /// slot, retains no value, and does not validate or register `B`. Required
    /// components, explicit-over-required precedence, Disabled accounting, and
    /// Transform or camera interpolation snapping otherwise match singular
    /// spawn exactly. No handles are returned because identities do not become
    /// live until the stage barrier.
    ///
    /// `Copy` deliberately excludes user-defined cloning work and destructors.
    /// Application remains linear in `count`; this method does not promise a
    /// native bulk ECS insertion.
    pub fn spawn_copies<B>(&mut self, bundle: B, count: usize) -> Result<(), CommandEnqueueError>
    where
        B: Bundle<Effect: NoBundleEffect> + Copy,
    {
        if count == 0 {
            return Ok(());
        }
        self.queue.push_spawn_copies(bundle, count)
    }

    /// Queues approved components to be added to or replaced on one entity.
    ///
    /// Existing explicit component types are replaced. Required components are
    /// materialized only when absent. The target and complete bundle are
    /// validated at the stage barrier before any structural command is
    /// applied. Inserting into and despawning the same entity in one stage is
    /// a batch conflict regardless of enqueue order.
    ///
    /// Multiple inserts apply in call order, so the last explicit value of one
    /// component type wins. An explicit or newly required [`Transform2d`] or
    /// [`ActiveCamera2d`] starts snapped; a required Transform that already
    /// exists keeps its interpolation history. The standard Bevy `Disabled`
    /// component may also be inserted directly, though [`Self::disable`] names
    /// that whole-entity operation more clearly.
    pub fn insert<B>(&mut self, entity: LogicEntity, bundle: B) -> Result<(), CommandEnqueueError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.queue.push(QueuedCommand::Insert {
            entity,
            bundle: Box::new(TypedInsert(bundle)),
        })
    }

    /// Queues approved explicit component types for removal from one entity.
    ///
    /// Removal is visible only after the current stage barrier. Required
    /// components are never removed implicitly, and naming an approved
    /// component that is absent is a successful no-op. Insert and remove
    /// commands apply in call order, but removing a component still required
    /// by the entity's final component set rejects the complete structural
    /// batch before mutation. Removing and despawning the same entity in one
    /// stage is likewise a batch conflict.
    ///
    /// Removing the standard Bevy `Disabled` component re-enables the entity
    /// after the barrier, though [`Self::enable`] names that operation more
    /// clearly. Retain its [`LogicEntity`] handle before disabling it: ordinary
    /// managed queries deliberately exclude disabled entities.
    pub fn remove<B>(&mut self, entity: LogicEntity) -> Result<(), CommandEnqueueError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.queue.push(QueuedCommand::Remove {
            entity,
            descriptor: RemoveDescriptor::new::<B>(),
        })
    }

    /// Queues an entity to become inactive after the current stage barrier.
    ///
    /// The entity keeps its identity, components, interpolation history, and
    /// place in the managed entity limit. Subsequent stages and same-frame
    /// extraction exclude it. Systems in the current stage continue to see
    /// the pre-barrier state. Repeated calls remain disabled, and this command
    /// has the same validation, ordering, atomicity, and despawn-conflict rules
    /// as [`Self::insert`]. Every call occupies one bounded Command slot,
    /// including a repeated disable.
    ///
    /// Retain the entity's [`LogicEntity`] before disabling it: ordinary
    /// managed queries deliberately cannot rediscover disabled entities. Pass
    /// that saved handle to [`Self::enable`] later.
    pub fn disable(&mut self, entity: LogicEntity) -> Result<(), CommandEnqueueError> {
        self.insert(entity, Disabled)
    }

    /// Queues a disabled entity to become active after the stage barrier.
    ///
    /// Subsequent stages and same-frame extraction can observe the entity
    /// again. Enabling an already enabled entity is a successful no-op. This
    /// command has the same validation, ordering, atomicity, and
    /// despawn-conflict rules as [`Self::remove`]. It requires a handle saved
    /// before the entity became unavailable to ordinary managed queries. Every
    /// call occupies one bounded Command slot, including a no-op enable.
    pub fn enable(&mut self, entity: LogicEntity) -> Result<(), CommandEnqueueError> {
        self.remove::<Disabled>(entity)
    }

    /// Queues a managed entity for deferred despawn.
    pub fn despawn(&mut self, entity: LogicEntity) -> Result<(), CommandEnqueueError> {
        self.queue.push(QueuedCommand::Despawn(entity))
    }

    /// Requests successful application exit after the current stage barrier.
    ///
    /// All Systems in the current stage still run. The request takes effect
    /// only if the complete command batch passes validation; repeated requests
    /// coalesce in the result, but each call still consumes one command slot. A
    /// fixed-stage request stops remaining catch-up ticks, and no new render
    /// extraction is produced. Pending input edges are discarded and
    /// interpolation endpoints are snapped, so a headless host that deliberately
    /// continues does not replay pre-exit input or display pre-exit state. If
    /// World replacement is requested in the same batch, exit takes precedence.
    ///
    /// This method is unavailable during isolated World Startup. It does not
    /// terminate the process itself: the desktop host exits its event loop,
    /// while headless hosts inspect the resulting frame report. It produces no
    /// `WorldExit` lifecycle record and promises neither a cleanup hook nor one
    /// final presentation.
    pub fn request_exit(&mut self) -> Result<(), CommandEnqueueError> {
        if !self.intents.application_control_is_available() {
            return Err(CommandEnqueueError::ExitUnavailable);
        }
        self.queue.push(QueuedCommand::Exit)
    }

    /// Queues a change to fixed-simulation pause at the stage barrier.
    ///
    /// FrameUpdate, input collection, extraction, and presentation continue
    /// while fixed simulation is paused. Every successfully enqueued call
    /// consumes one bounded command slot, including a request for the current
    /// value. Multiple calls in one successful stage are last-write-wins after
    /// every System in that stage has observed its entry-time clock state.
    /// A failed System or rejected command batch discards the pause request.
    ///
    /// Entering pause stops remaining fixed catch-up work without discarding
    /// its accumulated simulation time, clears undelivered fixed input edges,
    /// and snaps managed interpolation. Resume makes retained work eligible on
    /// the next application frame; paused wall time and input edges are not
    /// replayed. Pause is Application-owned and survives World replacement.
    /// Replacement keeps its own stronger time cleanup and stage-skipping
    /// rules; a rejected transition does not undo an otherwise committed pause.
    /// Application exit suppresses a pause change from the same batch.
    ///
    /// This method is unavailable during isolated World Startup. It does not
    /// bind an input action or implement toggle policy.
    pub fn set_paused(&mut self, paused: bool) -> Result<(), CommandEnqueueError> {
        if !self.intents.application_control_is_available() {
            return Err(CommandEnqueueError::PauseUnavailable);
        }
        self.queue.push(QueuedCommand::SetPaused(paused))
    }

    /// Issues a direct causal token valid during the current application frame.
    pub fn new_transition_intent(&mut self) -> Result<TransitionIntentToken, CommandEnqueueError> {
        self.intents.issue()
    }

    /// Queues a no-payload replacement request for stage-end arbitration.
    pub fn replace_world(
        &mut self,
        intent: TransitionIntentToken,
        target: WorldFactoryId,
    ) -> Result<(), CommandEnqueueError> {
        self.queue
            .push(QueuedCommand::ReplaceWorld(TransitionRequest::new(
                intent, target,
            )))
    }
}

pub(crate) struct CommittedCommands {
    pub(crate) spawned: usize,
    pub(crate) despawned: usize,
    pub(crate) live_entities: usize,
    pub(crate) transitions: Vec<TransitionRequest>,
    pub(crate) pause: Option<bool>,
    pub(crate) exit_requested: bool,
}

#[derive(Debug)]
pub(crate) enum CommandCommitError {
    Batch(CommandBatchError),
    RuntimeInvariant,
}

impl From<CommandBatchError> for CommandCommitError {
    fn from(error: CommandBatchError) -> Self {
        Self::Batch(error)
    }
}

#[cfg(test)]
fn managed_entity_count(world: &World) -> usize {
    world
        .iter_entities()
        .filter(|entity| entity.contains::<ManagedEntity>())
        .count()
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use bevy_ecs::{entity_disabling::Disabled, prelude::Component};

    use crate::component::ComponentRegistry;

    use super::*;

    #[derive(Component, Clone, Copy)]
    struct Marker(u8);

    #[derive(Component, Debug, PartialEq, Eq)]
    struct Added(u8);

    #[derive(Bundle)]
    struct RemovableBundle {
        marker: Marker,
        added: Added,
    }

    #[derive(Component, Default, Clone, Copy)]
    #[require(Transform2d)]
    struct NeedsTransform;

    #[derive(Component, Clone, Copy)]
    struct Unapproved;

    static NEXT_FRESH_REQUIRED: AtomicUsize = AtomicUsize::new(1);

    #[derive(Component, Debug, PartialEq, Eq)]
    struct FreshRequired(usize);

    impl Default for FreshRequired {
        fn default() -> Self {
            Self(NEXT_FRESH_REQUIRED.fetch_add(1, Ordering::SeqCst))
        }
    }

    #[derive(Component, Clone, Copy)]
    #[require(FreshRequired)]
    struct NeedsFreshRequired;

    #[derive(Component, Clone, Copy)]
    #[require(Disabled)]
    struct NeedsDisabled;

    #[derive(Component)]
    struct DropMarker(Arc<AtomicUsize>);

    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn setup() -> (World, ApprovedComponents, ApplicationId, WorldGeneration) {
        let application = ApplicationId::from_raw(1);
        let generation = WorldGeneration::new(application, 1);
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Marker>().is_ok());
        assert!(registry.approve::<Added>().is_ok());
        assert!(registry.approve::<DropMarker>().is_ok());
        assert!(registry.approve::<Disabled>().is_ok());
        assert!(registry.approve::<Transform2d>().is_ok());
        assert!(registry.approve::<ActiveCamera2d>().is_ok());
        assert!(registry.approve::<NeedsTransform>().is_ok());
        assert!(registry.approve::<FreshRequired>().is_ok());
        assert!(registry.approve::<NeedsFreshRequired>().is_ok());
        assert!(registry.approve::<NeedsDisabled>().is_ok());
        let mut world = World::new();
        let approved = match registry.install(&mut world) {
            Ok(approved) => approved,
            Err(error) => panic!("unexpected approval error: {error}"),
        };
        (world, approved, application, generation)
    }

    #[test]
    fn spawn_is_invisible_until_whole_batch_commit() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(4);
        assert!(queue.push(QueuedCommand::spawn(Marker(7))).is_ok());
        let command_capacity = queue.command_capacity();
        assert_eq!(managed_entity_count(&world), 0);

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 8);
        let committed = result.expect("valid spawn batch should commit");
        assert_eq!(committed.live_entities, 1);
        let values: Vec<_> = world.query::<&Marker>().iter(&world).map(|m| m.0).collect();
        assert_eq!(values, [7]);
        let marker = world
            .component_id::<Marker>()
            .expect("Marker should be registered");
        let managed = world
            .component_id::<ManagedEntity>()
            .expect("ManagedEntity should be registered");
        assert!(
            !world
                .archetypes()
                .iter()
                .any(|archetype| archetype.contains(marker) && !archetype.contains(managed))
        );
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
    }

    #[test]
    fn copy_spawns_use_logical_slots_and_preserve_consecutive_order() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(6);
        assert!(queue.push(QueuedCommand::spawn(Marker(1))).is_ok());
        assert!(queue.push_spawn_copies(Marker(2), 3).is_ok());
        assert!(queue.push(QueuedCommand::spawn(Marker(3))).is_ok());
        assert!(queue.push(QueuedCommand::Exit).is_ok());
        assert_eq!(queue.commands.len(), 4);
        assert_eq!(queue.used_slots, 6);

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 0, 5)
            .expect("exactly fitting repeated spawns should commit");

        assert_eq!(committed.spawned, 5);
        assert_eq!(committed.live_entities, 5);
        assert!(committed.exit_requested);
        let values: Vec<_> = world
            .iter_entities()
            .filter_map(|entity| {
                let marker = entity.get::<Marker>()?;
                let managed = entity
                    .get::<ManagedEntity>()
                    .expect("every copy should be managed");
                assert_eq!(managed.handle(entity.id()).world_generation(), generation);
                Some(marker.0)
            })
            .collect();
        assert_eq!(values, [1, 2, 2, 2, 3]);
        assert!(queue.is_clean());
    }

    #[test]
    fn despawn_and_copy_spawn_use_the_net_entity_count_atomically() {
        let (mut world, mut approved, application, generation) = setup();
        let removed_raw = world.spawn(Marker(1)).id();
        let retained_raw = world.spawn(Marker(2)).id();
        for raw in [removed_raw, retained_raw] {
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
        }
        let removed = LogicEntity::new(application, generation, removed_raw);

        let mut rejected = CommandQueue::new(4);
        assert!(rejected.push(QueuedCommand::Despawn(removed)).is_ok());
        assert!(rejected.push_spawn_copies(Marker(3), 3).is_ok());
        assert!(matches!(
            rejected.validate_and_apply(&mut world, &mut approved, application, generation, 2, 3,),
            Err(CommandCommitError::Batch(
                CommandBatchError::EntityLimitExceeded {
                    limit: 3,
                    requested: 4,
                }
            ))
        ));
        assert!(world.entities().contains(removed_raw));
        assert!(world.entities().contains(retained_raw));
        assert_eq!(managed_entity_count(&world), 2);

        let mut exact = CommandQueue::new(3);
        assert!(exact.push(QueuedCommand::Despawn(removed)).is_ok());
        assert!(exact.push_spawn_copies(Marker(4), 2).is_ok());
        let committed = exact
            .validate_and_apply(&mut world, &mut approved, application, generation, 2, 3)
            .expect("net entity count at the limit should commit");
        assert_eq!(committed.despawned, 1);
        assert_eq!(committed.spawned, 2);
        assert_eq!(committed.live_entities, 3);
        assert!(!world.entities().contains(removed_raw));
        assert!(world.entities().contains(retained_raw));
        assert_eq!(managed_entity_count(&world), 3);
    }

    #[test]
    fn copy_spawn_limit_failures_poison_without_a_retained_prefix_and_then_reuse() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(3);
        assert!(queue.push(QueuedCommand::spawn(Marker(1))).is_ok());
        assert!(matches!(
            queue.push_spawn_copies(Marker(2), 3),
            Err(CommandEnqueueError::LimitExceeded { limit: 3 })
        ));
        assert_eq!(queue.commands.len(), 1);
        assert_eq!(queue.used_slots, 1);
        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 8),
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit: 3 }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 0);
        assert!(queue.is_clean());

        assert!(queue.push_spawn_copies(Marker(4), 3).is_ok());
        assert!(matches!(
            queue.push(QueuedCommand::spawn(Marker(5))),
            Err(CommandEnqueueError::LimitExceeded { limit: 3 })
        ));
        queue.discard();
        assert!(queue.is_clean());

        assert!(queue.push_spawn_copies(Marker(6), 3).is_ok());
        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 0, 8)
            .expect("discarded poison should not affect a later batch");
        assert_eq!(committed.spawned, 3);
    }

    #[test]
    fn copy_spawn_logical_slot_overflow_uses_the_bounded_limit_error() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(usize::MAX);
        assert!(queue.push(QueuedCommand::spawn(Marker(1))).is_ok());
        assert!(matches!(
            queue.push_spawn_copies(Marker(2), usize::MAX),
            Err(CommandEnqueueError::LimitExceeded { limit }) if limit == usize::MAX
        ));
        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 1),
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit }
            )) if limit == usize::MAX
        ));
        assert_eq!(managed_entity_count(&world), 0);
        assert!(queue.is_clean());
    }

    #[test]
    fn copy_spawn_validates_once_and_preflights_the_complete_entity_count() {
        let (mut world, mut approved, application, generation) = setup();
        let mut over_capacity = CommandQueue::new(3);
        assert!(over_capacity.push_spawn_copies(Marker(1), 3).is_ok());
        assert!(matches!(
            over_capacity.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                0,
                2,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::EntityLimitExceeded {
                    limit: 2,
                    requested: 3,
                }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 0);

        let mut unapproved = CommandQueue::new(3);
        assert!(unapproved.push_spawn_copies(Unapproved, 3).is_ok());
        assert!(matches!(
            unapproved
                .validate_and_apply(&mut world, &mut approved, application, generation, 0, 0,),
            Err(CommandCommitError::Batch(CommandBatchError::Component(
                ComponentApprovalError::UnapprovedBundleComponent { .. }
            )))
        ));
        assert_eq!(managed_entity_count(&world), 0);
    }

    #[test]
    fn copy_spawn_entity_count_overflow_rejects_before_mutation() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(2);
        assert!(queue.push_spawn_copies(Marker(1), 2).is_ok());

        assert!(matches!(
            queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                usize::MAX - 1,
                usize::MAX,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::EntityLimitExceeded {
                    limit,
                    requested: usize::MAX,
                }
            )) if limit == usize::MAX
        ));
        assert_eq!(managed_entity_count(&world), 0);
        assert!(queue.is_clean());
    }

    #[test]
    fn every_copy_gets_fresh_requirements_and_snapped_spatial_state() {
        let (mut world, mut approved, application, generation) = setup();
        let mut transform = Transform2d::default();
        transform.begin_fixed_tick();
        assert!(
            transform
                .set_translation(sim_engine::Vec2::new(4.0, -2.0))
                .is_ok()
        );
        let mut camera = ActiveCamera2d::new(
            sim_engine::Camera2d::new(sim_engine::Vec2::ZERO, 12.0)
                .expect("finite camera should construct"),
        );
        camera.begin_fixed_tick();
        assert!(camera.set_center(sim_engine::Vec2::new(-3.0, 5.0)).is_ok());

        let mut queue = CommandQueue::new(2);
        assert!(
            queue
                .push_spawn_copies((NeedsFreshRequired, NeedsDisabled, transform, camera), 2)
                .is_ok()
        );
        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 0, 2)
            .expect("approved repeated spatial bundle should commit");
        assert_eq!(committed.spawned, 2);
        assert_eq!(committed.live_entities, 2);

        let mut fresh_values = Vec::new();
        for entity in world
            .iter_entities()
            .filter(|entity| entity.contains::<NeedsFreshRequired>())
        {
            assert!(entity.contains::<Disabled>());
            let transform = entity
                .get::<Transform2d>()
                .expect("explicit Transform should remain");
            assert_eq!(transform.previous_translation(), transform.translation());
            let camera = entity
                .get::<ActiveCamera2d>()
                .expect("explicit camera should remain");
            assert_eq!(camera.previous_center(), camera.center());
            fresh_values.push(
                entity
                    .get::<FreshRequired>()
                    .expect("required value should materialize per entity")
                    .0,
            );
        }
        assert_eq!(fresh_values.len(), 2);
        assert_ne!(fresh_values[0], fresh_values[1]);
    }

    #[test]
    fn committed_live_count_tracks_despawns_and_spawns_in_one_batch() {
        let (mut world, mut approved, application, generation) = setup();
        let removed_raw = world.spawn(Marker(1)).id();
        let retained_raw = world.spawn((Marker(2), Disabled)).id();
        for raw in [removed_raw, retained_raw] {
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
        }

        let removed = LogicEntity::new(application, generation, removed_raw);
        let mut queue = CommandQueue::new(4);
        assert!(queue.push(QueuedCommand::Despawn(removed)).is_ok());
        assert!(queue.push(QueuedCommand::spawn(Marker(3))).is_ok());

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 2, 2)
            .expect("balanced batch should preserve the live count");

        assert_eq!(committed.live_entities, 2);
        assert_eq!(managed_entity_count(&world), 2);
        assert!(!world.entities().contains(removed_raw));
        assert!(world.entities().contains(retained_raw));
        assert!(queue.is_clean());
        assert!(queue.despawn_capacity() >= 1);
    }

    #[test]
    fn insert_adds_and_replaces_in_command_order_without_changing_live_count() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(3);
        for command in [
            QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(Added(3))),
            },
            QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(Marker(7))),
            },
            QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(Marker(9))),
            },
        ] {
            assert!(queue.push(command).is_ok());
        }

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("approved inserts should commit");

        assert_eq!(committed.spawned, 0);
        assert_eq!(committed.despawned, 0);
        assert_eq!(committed.live_entities, 1);
        assert_eq!(world.get::<Added>(raw), Some(&Added(3)));
        assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(9));
        assert!(queue.is_clean());
    }

    #[test]
    fn remove_is_explicit_idempotent_and_ordered_with_insert() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world
            .spawn(RemovableBundle {
                marker: Marker(1),
                added: Added(2),
            })
            .id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(5);
        for command in [
            QueuedCommand::Remove {
                entity,
                descriptor: RemoveDescriptor::new::<Added>(),
            },
            QueuedCommand::Remove {
                entity,
                descriptor: RemoveDescriptor::new::<Added>(),
            },
            QueuedCommand::Remove {
                entity,
                descriptor: RemoveDescriptor::new::<Marker>(),
            },
            QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(Marker(9))),
            },
            QueuedCommand::Remove {
                entity,
                descriptor: RemoveDescriptor::new::<Added>(),
            },
        ] {
            assert!(queue.push(command).is_ok());
        }

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("approved removals and later insert should commit in order");

        assert_eq!(committed.spawned, 0);
        assert_eq!(committed.despawned, 0);
        assert_eq!(committed.live_entities, 1);
        assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(9));
        assert!(world.get::<Added>(raw).is_none());
        assert!(world.get::<ManagedEntity>(raw).is_some());
        assert!(queue.is_clean());
        assert!(queue.component_target_capacity() >= 1);
        assert!(queue.presence_override_capacity() >= 2);

        let mut tuple_queue = CommandQueue::new(1);
        assert!(
            tuple_queue
                .push(QueuedCommand::Remove {
                    entity,
                    descriptor: RemoveDescriptor::new::<(Marker, Added)>(),
                })
                .is_ok()
        );
        tuple_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("tuple removal should remove only explicit members");
        assert!(world.get::<Marker>(raw).is_none());
        assert!(world.get::<ManagedEntity>(raw).is_some());

        world.entity_mut(raw).insert(RemovableBundle {
            marker: Marker(4),
            added: Added(5),
        });
        let mut derived_queue = CommandQueue::new(1);
        assert!(
            derived_queue
                .push(QueuedCommand::Remove {
                    entity,
                    descriptor: RemoveDescriptor::new::<RemovableBundle>(),
                })
                .is_ok()
        );
        derived_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("derived bundle removal should remove its explicit members");
        assert!(world.get::<Marker>(raw).is_none());
        assert!(world.get::<Added>(raw).is_none());
        assert!(world.get::<ManagedEntity>(raw).is_some());
    }

    #[test]
    fn removal_planner_ignores_inserts_on_unaffected_entities() {
        let (mut world, _approved, application, generation) = setup();
        let affected_raw = world.spawn((Marker(1), Added(1))).id();
        let affected = LogicEntity::new(application, generation, affected_raw);
        world
            .entity_mut(affected_raw)
            .insert(ManagedEntity::new(affected));
        let unrelated_raw = world.spawn(Marker(2)).id();
        let unrelated = LogicEntity::new(application, generation, unrelated_raw);
        world
            .entity_mut(unrelated_raw)
            .insert(ManagedEntity::new(unrelated));
        let mut queue = CommandQueue::new(3);
        assert!(
            queue
                .push(QueuedCommand::Insert {
                    entity: unrelated,
                    bundle: Box::new(TypedInsert(Added(8))),
                })
                .is_ok()
        );
        assert!(
            queue
                .push(QueuedCommand::Remove {
                    entity: affected,
                    descriptor: RemoveDescriptor::new::<Added>(),
                })
                .is_ok()
        );
        assert!(
            queue
                .push(QueuedCommand::Insert {
                    entity: affected,
                    bundle: Box::new(TypedInsert(Marker(9))),
                })
                .is_ok()
        );

        queue
            .plan_component_presence(&mut world, 1)
            .expect("presence planning should fit its bounded storage");

        assert_eq!(queue.component_target_order, [affected_raw]);
        assert!(
            queue
                .presence_overrides
                .keys()
                .all(|(entity, _)| *entity == affected_raw)
        );
        assert!(
            queue
                .inserted_components
                .iter()
                .all(|(entity, _)| *entity == affected_raw)
        );
        assert_eq!(queue.inserted_components.len(), 1);
    }

    #[test]
    fn removal_validates_the_final_required_component_set() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(NeedsTransform).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));

        let mut invalid = CommandQueue::new(1);
        assert!(
            invalid
                .push(QueuedCommand::Remove {
                    entity,
                    descriptor: RemoveDescriptor::new::<Transform2d>(),
                })
                .is_ok()
        );
        let invalid_result =
            invalid.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1);
        let Err(CommandCommitError::Batch(CommandBatchError::RequiredComponentWouldBeMissing {
            entity: rejected,
            required,
            required_by,
        })) = invalid_result
        else {
            panic!("removing a retained requirement should reject the batch");
        };
        assert_eq!(rejected, entity);
        assert!(required.ends_with("Transform2d"), "{required}");
        assert!(required_by.ends_with("NeedsTransform"), "{required_by}");
        assert!(invalid.is_clean());
        assert!(invalid.component_target_capacity() >= 1);
        assert!(invalid.presence_override_capacity() >= 1);
        assert!(world.get::<NeedsTransform>(raw).is_some());
        assert!(world.get::<Transform2d>(raw).is_some());

        let mut remove_both = CommandQueue::new(2);
        assert!(
            remove_both
                .push(QueuedCommand::Remove {
                    entity,
                    descriptor: RemoveDescriptor::new::<Transform2d>(),
                })
                .is_ok()
        );
        assert!(
            remove_both
                .push(QueuedCommand::Remove {
                    entity,
                    descriptor: RemoveDescriptor::new::<NeedsTransform>(),
                })
                .is_ok()
        );
        remove_both
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("removing the requiree and requirement should be valid");
        assert!(world.get::<NeedsTransform>(raw).is_none());
        assert!(world.get::<Transform2d>(raw).is_none());

        let materialize_raw = world.spawn(Marker(2)).id();
        let materialize = LogicEntity::new(application, generation, materialize_raw);
        world
            .entity_mut(materialize_raw)
            .insert(ManagedEntity::new(materialize));
        let mut materialize_required = CommandQueue::new(2);
        assert!(
            materialize_required
                .push(QueuedCommand::Remove {
                    entity: materialize,
                    descriptor: RemoveDescriptor::new::<Transform2d>(),
                })
                .is_ok()
        );
        assert!(
            materialize_required
                .push(QueuedCommand::Insert {
                    entity: materialize,
                    bundle: Box::new(TypedInsert(NeedsTransform)),
                })
                .is_ok()
        );
        materialize_required
            .validate_and_apply(&mut world, &mut approved, application, generation, 2, 2)
            .expect("later requiree insert should rematerialize its Transform");
        let transform = world
            .get::<Transform2d>(materialize_raw)
            .expect("required Transform should exist");
        assert_eq!(transform.previous_translation(), transform.translation());

        let existing_raw = world.spawn(NeedsTransform).id();
        let existing = LogicEntity::new(application, generation, existing_raw);
        world
            .entity_mut(existing_raw)
            .insert(ManagedEntity::new(existing));
        let mut rematerialize_existing = CommandQueue::new(2);
        assert!(
            rematerialize_existing
                .push(QueuedCommand::Remove {
                    entity: existing,
                    descriptor: RemoveDescriptor::new::<Transform2d>(),
                })
                .is_ok()
        );
        assert!(
            rematerialize_existing
                .push(QueuedCommand::Insert {
                    entity: existing,
                    bundle: Box::new(TypedInsert(NeedsTransform)),
                })
                .is_ok()
        );
        rematerialize_existing
            .validate_and_apply(&mut world, &mut approved, application, generation, 3, 3)
            .expect("reinserting a retained requiree should rematerialize its requirement");
        assert!(world.get::<NeedsTransform>(existing_raw).is_some());
        assert!(world.get::<Transform2d>(existing_raw).is_some());

        let reject_raw = world.spawn(Marker(3)).id();
        let reject = LogicEntity::new(application, generation, reject_raw);
        world
            .entity_mut(reject_raw)
            .insert(ManagedEntity::new(reject));
        let mut invalid_order = CommandQueue::new(2);
        assert!(
            invalid_order
                .push(QueuedCommand::Insert {
                    entity: reject,
                    bundle: Box::new(TypedInsert(NeedsTransform)),
                })
                .is_ok()
        );
        assert!(
            invalid_order
                .push(QueuedCommand::Remove {
                    entity: reject,
                    descriptor: RemoveDescriptor::new::<Transform2d>(),
                })
                .is_ok()
        );
        assert!(matches!(
            invalid_order.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                4,
                4,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::RequiredComponentWouldBeMissing { .. }
            ))
        ));
        assert!(world.get::<NeedsTransform>(reject_raw).is_none());
        assert!(world.get::<Transform2d>(reject_raw).is_none());
    }

    #[test]
    fn remove_and_despawn_conflict_is_symmetric_and_atomic() {
        for remove_first in [true, false] {
            let (mut world, mut approved, application, generation) = setup();
            let drops = Arc::new(AtomicUsize::new(0));
            let raw = world
                .spawn((Marker(1), DropMarker(Arc::clone(&drops))))
                .id();
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
            let mut queue = CommandQueue::new(2);
            let remove = || QueuedCommand::Remove {
                entity,
                descriptor: RemoveDescriptor::new::<DropMarker>(),
            };
            if remove_first {
                assert!(queue.push(remove()).is_ok());
                assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
            } else {
                assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
                assert!(queue.push(remove()).is_ok());
            }

            assert!(matches!(
                queue.validate_and_apply(
                    &mut world,
                    &mut approved,
                    application,
                    generation,
                    1,
                    1,
                ),
                Err(CommandCommitError::Batch(
                    CommandBatchError::ConflictingEntityCommands {
                        entity: conflicted
                    }
                )) if conflicted == entity
            ));
            assert_eq!(drops.load(Ordering::SeqCst), 0);
            assert!(world.get::<DropMarker>(raw).is_some());
            assert!(world.entities().contains(raw));
        }
    }

    #[test]
    fn successful_and_repeated_remove_drop_the_stored_value_exactly_once() {
        let (mut world, mut approved, application, generation) = setup();
        let drops = Arc::new(AtomicUsize::new(0));
        let raw = world
            .spawn((Marker(1), DropMarker(Arc::clone(&drops))))
            .id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(2);
        for _ in 0..2 {
            assert!(
                queue
                    .push(QueuedCommand::Remove {
                        entity,
                        descriptor: RemoveDescriptor::new::<DropMarker>(),
                    })
                    .is_ok()
            );
        }

        queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("repeated removal should be an idempotent successful batch");
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(world.get::<DropMarker>(raw).is_none());
        assert!(world.get::<Marker>(raw).is_some());
        assert!(world.get::<ManagedEntity>(raw).is_some());
    }

    #[test]
    fn invalid_remove_target_or_type_rejects_without_mutation() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn((Marker(1), Added(2))).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let foreign = LogicEntity::new(application, WorldGeneration::new(application, 9), raw);

        for (target, descriptor) in [
            (foreign, RemoveDescriptor::new::<Added>()),
            (entity, RemoveDescriptor::new::<Unapproved>()),
            (entity, RemoveDescriptor::new::<ManagedEntity>()),
        ] {
            let mut queue = CommandQueue::new(2);
            assert!(queue.push(QueuedCommand::spawn(Marker(8))).is_ok());
            assert!(
                queue
                    .push(QueuedCommand::Remove {
                        entity: target,
                        descriptor,
                    })
                    .is_ok()
            );
            assert!(
                queue
                    .validate_and_apply(&mut world, &mut approved, application, generation, 1, 4,)
                    .is_err()
            );
            assert_eq!(managed_entity_count(&world), 1);
            assert_eq!(world.get::<Added>(raw), Some(&Added(2)));
        }

        let missing_raw = world.spawn(Marker(0)).id();
        let missing = LogicEntity::new(application, generation, missing_raw);
        world
            .entity_mut(missing_raw)
            .insert(ManagedEntity::new(missing));
        assert!(world.despawn(missing_raw));
        let mut missing_queue = CommandQueue::new(1);
        assert!(
            missing_queue
                .push(QueuedCommand::Remove {
                    entity: missing,
                    descriptor: RemoveDescriptor::new::<Added>(),
                })
                .is_ok()
        );
        assert!(matches!(
            missing_queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                4,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::MissingEntity { entity: rejected }
            )) if rejected == missing
        ));
        assert_eq!(world.get::<Added>(raw), Some(&Added(2)));
    }

    #[test]
    fn insert_and_despawn_conflict_is_symmetric_and_atomic() {
        for insert_first in [true, false] {
            let (mut world, mut approved, application, generation) = setup();
            let raw = world.spawn(Marker(1)).id();
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
            let mut queue = CommandQueue::new(2);
            let insert = || QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(Marker(2))),
            };
            if insert_first {
                assert!(queue.push(insert()).is_ok());
                assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
            } else {
                assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
                assert!(queue.push(insert()).is_ok());
            }

            let result =
                queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1);

            assert!(matches!(
                result,
                Err(CommandCommitError::Batch(
                    CommandBatchError::ConflictingEntityCommands { entity: conflicted }
                )) if conflicted == entity
            ));
            assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(1));
            assert!(world.entities().contains(raw));
            assert!(queue.is_clean());
        }
    }

    #[test]
    fn invalid_insert_target_and_bundle_reject_before_any_structural_mutation() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let foreign = LogicEntity::new(application, WorldGeneration::new(application, 7), raw);
        let mut foreign_queue = CommandQueue::new(1);
        assert!(
            foreign_queue
                .push(QueuedCommand::Insert {
                    entity: foreign,
                    bundle: Box::new(TypedInsert(Added(2))),
                })
                .is_ok()
        );
        assert!(matches!(
            foreign_queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                2,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::ForeignEntity { entity }
            )) if entity == foreign
        ));

        let missing_raw = world.spawn(Marker(8)).id();
        let missing = LogicEntity::new(application, generation, missing_raw);
        world
            .entity_mut(missing_raw)
            .insert(ManagedEntity::new(missing));
        assert!(world.despawn(missing_raw));
        let mut missing_queue = CommandQueue::new(1);
        assert!(
            missing_queue
                .push(QueuedCommand::Insert {
                    entity: missing,
                    bundle: Box::new(TypedInsert(Added(3))),
                })
                .is_ok()
        );
        assert!(matches!(
            missing_queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                2,
            ),
            Err(CommandCommitError::Batch(
                CommandBatchError::MissingEntity { entity }
            )) if entity == missing
        ));

        let mut identity_queue = CommandQueue::new(1);
        assert!(
            identity_queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(ManagedEntity::new(entity))),
                })
                .is_ok()
        );
        assert!(matches!(
            identity_queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                2,
            ),
            Err(CommandCommitError::Batch(CommandBatchError::Component(
                ComponentApprovalError::RuntimeIdentityInBundle
            )))
        ));

        let mut bundle_queue = CommandQueue::new(2);
        assert!(bundle_queue.push(QueuedCommand::spawn(Added(4))).is_ok());
        assert!(
            bundle_queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(Unapproved)),
                })
                .is_ok()
        );
        assert!(matches!(
            bundle_queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                2,
            ),
            Err(CommandCommitError::Batch(CommandBatchError::Component(_)))
        ));
        assert_eq!(managed_entity_count(&world), 1);
        assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(1));
        assert!(world.get::<Added>(raw).is_none());
    }

    #[test]
    fn insert_snaps_only_explicit_or_newly_required_interpolation_values() {
        let (mut world, mut approved, application, generation) = setup();
        let mut moving = Transform2d::from_xy(1.0, 0.0).expect("finite transform");
        moving.begin_fixed_tick();
        moving
            .set_translation(sim_engine::Vec2::new(2.0, 0.0))
            .expect("finite movement");
        let raw = world.spawn((Marker(1), moving)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut preserve_queue = CommandQueue::new(1);
        assert!(
            preserve_queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(NeedsTransform)),
                })
                .is_ok()
        );
        preserve_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("required transform insertion should commit");
        let preserved = world.get::<Transform2d>(raw).expect("transform remains");
        assert_eq!(
            preserved.previous_translation(),
            sim_engine::Vec2::new(1.0, 0.0)
        );
        assert_eq!(preserved.translation(), sim_engine::Vec2::new(2.0, 0.0));

        let mut replacement = Transform2d::from_xy(8.0, 0.0).expect("finite replacement transform");
        replacement.begin_fixed_tick();
        replacement
            .set_translation(sim_engine::Vec2::new(9.0, 0.0))
            .expect("finite replacement movement");
        let mut replace_queue = CommandQueue::new(1);
        assert!(
            replace_queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(replacement)),
                })
                .is_ok()
        );
        replace_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 1, 1)
            .expect("explicit transform replacement should commit");
        let replaced = world.get::<Transform2d>(raw).expect("transform remains");
        assert_eq!(
            replaced.previous_translation(),
            sim_engine::Vec2::new(9.0, 0.0)
        );
        assert_eq!(replaced.translation(), sim_engine::Vec2::new(9.0, 0.0));

        let plain_raw = world.spawn(Marker(2)).id();
        let plain = LogicEntity::new(application, generation, plain_raw);
        world
            .entity_mut(plain_raw)
            .insert(ManagedEntity::new(plain));
        let mut required_queue = CommandQueue::new(1);
        assert!(
            required_queue
                .push(QueuedCommand::Insert {
                    entity: plain,
                    bundle: Box::new(TypedInsert(NeedsTransform)),
                })
                .is_ok()
        );
        required_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 2, 2)
            .expect("missing required transform should materialize");
        let materialized = world
            .get::<Transform2d>(plain_raw)
            .expect("required transform exists");
        assert_eq!(
            materialized.previous_translation(),
            materialized.translation()
        );

        let mut replacement_camera = ActiveCamera2d::new(
            sim_engine::Camera2d::new(sim_engine::Vec2::ZERO, 16.0)
                .expect("valid replacement camera"),
        );
        replacement_camera
            .set_center(sim_engine::Vec2::new(7.0, -2.0))
            .expect("finite replacement center");
        let mut camera_queue = CommandQueue::new(1);
        assert!(
            camera_queue
                .push(QueuedCommand::Insert {
                    entity: plain,
                    bundle: Box::new(TypedInsert(replacement_camera)),
                })
                .is_ok()
        );
        camera_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 2, 2)
            .expect("explicit camera insertion should commit");
        let camera = world
            .get::<ActiveCamera2d>(plain_raw)
            .expect("camera should be inserted");
        assert_eq!(camera.previous_center(), sim_engine::Vec2::new(7.0, -2.0));
        assert_eq!(camera.center(), sim_engine::Vec2::new(7.0, -2.0));

        let precedence_raw = world.spawn(Marker(3)).id();
        let precedence = LogicEntity::new(application, generation, precedence_raw);
        world
            .entity_mut(precedence_raw)
            .insert(ManagedEntity::new(precedence));
        let mut explicit = Transform2d::from_xy(3.0, 0.0).expect("finite explicit transform");
        explicit.begin_fixed_tick();
        explicit
            .set_translation(sim_engine::Vec2::new(4.0, 0.0))
            .expect("finite explicit movement");
        let mut precedence_queue = CommandQueue::new(1);
        assert!(
            precedence_queue
                .push(QueuedCommand::Insert {
                    entity: precedence,
                    bundle: Box::new(TypedInsert((NeedsTransform, explicit))),
                })
                .is_ok()
        );
        precedence_queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 3, 3)
            .expect("explicit Transform should satisfy the required component");
        let explicit = world
            .get::<Transform2d>(precedence_raw)
            .expect("explicit Transform should be retained");
        assert_eq!(
            explicit.previous_translation(),
            sim_engine::Vec2::new(4.0, 0.0)
        );
        assert_eq!(explicit.translation(), sim_engine::Vec2::new(4.0, 0.0));
    }

    #[test]
    fn rejected_insert_drops_new_payload_once_without_replacing_old_value() {
        let (mut world, mut approved, application, generation) = setup();
        let old_drops = Arc::new(AtomicUsize::new(0));
        let new_drops = Arc::new(AtomicUsize::new(0));
        let raw = world
            .spawn((Marker(1), DropMarker(Arc::clone(&old_drops))))
            .id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(2);
        assert!(
            queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(DropMarker(Arc::clone(&new_drops)))),
                })
                .is_ok()
        );
        assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());

        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1),
            Err(CommandCommitError::Batch(
                CommandBatchError::ConflictingEntityCommands { .. }
            ))
        ));
        assert_eq!(new_drops.load(Ordering::SeqCst), 1);
        assert_eq!(old_drops.load(Ordering::SeqCst), 0);
        assert!(world.get::<DropMarker>(raw).is_some());

        let discarded_drops = Arc::new(AtomicUsize::new(0));
        let mut discarded = CommandQueue::new(1);
        assert!(
            discarded
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(DropMarker(Arc::clone(&discarded_drops)))),
                })
                .is_ok()
        );
        discarded.discard();
        assert_eq!(discarded_drops.load(Ordering::SeqCst), 1);
        assert_eq!(old_drops.load(Ordering::SeqCst), 0);
        assert!(world.get::<DropMarker>(raw).is_some());
    }

    #[test]
    fn insert_obeys_queue_poison_and_reuses_command_storage() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let retained_drops = Arc::new(AtomicUsize::new(0));
        let rejected_drops = Arc::new(AtomicUsize::new(0));
        let mut queue = CommandQueue::new(1);
        assert!(
            queue
                .push(QueuedCommand::Insert {
                    entity,
                    bundle: Box::new(TypedInsert(DropMarker(Arc::clone(&retained_drops)))),
                })
                .is_ok()
        );
        let capacity = queue.command_capacity();
        assert!(matches!(
            queue.push(QueuedCommand::Insert {
                entity,
                bundle: Box::new(TypedInsert(DropMarker(Arc::clone(&rejected_drops)))),
            }),
            Err(CommandEnqueueError::LimitExceeded { limit: 1 })
        ));
        assert_eq!(rejected_drops.load(Ordering::SeqCst), 1);
        assert_eq!(retained_drops.load(Ordering::SeqCst), 0);

        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1),
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit: 1 }
            ))
        ));
        assert_eq!(retained_drops.load(Ordering::SeqCst), 1);
        assert!(world.get::<DropMarker>(raw).is_none());
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), capacity);
    }

    #[test]
    fn repeated_exit_requests_coalesce_without_changing_entities() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(2);
        assert!(queue.push(QueuedCommand::Exit).is_ok());
        assert!(queue.push(QueuedCommand::Exit).is_ok());
        let capacity = queue.command_capacity();

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 0, 1)
            .expect("repeated exit requests should form one valid terminal request");

        assert!(committed.exit_requested);
        assert_eq!(committed.spawned, 0);
        assert_eq!(committed.despawned, 0);
        assert_eq!(committed.live_entities, 0);
        assert!(committed.transitions.is_empty());
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), capacity);

        assert!(queue.push(QueuedCommand::Exit).is_ok());
        assert!(queue.push(QueuedCommand::Exit).is_ok());
        assert!(matches!(
            queue.push(QueuedCommand::Exit),
            Err(CommandEnqueueError::LimitExceeded { limit: 2 })
        ));
        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 1),
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit: 2 }
            ))
        ));
        assert!(queue.is_clean());
    }

    #[test]
    fn pause_requests_are_inline_last_write_wins_and_use_exact_slots() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(3);
        assert!(queue.push(QueuedCommand::SetPaused(true)).is_ok());
        assert!(queue.push(QueuedCommand::spawn(Marker(4))).is_ok());
        assert!(queue.push(QueuedCommand::SetPaused(false)).is_ok());
        let capacity = queue.command_capacity();

        let committed = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 0, 1)
            .expect("an exactly fitting pause/spawn batch should commit");

        assert_eq!(committed.pause, Some(false));
        assert!(!committed.exit_requested);
        assert_eq!(committed.spawned, 1);
        assert!(matches!(
            world.query::<&Marker>().single(&world).map(|value| value.0),
            Ok(4)
        ));
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), capacity);

        assert!(queue.push(QueuedCommand::SetPaused(true)).is_ok());
        assert!(queue.push(QueuedCommand::SetPaused(true)).is_ok());
        assert!(queue.push(QueuedCommand::SetPaused(true)).is_ok());
        assert!(matches!(
            queue.push(QueuedCommand::SetPaused(false)),
            Err(CommandEnqueueError::LimitExceeded { limit: 3 })
        ));
        assert!(matches!(
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1),
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit: 3 }
            ))
        ));
        assert!(queue.is_clean());
    }

    #[test]
    fn tracked_count_underflow_rejects_before_mutation() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(1);
        assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
        let command_capacity = queue.command_capacity();

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 1);

        assert!(matches!(result, Err(CommandCommitError::RuntimeInvariant)));
        assert!(world.entities().contains(raw));
        assert_eq!(managed_entity_count(&world), 1);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
        assert!(queue.despawn_capacity() >= 1);
    }

    #[test]
    fn duplicate_despawn_rejects_spawn_too() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(4);
        assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
        assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
        assert!(queue.push(QueuedCommand::spawn(Marker(2))).is_ok());
        let command_capacity = queue.command_capacity();

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 8);
        assert!(matches!(
            result,
            Err(CommandCommitError::Batch(
                CommandBatchError::DuplicateDespawn { .. }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 1);
        assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(1));
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
        assert!(queue.despawn_capacity() >= 2);
    }

    #[test]
    fn foreign_generation_is_rejected() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn(Marker(1)).id();
        world
            .entity_mut(raw)
            .insert(ManagedEntity::new(LogicEntity::new(
                application,
                generation,
                raw,
            )));
        let foreign = LogicEntity::new(application, WorldGeneration::new(application, 2), raw);
        let mut queue = CommandQueue::new(2);
        assert!(queue.push(QueuedCommand::Despawn(foreign)).is_ok());
        let command_capacity = queue.command_capacity();

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 8);
        assert!(matches!(
            result,
            Err(CommandCommitError::Batch(
                CommandBatchError::ForeignEntity { .. }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 1);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
        assert!(queue.despawn_capacity() >= 1);
    }

    #[test]
    fn target_validation_rejects_mismatched_private_provenance() {
        let (mut world, mut approved, application, generation) = setup();
        let corrupted = WorldGeneration::new(application, 99);
        let raw = world
            .spawn((Marker(1), ManagedEntity::for_generation(corrupted)))
            .id();
        let entity = LogicEntity::new(application, generation, raw);
        let mut queue = CommandQueue::new(1);
        assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());

        assert!(matches!(
            queue.validate_and_apply(
                &mut world,
                &mut approved,
                application,
                generation,
                1,
                8,
            ),
            Err(CommandCommitError::Batch(CommandBatchError::MissingEntity {
                entity: rejected
            })) if rejected == entity
        ));
        assert!(world.entities().contains(raw));
        assert_eq!(managed_entity_count(&world), 1);
    }

    #[test]
    fn overflow_poison_rejects_retained_commands() {
        let (mut world, mut approved, application, generation) = setup();
        let mut queue = CommandQueue::new(1);
        let retained_drops = Arc::new(AtomicUsize::new(0));
        let rejected_drops = Arc::new(AtomicUsize::new(0));
        assert!(
            queue
                .push(QueuedCommand::spawn(DropMarker(Arc::clone(
                    &retained_drops,
                ))))
                .is_ok()
        );
        let command_capacity = queue.command_capacity();
        assert!(matches!(
            queue.push(QueuedCommand::spawn(DropMarker(Arc::clone(
                &rejected_drops,
            )))),
            Err(CommandEnqueueError::LimitExceeded { limit: 1 })
        ));
        assert_eq!(rejected_drops.load(Ordering::SeqCst), 1);
        assert_eq!(retained_drops.load(Ordering::SeqCst), 0);

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 0, 8);
        assert!(matches!(
            result,
            Err(CommandCommitError::Batch(
                CommandBatchError::LimitExceeded { limit: 1 }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 0);
        assert_eq!(retained_drops.load(Ordering::SeqCst), 1);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
    }

    #[test]
    fn disabled_entities_still_consume_the_hard_command_limit() {
        let (mut world, mut approved, application, generation) = setup();
        let raw = world.spawn((Marker(1), Disabled)).id();
        let entity = LogicEntity::new(application, generation, raw);
        world.entity_mut(raw).insert(ManagedEntity::new(entity));
        let mut queue = CommandQueue::new(1);
        assert!(queue.push(QueuedCommand::spawn(Marker(2))).is_ok());
        let command_capacity = queue.command_capacity();

        let result =
            queue.validate_and_apply(&mut world, &mut approved, application, generation, 1, 1);

        assert!(matches!(
            result,
            Err(CommandCommitError::Batch(
                CommandBatchError::EntityLimitExceeded {
                    limit: 1,
                    requested: 2,
                }
            ))
        ));
        assert_eq!(managed_entity_count(&world), 1);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
    }

    #[test]
    fn successful_batches_reuse_command_and_despawn_capacity() {
        let (mut world, mut approved, application, generation) = setup();
        let mut entities = Vec::new();
        for value in 0..4 {
            let raw = world.spawn(Marker(value)).id();
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
            entities.push(entity);
        }
        let mut queue = CommandQueue::new(4);

        for entity in &entities {
            assert!(queue.push(QueuedCommand::Despawn(*entity)).is_ok());
        }
        let command_capacity = queue.command_capacity();
        let first = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 4, 4)
            .expect("first warmed batch should commit");
        let despawn_capacity = queue.despawn_capacity();
        assert_eq!(first.despawned, 4);
        assert!(queue.is_clean());
        assert!(despawn_capacity >= 4);

        let mut second_entities = Vec::new();
        for value in 4..8 {
            let raw = world.spawn(Marker(value)).id();
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
            second_entities.push(entity);
        }
        for entity in second_entities {
            assert!(queue.push(QueuedCommand::Despawn(entity)).is_ok());
        }
        assert_eq!(queue.command_capacity(), command_capacity);
        let second = queue
            .validate_and_apply(&mut world, &mut approved, application, generation, 4, 4)
            .expect("second equal-sized batch should commit");
        assert_eq!(second.despawned, 4);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
        assert_eq!(queue.despawn_capacity(), despawn_capacity);
    }

    #[test]
    fn discard_drops_payloads_and_retains_command_capacity() {
        let drops = Arc::new(AtomicUsize::new(0));
        let mut queue = CommandQueue::new(2);
        assert!(
            queue
                .push(QueuedCommand::spawn(DropMarker(Arc::clone(&drops))))
                .is_ok()
        );
        let command_capacity = queue.command_capacity();

        queue.discard();

        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(queue.is_clean());
        assert_eq!(queue.command_capacity(), command_capacity);
    }
}

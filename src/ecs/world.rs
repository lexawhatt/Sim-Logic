//! Isolated managed World construction.

use std::{error::Error, fmt};

use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    prelude::Resource,
    world::World,
};

use crate::{
    component::{ApprovedComponents, ComponentApprovalError, validate_resource},
    identity::{ApplicationId, LogicEntity, ManagedEntity, WorldGeneration},
};

/// Failure while constructing an isolated World candidate.
#[derive(Debug)]
pub enum WorldBuildError {
    /// A bundle or Resource violates the managed component rules.
    Component(ComponentApprovalError),
    /// Candidate construction would exceed the configured entity limit.
    EntityLimitExceeded {
        /// Frozen maximum managed entity count.
        limit: usize,
    },
    /// A candidate attempted to despawn an entity outside itself.
    ForeignEntity {
        /// Rejected opaque entity handle.
        entity: LogicEntity,
    },
    /// A candidate attempted to despawn a handle that is no longer live.
    MissingEntity {
        /// Rejected opaque entity handle.
        entity: LogicEntity,
    },
    /// Application-specific candidate validation failed.
    User {
        /// Human-readable failure supplied by the factory.
        message: String,
    },
}

impl WorldBuildError {
    /// Creates an application-specific candidate construction failure.
    pub fn user(message: impl Into<String>) -> Self {
        Self::User {
            message: message.into(),
        }
    }
}

impl fmt::Display for WorldBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Component(error) => write!(formatter, "candidate value was rejected: {error}"),
            Self::EntityLimitExceeded { limit } => {
                write!(formatter, "candidate exceeds its entity limit of {limit}")
            }
            Self::ForeignEntity { entity } => {
                write!(formatter, "entity {entity:?} belongs to another candidate")
            }
            Self::MissingEntity { entity } => {
                write!(
                    formatter,
                    "entity {entity:?} is not alive in this candidate"
                )
            }
            Self::User { message } => formatter.write_str(message),
        }
    }
}

impl Error for WorldBuildError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Component(error) => Some(error),
            _ => None,
        }
    }
}

/// Restricted builder for one isolated World candidate.
///
/// It deliberately provides no raw mutable Bevy `World`, observer registration,
/// or direct structural escape hatch. Every entity receives a generation-aware
/// [`LogicEntity`] identity.
pub struct WorldBuilder {
    world: World,
    approved: ApprovedComponents,
    application: ApplicationId,
    generation: WorldGeneration,
    entity_limit: usize,
    managed_entities: usize,
}

impl WorldBuilder {
    pub(crate) fn new(
        world: World,
        approved: ApprovedComponents,
        application: ApplicationId,
        generation: WorldGeneration,
        entity_limit: usize,
    ) -> Self {
        Self {
            world,
            approved,
            application,
            generation,
            entity_limit,
            managed_entities: 0,
        }
    }

    /// Spawns one approved, side-effect-free bundle immediately in the isolated
    /// candidate.
    ///
    /// Approved required components are materialized during the spawn. An
    /// explicit value in `bundle` takes precedence over its required default.
    ///
    /// The returned identity already carries the candidate's reserved
    /// generation. It cannot address the active World before candidate commit.
    pub fn spawn<B>(&mut self, bundle: B) -> Result<LogicEntity, WorldBuildError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.validate_spawn_bundle::<B>()?;
        self.ensure_spawn_capacity(1)?;
        Ok(self.spawn_validated(bundle))
    }

    /// Spawns one fixed-size array of approved, side-effect-free bundles
    /// immediately in the isolated candidate.
    ///
    /// Bundle approval and capacity for the complete array are checked before
    /// the first entity is created. A typed validation or entity-limit error
    /// therefore leaves no spawned prefix from this call. A zero-length array
    /// still validates its bundle type, then succeeds without spawning an
    /// entity or changing the managed-entity count.
    ///
    /// Required components, explicit-value precedence, and candidate identity
    /// are identical to repeated successful [`Self::spawn`] calls. Returned
    /// handles follow input order. This fixed-array helper does not perform a
    /// dynamic Bevy batch insertion and makes no bulk-performance guarantee.
    pub fn spawn_array<B, const N: usize>(
        &mut self,
        bundles: [B; N],
    ) -> Result<[LogicEntity; N], WorldBuildError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.validate_spawn_bundle::<B>()?;
        self.ensure_spawn_capacity(N)?;
        Ok(bundles.map(|bundle| self.spawn_validated(bundle)))
    }

    fn validate_spawn_bundle<B>(&mut self) -> Result<(), WorldBuildError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        self.approved
            .validate_bundle::<B>(&mut self.world)
            .map_err(WorldBuildError::Component)
    }

    fn ensure_spawn_capacity(&self, additional: usize) -> Result<(), WorldBuildError> {
        let Some(requested) = self.managed_entities.checked_add(additional) else {
            return Err(WorldBuildError::EntityLimitExceeded {
                limit: self.entity_limit,
            });
        };
        if requested > self.entity_limit {
            return Err(WorldBuildError::EntityLimitExceeded {
                limit: self.entity_limit,
            });
        }
        Ok(())
    }

    fn spawn_validated<B>(&mut self, bundle: B) -> LogicEntity
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        let managed = ManagedEntity::for_generation(self.generation);
        let entity = self.world.spawn((bundle, managed)).id();
        let identity = managed.handle(entity);
        self.managed_entities += 1;
        identity
    }

    /// Despawns one entity from the isolated candidate.
    pub fn despawn(&mut self, entity: LogicEntity) -> Result<(), WorldBuildError> {
        if entity.application() != self.application || entity.world_generation() != self.generation
        {
            return Err(WorldBuildError::ForeignEntity { entity });
        }
        if !self
            .world
            .get::<ManagedEntity>(entity.entity())
            .is_some_and(|managed| managed.handle(entity.entity()) == entity)
        {
            return Err(WorldBuildError::MissingEntity { entity });
        }
        let Some(managed_entities) = self.managed_entities.checked_sub(1) else {
            debug_assert!(false, "candidate managed-entity count is inconsistent");
            return Err(WorldBuildError::MissingEntity { entity });
        };
        let removed = self.world.despawn(entity.entity());
        debug_assert!(removed, "validated candidate entity disappeared");
        self.managed_entities = managed_entities;
        Ok(())
    }

    /// Inserts or replaces one typed World Resource in the isolated candidate.
    ///
    /// Resources with user lifecycle hooks or user-declared required
    /// components are rejected because they can create structural work outside
    /// the managed candidate path.
    pub fn insert_resource<R: Resource>(
        &mut self,
        resource: R,
    ) -> Result<&mut Self, WorldBuildError> {
        validate_resource::<R>(&mut self.world).map_err(WorldBuildError::Component)?;
        self.world.insert_resource(resource);
        Ok(self)
    }

    /// Returns the candidate's reserved generation.
    pub const fn generation(&self) -> WorldGeneration {
        self.generation
    }

    pub(crate) fn finish(self) -> (World, ApprovedComponents, usize) {
        (self.world, self.approved, self.managed_entities)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use bevy_ecs::{
        entity_disabling::Disabled,
        lifecycle::HookContext,
        prelude::{Component, Resource},
        world::DeferredWorld,
    };

    use crate::component::{ComponentRegistry, LifecycleHook};

    use super::*;

    #[derive(Component)]
    struct Marker;

    #[derive(Component)]
    struct Unapproved;

    #[derive(Component)]
    struct Indexed(u8);

    #[derive(Component)]
    #[require(RequiredValue)]
    struct NeedsRequired;

    #[derive(Component, Debug, Default, PartialEq, Eq)]
    struct RequiredValue(u8);

    #[derive(Component)]
    struct DropProbe(Arc<AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[derive(Debug, Resource, PartialEq, Eq)]
    struct Setting(u8);

    fn on_resource_add(_world: DeferredWorld<'_>, _context: HookContext) {}

    #[derive(Resource)]
    #[component(on_add = on_resource_add)]
    struct HookedSetting;

    #[derive(Component, Default)]
    struct RequiredSettingPart;

    #[derive(Resource)]
    #[require(RequiredSettingPart)]
    struct SettingWithRequiredComponent;

    fn builder_from_registry(limit: usize, registry: ComponentRegistry) -> WorldBuilder {
        let application = ApplicationId::from_raw(1);
        let generation = WorldGeneration::new(application, 3);
        let mut world = World::new();
        let approved = match registry.install(&mut world) {
            Ok(approved) => approved,
            Err(error) => panic!("unexpected approval error: {error}"),
        };
        WorldBuilder::new(world, approved, application, generation, limit)
    }

    fn builder(limit: usize) -> WorldBuilder {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Marker>().is_ok());
        assert!(registry.approve::<Disabled>().is_ok());
        builder_from_registry(limit, registry)
    }

    #[test]
    fn managed_spawn_assigns_candidate_generation() {
        let mut builder = builder(1);
        let entity = builder.spawn(Marker);
        let entity = match entity {
            Ok(entity) => entity,
            Err(error) => panic!("unexpected spawn error: {error}"),
        };

        assert_eq!(entity.world_generation(), builder.generation());
        assert!(matches!(
            builder.spawn(Marker),
            Err(WorldBuildError::EntityLimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn managed_spawn_creates_only_the_final_managed_archetype() {
        let mut builder = builder(1);
        let entity = builder.spawn(Marker).expect("managed spawn should succeed");
        let (world, _, managed_entities) = builder.finish();
        let marker = world
            .component_id::<Marker>()
            .expect("Marker should be registered");
        let managed = world
            .component_id::<ManagedEntity>()
            .expect("ManagedEntity should be registered");

        assert_eq!(managed_entities, 1);
        assert_eq!(
            world
                .get::<ManagedEntity>(entity.entity())
                .map(|managed| managed.handle(entity.entity())),
            Some(entity)
        );
        assert!(
            !world
                .archetypes()
                .iter()
                .any(|archetype| archetype.contains(marker) && !archetype.contains(managed))
        );
    }

    #[test]
    fn array_spawn_returns_input_order_handles_with_candidate_generation() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Indexed>().is_ok());
        let mut builder = builder_from_registry(3, registry);

        let handles = builder
            .spawn_array([Indexed(7), Indexed(3), Indexed(9)])
            .expect("exactly fitting array should spawn");

        assert_eq!(
            handles.map(LogicEntity::world_generation),
            [builder.generation(); 3]
        );
        assert_ne!(handles[0], handles[1]);
        assert_ne!(handles[1], handles[2]);
        for (handle, expected) in handles.into_iter().zip([7, 3, 9]) {
            assert_eq!(
                builder
                    .world
                    .get::<Indexed>(handle.entity())
                    .map(|value| value.0),
                Some(expected)
            );
            assert_eq!(
                builder
                    .world
                    .get::<ManagedEntity>(handle.entity())
                    .map(|managed| managed.handle(handle.entity())),
                Some(handle)
            );
        }
        assert!(matches!(
            builder.spawn(Indexed(1)),
            Err(WorldBuildError::EntityLimitExceeded { limit: 3 })
        ));
    }

    #[test]
    fn zero_and_single_element_arrays_preserve_validation_and_limit_semantics() {
        let mut builder = builder(1);

        let empty = builder
            .spawn_array::<Marker, 0>([])
            .expect("approved empty array should be valid");
        assert_eq!(empty, []);
        let [only] = builder
            .spawn_array([Marker])
            .expect("single-element array should match singular spawn");
        assert_eq!(only.world_generation(), builder.generation());
        assert!(builder.spawn_array::<Marker, 0>([]).is_ok());

        assert!(matches!(
            builder.spawn_array::<Unapproved, 0>([]),
            Err(WorldBuildError::Component(
                ComponentApprovalError::UnapprovedBundleComponent { .. }
            ))
        ));
    }

    #[test]
    fn array_limit_failure_creates_no_prefix_and_leaves_capacity_available() {
        let mut builder = builder(3);
        let first = builder.spawn(Marker).expect("first entity should fit");

        assert!(matches!(
            builder.spawn_array([Marker, Marker, Marker]),
            Err(WorldBuildError::EntityLimitExceeded { limit: 3 })
        ));
        assert_eq!(builder.managed_entities, 1);
        assert_eq!(
            builder
                .world
                .iter_entities()
                .filter(|entity| entity.contains::<ManagedEntity>())
                .count(),
            1
        );
        assert!(builder.world.get::<Marker>(first.entity()).is_some());
        assert!(builder.spawn_array([Marker, Marker]).is_ok());
        assert_eq!(builder.managed_entities, 3);
    }

    #[test]
    fn array_count_overflow_is_rejected_before_spawning() {
        let mut builder = builder(usize::MAX);
        builder.managed_entities = usize::MAX;

        assert!(matches!(
            builder.spawn_array([Marker]),
            Err(WorldBuildError::EntityLimitExceeded { limit: usize::MAX })
        ));
        assert!(
            builder
                .world
                .iter_entities()
                .all(|entity| !entity.contains::<ManagedEntity>())
        );
    }

    #[test]
    fn bundle_validation_precedes_array_limit_validation() {
        let mut builder = builder(1);
        assert!(builder.spawn(Marker).is_ok());

        assert!(matches!(
            builder.spawn_array([Unapproved]),
            Err(WorldBuildError::Component(
                ComponentApprovalError::UnapprovedBundleComponent { .. }
            ))
        ));
        assert_eq!(builder.managed_entities, 1);
    }

    #[test]
    fn rejected_array_drops_every_input_once() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Marker>().is_ok());
        assert!(registry.approve::<DropProbe>().is_ok());
        let mut builder = builder_from_registry(1, registry);
        assert!(builder.spawn(Marker).is_ok());
        let drops = Arc::new(AtomicUsize::new(0));

        let result = builder.spawn_array([
            DropProbe(Arc::clone(&drops)),
            DropProbe(Arc::clone(&drops)),
            DropProbe(Arc::clone(&drops)),
        ]);

        assert!(matches!(
            result,
            Err(WorldBuildError::EntityLimitExceeded { limit: 1 })
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 3);
        assert_eq!(builder.managed_entities, 1);
    }

    #[test]
    fn array_spawn_materializes_required_defaults_and_preserves_explicit_values() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<NeedsRequired>().is_ok());
        assert!(registry.approve::<RequiredValue>().is_ok());
        let mut builder = builder_from_registry(4, registry);

        let implicit = builder
            .spawn_array([NeedsRequired, NeedsRequired])
            .expect("approved requirements should materialize");
        let explicit = builder
            .spawn_array([
                (NeedsRequired, RequiredValue(4)),
                (NeedsRequired, RequiredValue(8)),
            ])
            .expect("explicit requirements should take precedence");

        for handle in implicit {
            assert_eq!(
                builder.world.get::<RequiredValue>(handle.entity()),
                Some(&RequiredValue::default())
            );
        }
        for (handle, expected) in explicit.into_iter().zip([4, 8]) {
            assert_eq!(
                builder.world.get::<RequiredValue>(handle.entity()),
                Some(&RequiredValue(expected))
            );
        }
    }

    #[test]
    fn disabled_array_entities_consume_capacity_and_use_only_final_archetype() {
        let mut builder = builder(2);
        let handles = builder
            .spawn_array([(Marker, Disabled), (Marker, Disabled)])
            .expect("disabled entities should be accepted");
        let marker = builder
            .world
            .component_id::<Marker>()
            .expect("registered Marker");
        let managed = builder
            .world
            .component_id::<ManagedEntity>()
            .expect("registered ManagedEntity");

        for handle in handles {
            assert!(builder.world.get::<Disabled>(handle.entity()).is_some());
            assert!(builder.world.get::<Marker>(handle.entity()).is_some());
        }
        assert_eq!(builder.managed_entities, 2);
        assert!(
            !builder
                .world
                .archetypes()
                .iter()
                .any(|archetype| archetype.contains(marker) && !archetype.contains(managed))
        );
        assert!(matches!(
            builder.spawn(Marker),
            Err(WorldBuildError::EntityLimitExceeded { limit: 2 })
        ));
    }

    #[test]
    fn disabled_entities_still_consume_the_hard_candidate_limit() {
        let mut builder = builder(1);
        assert!(builder.spawn((Marker, Disabled)).is_ok());

        assert!(matches!(
            builder.spawn(Marker),
            Err(WorldBuildError::EntityLimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn despawn_releases_candidate_capacity_and_finish_reports_exact_count() {
        let mut builder = builder(1);
        let first = builder.spawn(Marker).expect("first entity should fit");
        assert!(builder.despawn(first).is_ok());
        assert!(builder.spawn((Marker, Disabled)).is_ok());

        let (world, _, managed_entities) = builder.finish();
        let actual = world
            .iter_entities()
            .filter(|entity| entity.contains::<ManagedEntity>())
            .count();
        assert_eq!(managed_entities, 1);
        assert_eq!(managed_entities, actual);
    }

    #[test]
    fn repeated_despawn_remains_a_missing_entity_error() {
        let mut builder = builder(1);
        let entity = builder.spawn(Marker).expect("first entity should fit");
        assert!(builder.despawn(entity).is_ok());

        assert!(matches!(
            builder.despawn(entity),
            Err(WorldBuildError::MissingEntity { .. })
        ));
    }

    #[test]
    fn despawn_rejects_a_row_with_mismatched_private_provenance() {
        let mut builder = builder(1);
        let entity = builder.spawn(Marker).expect("managed spawn should succeed");
        let corrupted = WorldGeneration::new(ApplicationId::from_raw(1), 99);
        builder
            .world
            .entity_mut(entity.entity())
            .insert(ManagedEntity::for_generation(corrupted));

        assert!(matches!(
            builder.despawn(entity),
            Err(WorldBuildError::MissingEntity { entity: rejected }) if rejected == entity
        ));
        assert_eq!(builder.managed_entities, 1);
    }

    #[test]
    fn resources_are_inserted_without_raw_world_access() {
        let mut builder = builder(1);
        assert!(builder.insert_resource(Setting(7)).is_ok());
        let (world, _, _) = builder.finish();

        assert_eq!(world.get_resource::<Setting>(), Some(&Setting(7)));
    }

    #[test]
    fn resources_with_lifecycle_hooks_are_rejected() {
        let mut builder = builder(1);
        let result = builder.insert_resource(HookedSetting);

        assert!(matches!(
            result,
            Err(WorldBuildError::Component(
                ComponentApprovalError::LifecycleHook {
                    component,
                    hook: LifecycleHook::Add,
                }
            )) if component == std::any::type_name::<HookedSetting>()
        ));
    }

    #[test]
    fn resource_required_components_are_rejected_but_is_resource_is_allowed() {
        let mut builder = builder(1);
        assert!(builder.insert_resource(Setting(7)).is_ok());

        let result = builder.insert_resource(SettingWithRequiredComponent);
        let (component, required) = match result {
            Err(WorldBuildError::Component(
                ComponentApprovalError::UnapprovedRequiredComponent {
                    component,
                    required,
                },
            )) => (component, required),
            Err(error) => panic!("unexpected resource validation error: {error}"),
            Ok(_) => panic!("resource with a user-required component was accepted"),
        };

        assert_eq!(
            component,
            std::any::type_name::<SettingWithRequiredComponent>()
        );
        assert!(!required.is_empty());
    }
}

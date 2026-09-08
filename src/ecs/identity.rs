//! Opaque runtime identities.

use std::{
    error::Error,
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

use bevy_ecs::{component::Component, entity::Entity, prelude::Resource, query::QueryData};

static NEXT_APPLICATION_ID: AtomicU64 = AtomicU64::new(1);

/// Failure to issue another never-reused runtime identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdentityExhausted;

impl fmt::Display for IdentityExhausted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Sim;Logic runtime identity space is exhausted")
    }
}

impl Error for IdentityExhausted {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ApplicationId(u64);

impl ApplicationId {
    pub(crate) fn issue() -> Result<Self, IdentityExhausted> {
        NEXT_APPLICATION_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map(Self)
            .map_err(|_| IdentityExhausted)
    }

    #[cfg(test)]
    pub(crate) const fn from_raw(value: u64) -> Self {
        Self(value)
    }
}

/// Identifies one prepared ECS world within one application instance.
///
/// Generations are issued by the runtime and never reused. A generation may be
/// absent from the active application when preparation of its candidate fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldGeneration {
    application: ApplicationId,
    sequence: u64,
}

impl WorldGeneration {
    pub(crate) const fn new(application: ApplicationId, sequence: u64) -> Self {
        Self {
            application,
            sequence,
        }
    }

    #[cfg(test)]
    pub(crate) const fn belongs_to(self, application: ApplicationId) -> bool {
        self.application.0 == application.0
    }
}

/// Identifies one registered no-payload World factory.
///
/// The handle is meaningful only to the application that issued it. Its
/// internal registry slot and registration generation are intentionally
/// inaccessible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WorldFactoryId {
    application: ApplicationId,
    slot: u32,
    registration_generation: u32,
}

impl WorldFactoryId {
    pub(crate) const fn new(
        application: ApplicationId,
        slot: u32,
        registration_generation: u32,
    ) -> Self {
        Self {
            application,
            slot,
            registration_generation,
        }
    }

    pub(crate) const fn application(self) -> ApplicationId {
        self.application
    }

    pub(crate) const fn slot(self) -> u32 {
        self.slot
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum IntentKind {
    Input,
    Direct,
}

/// A runtime-issued token identifying one causal transition occurrence.
///
/// Input-derived tokens may be used only while their input edge is being
/// delivered. Direct tokens are valid only during their issuance frame. Tokens
/// are runtime handles and are not stable or serializable identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TransitionIntentToken {
    application: ApplicationId,
    origin: WorldGeneration,
    issued_frame: u64,
    occurrence: u64,
    kind: IntentKind,
}

impl TransitionIntentToken {
    pub(crate) const fn input(
        application: ApplicationId,
        origin: WorldGeneration,
        issued_frame: u64,
        occurrence: u64,
    ) -> Self {
        Self {
            application,
            origin,
            issued_frame,
            occurrence,
            kind: IntentKind::Input,
        }
    }

    pub(crate) const fn direct(
        application: ApplicationId,
        origin: WorldGeneration,
        issued_frame: u64,
        occurrence: u64,
    ) -> Self {
        Self {
            application,
            origin,
            issued_frame,
            occurrence,
            kind: IntentKind::Direct,
        }
    }

    pub(crate) const fn application(self) -> ApplicationId {
        self.application
    }

    pub(crate) const fn origin(self) -> WorldGeneration {
        self.origin
    }

    pub(crate) const fn issued_frame(self) -> u64 {
        self.issued_frame
    }

    pub(crate) const fn kind(self) -> IntentKind {
        self.kind
    }
}

/// An entity identity scoped to one application and World generation.
///
/// Sim;Logic returns this opaque handle for managed entities and keeps its ECS
/// storage private. Structural APIs accept the handle instead of a raw Bevy
/// Entity so a stale identity cannot accidentally address an entity in a
/// replacement World.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LogicEntity {
    application: ApplicationId,
    generation: WorldGeneration,
    entity: Entity,
}

/// Private ECS storage for the World generation of a managed entity.
///
/// The raw Bevy entity is supplied by the query row when reconstructing the
/// public handle. Keeping this component private prevents ordinary systems
/// from replacing runtime provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub(crate) struct ManagedEntity(WorldGeneration);

#[derive(Debug, Clone, Copy, Resource)]
pub(crate) struct WorldIdentityState {
    application: ApplicationId,
    generation: WorldGeneration,
}

impl WorldIdentityState {
    pub(crate) const fn new(application: ApplicationId, generation: WorldGeneration) -> Self {
        Self {
            application,
            generation,
        }
    }

    pub(crate) fn owns(self, entity: LogicEntity) -> bool {
        entity.application.0 == self.application.0 && entity.generation == self.generation
    }

    pub(crate) const fn generation(self) -> WorldGeneration {
        self.generation
    }
}

impl ManagedEntity {
    pub(crate) const fn for_generation(generation: WorldGeneration) -> Self {
        Self(generation)
    }

    #[cfg(test)]
    pub(crate) const fn new(entity: LogicEntity) -> Self {
        Self(entity.world_generation())
    }

    pub(crate) const fn handle(self, entity: Entity) -> LogicEntity {
        LogicEntity::new(self.0.application, self.0, entity)
    }
}

/// Read-only query data that yields an entity's managed Sim;Logic identity.
///
/// Use this inside a managed [`crate::query::Query`], for example
/// `Query<(LogicEntityRef, &Health)>`. The private immutable field prevents a
/// System from rewriting provenance. Managed point lookup accepts the
/// resulting handle and does not expose raw Bevy entity lookup.
#[derive(QueryData)]
pub struct LogicEntityRef {
    entity: Entity,
    identity: &'static ManagedEntity,
}

impl LogicEntityRefItem<'_, '_> {
    /// Copies the opaque managed handle carried by this query item.
    pub const fn handle(&self) -> LogicEntity {
        self.identity.handle(self.entity)
    }
}

impl LogicEntity {
    pub(crate) const fn new(
        application: ApplicationId,
        generation: WorldGeneration,
        entity: Entity,
    ) -> Self {
        Self {
            application,
            generation,
            entity,
        }
    }

    /// Returns the World generation that owns this entity.
    pub const fn world_generation(self) -> WorldGeneration {
        self.generation
    }

    pub(crate) const fn application(self) -> ApplicationId {
        self.application
    }

    pub(crate) const fn entity(self) -> Entity {
        self.entity
    }

    pub(crate) const fn stable_bits(self) -> u64 {
        self.entity.to_bits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_keep_application_provenance() {
        let first = WorldGeneration::new(ApplicationId::from_raw(1), 7);
        let second = WorldGeneration::new(ApplicationId::from_raw(2), 7);

        assert_ne!(first, second);
        assert!(first.belongs_to(ApplicationId::from_raw(1)));
        assert!(!first.belongs_to(ApplicationId::from_raw(2)));
    }

    #[test]
    fn managed_storage_reconstructs_the_exact_raw_entity_without_storing_it() {
        let application = ApplicationId::from_raw(9);
        let generation = WorldGeneration::new(application, 17);
        let raw = Entity::from_bits(0x0000_0007_0000_002a);
        let managed = ManagedEntity::for_generation(generation);

        assert_eq!(
            managed.handle(raw),
            LogicEntity::new(application, generation, raw)
        );
        assert_eq!(
            std::mem::size_of::<ManagedEntity>(),
            std::mem::size_of::<WorldGeneration>()
        );
        assert!(std::mem::size_of::<ManagedEntity>() < std::mem::size_of::<LogicEntity>());
    }
}

//! Generation-checked component queries for managed Systems.

use std::{
    error::Error,
    fmt,
    ops::{Deref, DerefMut},
};

use bevy_ecs::{
    entity_disabling::Disabled,
    query::{
        Allow, IterQueryData, QueryData, QueryFilter, QueryIter as BevyQueryIter,
        QuerySingleError as BevyQuerySingleError, ROQueryItem, With, Without,
    },
    system::{Query as BevyQuery, Single as BevySingle, SystemParam},
};

use crate::identity::{LogicEntity, ManagedEntity, WorldGeneration, WorldIdentityState};

type ManagedFilter<F> = (F, Allow<Disabled>, Without<Disabled>, With<ManagedEntity>);

/// A managed entity cannot be resolved by a component query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryEntityError {
    /// The handle belongs to another application or World generation.
    ForeignWorld {
        /// Rejected managed handle.
        entity: LogicEntity,
        /// Generation against which the query was running.
        active: WorldGeneration,
    },
    /// The entity is no longer alive or does not match the lookup rules.
    DoesNotMatch {
        /// Managed handle that could not produce an item.
        entity: LogicEntity,
    },
}

impl fmt::Display for QueryEntityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignWorld { entity, active } => write!(
                formatter,
                "entity {entity:?} does not belong to active generation {active:?}"
            ),
            Self::DoesNotMatch { entity } => {
                write!(formatter, "entity {entity:?} is absent or does not match")
            }
        }
    }
}

impl Error for QueryEntityError {}

/// A managed query expected exactly one matching enabled entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuerySingleError {
    /// No enabled managed entity matched the query.
    NoEntities,
    /// More than one enabled managed entity matched the query.
    MultipleEntities,
}

impl fmt::Display for QuerySingleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntities => formatter.write_str("no enabled managed entity matches the query"),
            Self::MultipleEntities => {
                formatter.write_str("more than one enabled managed entity matches the query")
            }
        }
    }
}

impl Error for QuerySingleError {}

/// Typed component access for one active Sim;Logic World generation.
///
/// Iteration follows Bevy ECS borrowing and filtering rules, but never yields
/// a raw Bevy entity identifier. Use [`crate::identity::LogicEntityRef`] in the
/// query data when a stable managed handle is needed. Point lookup accepts only
/// [`LogicEntity`] and rejects handles from a retired or foreign World before
/// touching component storage. Within the active World, it also requires the
/// exact live managed identity at that ECS row before fetching user data.
///
/// Disabled entities are always excluded in this first slice, independently
/// of Bevy's mutable global default-query-filter Resource.
#[derive(SystemParam)]
pub struct Query<'w, 's, D: QueryData + 'static, F: QueryFilter + 'static = ()> {
    inner: BevyQuery<'w, 's, D, ManagedFilter<F>>,
    identities: BevyQuery<'w, 's, &'static ManagedEntity, ManagedFilter<()>>,
    identity: bevy_ecs::prelude::Res<'w, WorldIdentityState>,
}

/// Iterator over enabled, runtime-managed entities selected by [`Query`].
///
/// This wrapper keeps Sim;Logic's private managed-entity filter out of the
/// public type signature while preserving allocation-free ECS iteration.
pub struct QueryIterator<'w, 's, D: QueryData, F: QueryFilter> {
    inner: BevyQueryIter<'w, 's, D, ManagedFilter<F>>,
}

/// Exactly one enabled, runtime-managed entity selected for a System.
///
/// Cardinality is a System precondition: if zero or multiple entities match,
/// parameter validation fails before the System body runs and Sim;Logic stops
/// the current stage with a [`crate::system::SystemRunFailure`]. Use
/// [`Query::single`] when absence or multiplicity is expected control flow
/// that the System should handle itself.
///
/// Like [`Query`], this parameter always requires Sim;Logic's private managed
/// identity and excludes [`Disabled`] entities independently of Bevy's mutable
/// global default-query-filter Resource. [`crate::identity::LogicEntityRef`]
/// is the supported way to include the opaque entity handle in `D`.
#[derive(SystemParam)]
pub struct Single<'w, 's, D: IterQueryData + 'static, F: QueryFilter + 'static = ()> {
    inner: BevySingle<'w, 's, D, ManagedFilter<F>>,
}

impl<'w, 's, D: IterQueryData + 'static, F: QueryFilter + 'static> Deref for Single<'w, 's, D, F> {
    type Target = D::Item<'w, 's>;

    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<'w, 's, D: IterQueryData + 'static, F: QueryFilter + 'static> DerefMut
    for Single<'w, 's, D, F>
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<'w, 's, D: IterQueryData + 'static, F: QueryFilter + 'static> Single<'w, 's, D, F> {
    /// Returns the fetched query item without performing another lookup.
    ///
    /// This is useful for tuple data containing mutable component borrows.
    pub fn into_inner(self) -> D::Item<'w, 's> {
        self.inner.into_inner()
    }
}

impl<'w, 's, D: IterQueryData, F: QueryFilter> Iterator for QueryIterator<'w, 's, D, F> {
    type Item = D::Item<'w, 's>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<'s, D: QueryData + 'static, F: QueryFilter + 'static> Query<'_, 's, D, F> {
    /// Iterates over read-only forms of all matching enabled entities.
    ///
    /// Iteration order is not stable API.
    pub fn iter(&self) -> QueryIterator<'_, 's, D::ReadOnly, F> {
        QueryIterator {
            inner: self.inner.iter(),
        }
    }

    /// Iterates over all matching enabled entities with the access declared by
    /// `D`. Iteration order is not stable API.
    pub fn iter_mut(&mut self) -> QueryIterator<'_, 's, D, F>
    where
        D: IterQueryData,
    {
        QueryIterator {
            inner: self.inner.iter_mut(),
        }
    }

    /// Reads the only matching enabled managed entity.
    ///
    /// Returns [`QuerySingleError::NoEntities`] or
    /// [`QuerySingleError::MultipleEntities`] when cardinality is not exactly
    /// one.
    pub fn single(&self) -> Result<ROQueryItem<'_, 's, D>, QuerySingleError> {
        self.inner.single().map_err(map_single_error)
    }

    /// Accesses the only matching enabled managed entity mutably.
    ///
    /// Returns [`QuerySingleError::NoEntities`] or
    /// [`QuerySingleError::MultipleEntities`] when cardinality is not exactly
    /// one.
    pub fn single_mut(&mut self) -> Result<D::Item<'_, 's>, QuerySingleError>
    where
        D: IterQueryData,
    {
        self.inner.single_mut().map_err(map_single_error)
    }

    /// Reads one matching enabled entity after checking application and World
    /// provenance.
    pub fn get(&self, entity: LogicEntity) -> Result<ROQueryItem<'_, 's, D>, QueryEntityError> {
        self.validate(entity)?;
        self.inner
            .get(entity.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity })
    }

    /// Accesses one matching enabled entity mutably after checking application
    /// and World provenance.
    pub fn get_mut(&mut self, entity: LogicEntity) -> Result<D::Item<'_, 's>, QueryEntityError> {
        self.validate(entity)?;
        self.inner
            .get_mut(entity.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity })
    }

    fn validate(&self, entity: LogicEntity) -> Result<(), QueryEntityError> {
        if !self.identity.owns(entity) {
            return Err(QueryEntityError::ForeignWorld {
                entity,
                active: self.identity.generation(),
            });
        }
        if !self
            .identities
            .get(entity.entity())
            .is_ok_and(|managed| managed.handle(entity.entity()) == entity)
        {
            return Err(QueryEntityError::DoesNotMatch { entity });
        }
        Ok(())
    }
}

fn map_single_error(error: BevyQuerySingleError) -> QuerySingleError {
    match error {
        BevyQuerySingleError::NoEntities(_) => QuerySingleError::NoEntities,
        BevyQuerySingleError::MultipleEntities(_) => QuerySingleError::MultipleEntities,
    }
}

impl<'w, 's, D, F> IntoIterator for &'w Query<'_, 's, D, F>
where
    D: QueryData + 'static,
    F: QueryFilter + 'static,
{
    type Item = ROQueryItem<'w, 's, D>;
    type IntoIter = QueryIterator<'w, 's, D::ReadOnly, F>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'w, 's, D, F> IntoIterator for &'w mut Query<'_, 's, D, F>
where
    D: IterQueryData + 'static,
    F: QueryFilter + 'static,
{
    type Item = D::Item<'w, 's>;
    type IntoIter = QueryIterator<'w, 's, D, F>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::{
        entity_disabling::Disabled,
        prelude::{Component, Res, ResMut, Resource},
        query::Allow,
        world::World,
    };

    use crate::{
        identity::{
            ApplicationId, LogicEntity, LogicEntityRef, ManagedEntity, WorldGeneration,
            WorldIdentityState,
        },
        system::{Stage, StageFactories, SystemRunFailure},
    };

    use super::*;

    #[derive(Component)]
    struct Marker(u8);

    #[derive(Resource, Default)]
    struct Observation(Option<Result<u8, QuerySingleError>>);

    #[derive(Resource, Default)]
    struct RequiredSingleObservation {
        runs: usize,
        value: Option<u8>,
        entity: Option<LogicEntity>,
    }

    #[derive(Resource, Clone, Copy)]
    struct PointLookupTarget(LogicEntity);

    #[derive(Resource, Default)]
    struct PointLookupObservation {
        read: Option<Result<LogicEntity, QueryEntityError>>,
        write: Option<Result<(), QueryEntityError>>,
    }

    fn observe_single(markers: Query<&Marker>, mut observation: ResMut<Observation>) {
        observation.0 = Some(markers.single().map(|marker| marker.0));
    }

    fn increment_single(mut markers: Query<&mut Marker>) {
        if let Ok(mut marker) = markers.single_mut() {
            marker.0 += 1;
        }
    }

    fn observe_required_single(
        marker: Single<&Marker>,
        mut observation: ResMut<RequiredSingleObservation>,
    ) {
        observation.runs += 1;
        observation.value = Some(marker.0);
    }

    fn observe_required_single_tuple(
        marker: Single<(LogicEntityRef, &Marker)>,
        mut observation: ResMut<RequiredSingleObservation>,
    ) {
        let (entity, marker) = marker.into_inner();
        observation.runs += 1;
        observation.value = Some(marker.0);
        observation.entity = Some(entity.handle());
    }

    fn increment_required_single(mut marker: Single<&mut Marker>) {
        marker.0 += 1;
    }

    fn observe_point_lookup(
        target: Res<PointLookupTarget>,
        markers: Query<(LogicEntityRef, &Marker)>,
        mut observation: ResMut<PointLookupObservation>,
    ) {
        observation.read = Some(
            markers
                .get(target.0)
                .map(|(identity, _marker)| identity.handle()),
        );
    }

    fn mutate_point_lookup(
        target: Res<PointLookupTarget>,
        mut markers: Query<&mut Marker>,
        mut observation: ResMut<PointLookupObservation>,
    ) {
        observation.write = Some(markers.get_mut(target.0).map(|mut marker| marker.0 += 1));
    }

    fn managed_world(values: &[(u8, bool)]) -> World {
        let application = ApplicationId::from_raw(1);
        let generation = WorldGeneration::new(application, 1);
        let mut world = World::new();
        world.insert_resource(WorldIdentityState::new(application, generation));
        world.insert_resource(Observation::default());
        world.insert_resource(RequiredSingleObservation::default());
        for (value, disabled) in values {
            let raw = if *disabled {
                world.spawn((Marker(*value), Disabled)).id()
            } else {
                world.spawn(Marker(*value)).id()
            };
            let entity = LogicEntity::new(application, generation, raw);
            world.entity_mut(raw).insert(ManagedEntity::new(entity));
        }
        world
    }

    fn run_system_result<S, MarkerType>(
        world: &mut World,
        system: S,
    ) -> Result<(), SystemRunFailure>
    where
        S: bevy_ecs::system::SystemParamFunction<MarkerType, In = (), Out = ()>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: crate::system::SupportedSystemParamTuple,
        MarkerType: 'static,
    {
        let mut factories = StageFactories::default();
        factories.add(system);
        let mut stage = factories
            .instantiate(world, Stage::FixedUpdate)
            .expect("query test System should initialize");
        stage.run(world)
    }

    fn run_system<S, MarkerType>(world: &mut World, system: S)
    where
        S: bevy_ecs::system::SystemParamFunction<MarkerType, In = (), Out = ()>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: crate::system::SupportedSystemParamTuple,
        MarkerType: 'static,
    {
        run_system_result(world, system).expect("query test System should run");
    }

    #[test]
    fn single_reports_zero_one_and_multiple_enabled_entities() {
        for (values, expected) in [
            (&[][..], Err(QuerySingleError::NoEntities)),
            (&[(7, false)][..], Ok(7)),
            (
                &[(7, false), (9, false)][..],
                Err(QuerySingleError::MultipleEntities),
            ),
            (&[(7, false), (9, true)][..], Ok(7)),
            (&[(9, true)][..], Err(QuerySingleError::NoEntities)),
        ] {
            let mut world = managed_world(values);
            run_system(&mut world, observe_single);
            assert_eq!(world.resource::<Observation>().0, Some(expected));
        }
    }

    #[test]
    fn single_mut_changes_the_only_enabled_managed_entity() {
        let mut world = managed_world(&[(4, false), (9, true)]);
        run_system(&mut world, increment_single);

        let values: Vec<_> = world
            .query_filtered::<&Marker, Allow<Disabled>>()
            .iter(&world)
            .map(|value| value.0)
            .collect();
        assert!(values.contains(&5));
        assert!(values.contains(&9));
    }

    #[test]
    fn required_single_dereferences_the_only_enabled_managed_entity() {
        let mut world = managed_world(&[(7, false), (9, true)]);
        run_system(&mut world, observe_required_single);

        let observation = world.resource::<RequiredSingleObservation>();
        assert_eq!(observation.runs, 1);
        assert_eq!(observation.value, Some(7));
    }

    #[test]
    fn required_single_cardinality_failure_stops_before_the_system_body() {
        for (values, expected_reason) in [
            (&[][..], "No matching entities"),
            (&[(7, false), (9, false)][..], "Multiple matching entities"),
            (&[(7, true)][..], "No matching entities"),
        ] {
            let mut world = managed_world(values);
            let error = run_system_result(&mut world, observe_required_single)
                .expect_err("invalid cardinality should stop the managed System");

            assert!(
                error.reason().contains(expected_reason),
                "unexpected parameter validation reason: {}",
                error.reason()
            );
            assert!(
                !error.reason().contains("ManagedEntity"),
                "private managed filter leaked into diagnostics: {}",
                error.reason()
            );
            assert_eq!(world.resource::<RequiredSingleObservation>().runs, 0);
        }
    }

    #[test]
    fn required_single_tuple_returns_the_managed_identity_without_an_extra_lookup() {
        let mut world = managed_world(&[(11, false)]);
        let (raw, managed, _) = world
            .query::<(bevy_ecs::entity::Entity, &ManagedEntity, &Marker)>()
            .single(&world)
            .expect("test World should have one managed marker");
        let expected = managed.handle(raw);

        run_system(&mut world, observe_required_single_tuple);

        let observation = world.resource::<RequiredSingleObservation>();
        assert_eq!(observation.runs, 1);
        assert_eq!(observation.value, Some(11));
        assert_eq!(observation.entity, Some(expected));
    }

    #[test]
    fn required_single_mutates_only_the_enabled_managed_entity() {
        let mut world = managed_world(&[(4, false), (9, true)]);
        run_system(&mut world, increment_required_single);

        let values: Vec<_> = world
            .query_filtered::<&Marker, Allow<Disabled>>()
            .iter(&world)
            .map(|value| value.0)
            .collect();
        assert!(values.contains(&5));
        assert!(values.contains(&9));
    }

    #[test]
    fn point_lookups_reject_a_row_with_mismatched_private_provenance() {
        let mut world = managed_world(&[(7, false)]);
        let (raw, managed) = world
            .query::<(bevy_ecs::entity::Entity, &ManagedEntity)>()
            .single(&world)
            .expect("test World should have one managed marker");
        let target = managed.handle(raw);
        world.insert_resource(PointLookupTarget(target));
        world.insert_resource(PointLookupObservation::default());
        let corrupted = WorldGeneration::new(ApplicationId::from_raw(1), 99);
        world
            .entity_mut(raw)
            .insert(ManagedEntity::for_generation(corrupted));

        run_system(&mut world, observe_point_lookup);
        run_system(&mut world, mutate_point_lookup);

        let observation = world.resource::<PointLookupObservation>();
        assert!(matches!(
            observation.read,
            Some(Err(QueryEntityError::DoesNotMatch { entity })) if entity == target
        ));
        assert!(matches!(
            observation.write,
            Some(Err(QueryEntityError::DoesNotMatch { entity })) if entity == target
        ));
        assert_eq!(world.get::<Marker>(raw).map(|marker| marker.0), Some(7));
    }
}

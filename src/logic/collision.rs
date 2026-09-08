//! Basic collision shapes and live overlap queries.

use std::{error::Error, fmt};

use bevy_ecs::{
    entity::Entity,
    entity_disabling::Disabled,
    prelude::{Component, Res},
    query::{Allow, QueryFilter, With, Without},
    system::{Query as BevyQuery, SystemParam},
};
use sim_engine::Vec2;

use crate::{
    identity::{LogicEntity, ManagedEntity, WorldIdentityState},
    query::QueryEntityError,
    visual::{CircleVisual, RectangleVisual, Transform2d},
};

type CircleColliderData = (
    Entity,
    &'static ManagedEntity,
    &'static Transform2d,
    &'static CircleCollider2d,
);
type RectangleColliderData = (
    Entity,
    &'static ManagedEntity,
    &'static Transform2d,
    &'static RectangleCollider2d,
);
type EnabledManaged = (Allow<Disabled>, Without<Disabled>, With<ManagedEntity>);
type EnabledManagedWith<F> = (F, Allow<Disabled>, Without<Disabled>, With<ManagedEntity>);

/// A circular collision area centered on an entity's [`Transform2d`].
///
/// This component describes overlap geometry only. It does not apply forces,
/// move entities, emit events, or automatically inherit and track a radius
/// from [`crate::visual::CircleVisual`]. [`Self::from_visual`] can explicitly
/// copy the initial radius once; it does not copy a transform or synchronize
/// later changes. Keeping logic and presentation separate permits invisible
/// triggers and hit areas that differ from their visuals.
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. The canonical translation of a
/// transform supplied in the same bundle takes precedence; normal spawn
/// snapping may reset its previous interpolation endpoint to that translation.
/// To exclude an initially spawned collider from standard overlap queries,
/// attach the automatically approved
/// [`bevy_ecs::entity_disabling::Disabled`], or use
/// [`crate::commands::LogicCommands::disable`] at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct CircleCollider2d {
    radius: f32,
}

impl CircleCollider2d {
    /// Creates a collider with a finite, strictly positive world-space radius.
    pub fn new(radius: f32) -> Result<Self, CircleColliderError> {
        validate_radius(radius)?;
        Ok(Self { radius })
    }

    /// Copies the initial radius of a circle visual into a new collider.
    ///
    /// This is a one-time snapshot, not synchronization, and it does not copy
    /// a transform. Later radius changes on either value do not affect the
    /// other. Use [`Self::new`] when the hit area should intentionally differ
    /// from the visual.
    pub const fn from_visual(visual: &CircleVisual) -> Self {
        // CircleVisual's safe API enforces the same positive-finite invariant.
        Self {
            radius: visual.radius(),
        }
    }

    /// Returns the radius in caller-defined world units.
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// Replaces the radius after validation.
    ///
    /// A rejected value leaves the previous radius unchanged.
    pub fn set_radius(&mut self, radius: f32) -> Result<(), CircleColliderError> {
        validate_radius(radius)?;
        self.radius = radius;
        Ok(())
    }

    /// Reports whether two colliders overlap at their canonical transforms.
    ///
    /// Tangent circles count as overlapping. Calculation widens coordinates
    /// and radii to `f64` before arithmetic, so subtraction and squaring do not
    /// overflow for finite `f32` values. Interpolated render positions are not
    /// involved.
    pub fn overlaps(
        &self,
        transform: &Transform2d,
        other: &Self,
        other_transform: &Transform2d,
    ) -> bool {
        circles_overlap(
            transform.translation(),
            self.radius,
            other_transform.translation(),
            other.radius,
        )
    }

    /// Reports whether this circle overlaps an axis-aligned rectangle.
    ///
    /// The rectangle uses its full size and the center supplied by
    /// `rectangle_transform`. Circle center containment plus face or corner
    /// contact count as overlap. Every coordinate and geometry value widens
    /// to `f64` before arithmetic; render interpolation, visual corner radius,
    /// rotation, and scale are not involved.
    pub fn overlaps_rectangle(
        &self,
        transform: &Transform2d,
        rectangle: &RectangleCollider2d,
        rectangle_transform: &Transform2d,
    ) -> bool {
        circle_rectangle_overlap(
            transform.translation(),
            self.radius,
            rectangle_transform.translation(),
            rectangle.size,
        )
    }
}

impl CircleVisual {
    /// Returns `(visual, collider)` as a spawnable pair matching this radius.
    ///
    /// The collider is a one-time geometry snapshot produced by
    /// [`CircleCollider2d::from_visual`]. Later changes to either returned
    /// value remain independent. The tuple can be nested directly in a larger
    /// spawn bundle.
    pub const fn with_matching_collider(self) -> (Self, CircleCollider2d) {
        let collider = CircleCollider2d::from_visual(&self);
        (self, collider)
    }
}

/// Invalid radius supplied to [`CircleCollider2d`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircleColliderError {
    /// A circle radius was non-finite or not strictly positive.
    InvalidRadius {
        /// Rejected radius in world units.
        value: f32,
    },
}

impl fmt::Display for CircleColliderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRadius { value } => write!(
                formatter,
                "collider radius must be positive and finite, got {value}"
            ),
        }
    }
}

impl Error for CircleColliderError {}

/// An axis-aligned rectangular collision area centered on an entity's
/// [`Transform2d`].
///
/// `size` is the full width and height in caller-defined world units. This
/// component describes overlap geometry only: it does not automatically
/// inherit or track geometry from [`crate::visual::RectangleVisual`], move
/// entities, or resolve collisions. [`Self::from_visual`] can explicitly copy
/// the initial full size once, but it ignores rounded corners, does not copy a
/// transform, and does not synchronize later changes. Keeping geometry
/// independent permits invisible hit areas and visuals whose shape differs
/// from gameplay. Spawning this component without a [`Transform2d`]
/// automatically inserts a default transform at the world origin. The
/// canonical translation of a transform supplied in the same bundle takes
/// precedence; normal spawn snapping may reset its previous interpolation
/// endpoint to that translation.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct RectangleCollider2d {
    size: Vec2,
}

impl RectangleCollider2d {
    /// Creates a collider with finite, strictly positive full width and height.
    pub fn new(size: Vec2) -> Result<Self, RectangleColliderError> {
        validate_rectangle_size(size)?;
        Ok(Self { size })
    }

    /// Copies the initial full size of a rectangle visual into a new collider.
    ///
    /// Corner radius and presentation properties are not collision geometry
    /// and are ignored. This is a one-time snapshot, not synchronization, and
    /// it does not copy a transform. Later size changes on either value do not
    /// affect the other. Use [`Self::new`] for deliberately different or
    /// invisible hit areas.
    pub const fn from_visual(visual: &RectangleVisual) -> Self {
        // RectangleVisual's safe API enforces the same positive-finite invariant.
        Self {
            size: visual.size(),
        }
    }

    /// Returns the full width and height in caller-defined world units.
    pub const fn size(&self) -> Vec2 {
        self.size
    }

    /// Replaces the full width and height after validation.
    ///
    /// A rejected value leaves the complete previous size unchanged.
    pub fn set_size(&mut self, size: Vec2) -> Result<(), RectangleColliderError> {
        validate_rectangle_size(size)?;
        self.size = size;
        Ok(())
    }

    /// Reports whether two colliders overlap at their canonical transforms.
    ///
    /// Rectangles are axis-aligned and edge or corner contact counts as
    /// overlap. Calculation widens positions and sizes to `f64` before
    /// arithmetic, preventing overflow for finite `f32` values. Render
    /// interpolation, visual corner radius, rotation, and scale are not
    /// involved.
    pub fn overlaps(
        &self,
        transform: &Transform2d,
        other: &Self,
        other_transform: &Transform2d,
    ) -> bool {
        rectangles_overlap(
            transform.translation(),
            self.size,
            other_transform.translation(),
            other.size,
        )
    }

    /// Reports whether this axis-aligned rectangle overlaps a circle.
    ///
    /// This is the symmetric orientation of
    /// [`CircleCollider2d::overlaps_rectangle`] and delegates to the same
    /// widened closed-contact predicate.
    pub fn overlaps_circle(
        &self,
        transform: &Transform2d,
        circle: &CircleCollider2d,
        circle_transform: &Transform2d,
    ) -> bool {
        circle_rectangle_overlap(
            circle_transform.translation(),
            circle.radius,
            transform.translation(),
            self.size,
        )
    }
}

impl RectangleVisual {
    /// Returns `(visual, collider)` as a spawnable pair matching this full size.
    ///
    /// The collider is a one-time geometry snapshot produced by
    /// [`RectangleCollider2d::from_visual`]. Corner radius and all other
    /// presentation properties are ignored. Later changes to either returned
    /// value remain independent. The tuple can be nested directly in a larger
    /// spawn bundle.
    pub const fn with_matching_collider(self) -> (Self, RectangleCollider2d) {
        let collider = RectangleCollider2d::from_visual(&self);
        (self, collider)
    }
}

/// Invalid geometry supplied to [`RectangleCollider2d`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RectangleColliderError {
    /// A full size contained a non-finite or non-positive component.
    InvalidSize {
        /// Rejected full width and height.
        value: Vec2,
    },
}

impl fmt::Display for RectangleColliderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSize { value } => write!(
                formatter,
                "collider size components must be positive and finite, got {value:?}"
            ),
        }
    }
}

impl Error for RectangleColliderError {}

/// Live read-only search for enabled circular colliders in the active World.
///
/// `F` is a Bevy query filter applied only to candidate entities, such as
/// `With<Coin>` or `(With<Target>, Without<Projectile>)`. The source entity
/// need not match `F`, but it must still be an enabled managed entity. The
/// supplied source transform and collider define the tested geometry. The
/// runtime validates the source handle, but their association with that handle
/// is caller-enforced; ordinary use borrows both from the same source entity.
///
/// Each call to [`CircleOverlapEntities::iter_overlapping`] performs an
/// allocation-free linear scan over ECS rows visited after archetype pruning
/// by `F`. Row-level filters may inspect rows that they do not yield. Results
/// exclude the source entity and have no stable iteration order. Direct
/// component changes made by an earlier System in the same stage are visible;
/// structural [`crate::commands::LogicCommands`] remain deferred until the
/// stage barrier.
/// This is an overlap query, not a spatial index or physics solver.
///
/// The source transform and collider are explicit method arguments rather
/// than hidden reads. A separate mutable query for `Transform2d` or
/// `CircleCollider2d` must still be archetype-disjoint from this parameter's
/// candidate filter `F`; passing a source handle or proposed transform does
/// not establish disjointness for Bevy. Splitting movement and collision into
/// ordered Systems is the simplest general pattern.
#[derive(SystemParam)]
pub struct CircleOverlapEntities<'w, 's, F: QueryFilter + 'static = ()> {
    sources: BevyQuery<'w, 's, &'static ManagedEntity, EnabledManaged>,
    candidates: BevyQuery<'w, 's, CircleColliderData, EnabledManagedWith<F>>,
    identity: Res<'w, WorldIdentityState>,
}

/// Live read-only search for enabled rectangular colliders in the active World.
///
/// `F` is a Bevy query filter applied only to candidate entities, such as
/// `With<Wall>` or `(With<Target>, Without<Player>)`. The source entity need
/// not match `F`, but it must still be an enabled managed entity. The supplied
/// source transform and collider define the tested geometry. The runtime
/// validates the source handle, but their association with that handle is
/// caller-enforced; ordinary use borrows the collider from that entity while a
/// tentative movement check may pass a copied proposed transform.
///
/// Each call to [`RectangleOverlapEntities::iter_overlapping`] performs an
/// allocation-free linear scan over ECS rows visited after archetype pruning
/// by `F`. Row-level filters may inspect rows that they do not yield. Results
/// exclude the source entity and have no stable iteration order. Direct
/// component changes made by an earlier System in the same stage are visible;
/// structural [`crate::commands::LogicCommands`] remain deferred until the
/// stage barrier. This is an overlap query, not a spatial index or physics
/// solver.
///
/// The candidate query reads [`Transform2d`] and [`RectangleCollider2d`]. A
/// separate mutable query for either value must be archetype-disjoint from this
/// parameter's candidate filter `F`; passing a source handle or proposed
/// transform does not establish disjointness for Bevy.
#[derive(SystemParam)]
pub struct RectangleOverlapEntities<'w, 's, F: QueryFilter + 'static = ()> {
    sources: BevyQuery<'w, 's, &'static ManagedEntity, EnabledManaged>,
    candidates: BevyQuery<'w, 's, RectangleColliderData, EnabledManagedWith<F>>,
    identity: Res<'w, WorldIdentityState>,
}

impl<'s, F: QueryFilter + 'static> RectangleOverlapEntities<'_, 's, F> {
    /// Reports whether at least one candidate overlaps `source` right now.
    ///
    /// This is an early-exit boolean convenience over
    /// [`Self::iter_overlapping`]. It preserves the same source validation,
    /// filtering, caller-supplied geometry, and closed-contact semantics, but
    /// does not expose which unordered candidate matched. The worst case
    /// remains a linear candidate scan.
    pub fn has_overlap(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &RectangleCollider2d,
    ) -> Result<bool, QueryEntityError> {
        let mut overlaps = self.iter_overlapping(source, source_transform, source_collider)?;
        Ok(overlaps.next().is_some())
    }

    /// Iterates over candidate identities overlapping `source` right now.
    ///
    /// A foreign or retired handle is rejected before ECS lookup. A handle
    /// that is absent, unmanaged, or disabled produces
    /// [`QueryEntityError::DoesNotMatch`]. The supplied geometry is used by
    /// value. Its association with `source` is caller-enforced.
    pub fn iter_overlapping(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &RectangleCollider2d,
    ) -> Result<impl Iterator<Item = LogicEntity> + '_, QueryEntityError> {
        if !self.identity.owns(source) {
            return Err(QueryEntityError::ForeignWorld {
                entity: source,
                active: self.identity.generation(),
            });
        }

        let managed = self
            .sources
            .get(source.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity: source })?;
        if managed.handle(source.entity()) != source {
            return Err(QueryEntityError::DoesNotMatch { entity: source });
        }
        let source_position = source_transform.translation();
        let source_size = source_collider.size;

        Ok(self
            .candidates
            .iter()
            .filter_map(move |(raw, candidate, transform, collider)| {
                let candidate = candidate.handle(raw);
                (candidate != source
                    && rectangles_overlap(
                        source_position,
                        source_size,
                        transform.translation(),
                        collider.size,
                    ))
                .then_some(candidate)
            }))
    }

    /// Reports whether at least one rectangular candidate overlaps a circular source.
    ///
    /// This is exact early-exit sugar over
    /// [`Self::iter_overlapping_with_circle`]. The source geometry is supplied
    /// by the caller; the candidate filter and all ordinary managed-query
    /// validation remain unchanged.
    pub fn has_overlap_with_circle(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &CircleCollider2d,
    ) -> Result<bool, QueryEntityError> {
        let mut overlaps =
            self.iter_overlapping_with_circle(source, source_transform, source_collider)?;
        Ok(overlaps.next().is_some())
    }

    /// Iterates over rectangular candidates overlapping a circular source.
    ///
    /// Tangent face and corner contact count as overlap. A foreign or retired
    /// handle is rejected before lookup; absent, unmanaged, disabled, or
    /// private-marker-mismatched sources produce
    /// [`QueryEntityError::DoesNotMatch`]. The supplied geometry is used by
    /// value and its association with `source` is caller-enforced. `F` applies
    /// only to rectangular candidates; the source and disabled candidates are
    /// excluded. This is an allocation-free linear scan with no stable order.
    pub fn iter_overlapping_with_circle(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &CircleCollider2d,
    ) -> Result<impl Iterator<Item = LogicEntity> + '_, QueryEntityError> {
        if !self.identity.owns(source) {
            return Err(QueryEntityError::ForeignWorld {
                entity: source,
                active: self.identity.generation(),
            });
        }

        let managed = self
            .sources
            .get(source.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity: source })?;
        if managed.handle(source.entity()) != source {
            return Err(QueryEntityError::DoesNotMatch { entity: source });
        }
        let source_position = source_transform.translation();
        let source_radius = source_collider.radius;

        Ok(self
            .candidates
            .iter()
            .filter_map(move |(raw, candidate, transform, collider)| {
                let candidate = candidate.handle(raw);
                (candidate != source
                    && circle_rectangle_overlap(
                        source_position,
                        source_radius,
                        transform.translation(),
                        collider.size,
                    ))
                .then_some(candidate)
            }))
    }
}

impl<'s, F: QueryFilter + 'static> CircleOverlapEntities<'_, 's, F> {
    /// Reports whether at least one candidate overlaps `source` right now.
    ///
    /// This is an early-exit boolean convenience over
    /// [`Self::iter_overlapping`]. It preserves the same source validation,
    /// filtering, caller-supplied geometry, and tangent-contact semantics, but
    /// does not expose which unordered candidate matched. The worst case
    /// remains a linear candidate scan.
    pub fn has_overlap(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &CircleCollider2d,
    ) -> Result<bool, QueryEntityError> {
        let mut overlaps = self.iter_overlapping(source, source_transform, source_collider)?;
        Ok(overlaps.next().is_some())
    }

    /// Iterates over candidate identities overlapping `source` right now.
    ///
    /// A foreign or retired handle is rejected before ECS lookup. A handle
    /// that is absent, unmanaged, or disabled produces
    /// [`QueryEntityError::DoesNotMatch`]. The supplied geometry is used by
    /// value. Its association with `source` is caller-enforced; callers should
    /// borrow it from that entity in the same System.
    pub fn iter_overlapping(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &CircleCollider2d,
    ) -> Result<impl Iterator<Item = LogicEntity> + '_, QueryEntityError> {
        if !self.identity.owns(source) {
            return Err(QueryEntityError::ForeignWorld {
                entity: source,
                active: self.identity.generation(),
            });
        }

        let managed = self
            .sources
            .get(source.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity: source })?;
        if managed.handle(source.entity()) != source {
            return Err(QueryEntityError::DoesNotMatch { entity: source });
        }
        let source_position = source_transform.translation();
        let source_radius = source_collider.radius;

        Ok(self
            .candidates
            .iter()
            .filter_map(move |(raw, candidate, transform, collider)| {
                let candidate = candidate.handle(raw);
                (candidate != source
                    && circles_overlap(
                        source_position,
                        source_radius,
                        transform.translation(),
                        collider.radius,
                    ))
                .then_some(candidate)
            }))
    }

    /// Reports whether at least one circular candidate overlaps a rectangular source.
    ///
    /// This is exact early-exit sugar over
    /// [`Self::iter_overlapping_with_rectangle`]. The source geometry is
    /// supplied by the caller; filtering and source validation are identical
    /// to the same-shape path.
    pub fn has_overlap_with_rectangle(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &RectangleCollider2d,
    ) -> Result<bool, QueryEntityError> {
        let mut overlaps =
            self.iter_overlapping_with_rectangle(source, source_transform, source_collider)?;
        Ok(overlaps.next().is_some())
    }

    /// Iterates over circular candidates overlapping a rectangular source.
    ///
    /// Tangent face and corner contact count as overlap. A foreign or retired
    /// handle is rejected before lookup; absent, unmanaged, disabled, or
    /// private-marker-mismatched sources produce
    /// [`QueryEntityError::DoesNotMatch`]. The supplied geometry is used by
    /// value and its association with `source` is caller-enforced. `F` applies
    /// only to circular candidates; the source and disabled candidates are
    /// excluded. This is an allocation-free linear scan with no stable order.
    pub fn iter_overlapping_with_rectangle(
        &self,
        source: LogicEntity,
        source_transform: &Transform2d,
        source_collider: &RectangleCollider2d,
    ) -> Result<impl Iterator<Item = LogicEntity> + '_, QueryEntityError> {
        if !self.identity.owns(source) {
            return Err(QueryEntityError::ForeignWorld {
                entity: source,
                active: self.identity.generation(),
            });
        }

        let managed = self
            .sources
            .get(source.entity())
            .map_err(|_| QueryEntityError::DoesNotMatch { entity: source })?;
        if managed.handle(source.entity()) != source {
            return Err(QueryEntityError::DoesNotMatch { entity: source });
        }
        let source_position = source_transform.translation();
        let source_size = source_collider.size;

        Ok(self
            .candidates
            .iter()
            .filter_map(move |(raw, candidate, transform, collider)| {
                let candidate = candidate.handle(raw);
                (candidate != source
                    && circle_rectangle_overlap(
                        transform.translation(),
                        collider.radius,
                        source_position,
                        source_size,
                    ))
                .then_some(candidate)
            }))
    }
}

fn validate_radius(value: f32) -> Result<(), CircleColliderError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or(CircleColliderError::InvalidRadius { value })
}

fn validate_rectangle_size(value: Vec2) -> Result<(), RectangleColliderError> {
    (value.is_finite() && value.x() > 0.0 && value.y() > 0.0)
        .then_some(())
        .ok_or(RectangleColliderError::InvalidSize { value })
}

fn circles_overlap(first: Vec2, first_radius: f32, second: Vec2, second_radius: f32) -> bool {
    let radius = f64::from(first_radius) + f64::from(second_radius);
    let radius_squared = radius * radius;
    let x = f64::from(first.x()) - f64::from(second.x());
    let y = f64::from(first.y()) - f64::from(second.y());
    let y_squared = y * y;
    let approximate_distance_squared = x * x + y_squared;

    // For validated `f32` geometry, `x*x`, `y*y`, their sum, and the squared
    // radius are non-negative finite `f64`s; every nonzero squared term is
    // normal at this widened precision. Rounding `x*x` before a non-cancelling
    // addition can therefore move the result by at most one adjacent `f64`
    // versus the fused form. If the comparisons could disagree, the squared
    // radius must lie between those adjacent results. This conservative
    // two-step band catches that region and preserves the original boundary.
    if approximate_distance_squared
        .to_bits()
        .abs_diff(radius_squared.to_bits())
        <= 2
    {
        return x.mul_add(x, y_squared) <= radius_squared;
    }

    approximate_distance_squared <= radius_squared
}

fn rectangles_overlap(first: Vec2, first_size: Vec2, second: Vec2, second_size: Vec2) -> bool {
    let x_distance = (f64::from(first.x()) - f64::from(second.x())).abs();
    let x_reach = (f64::from(first_size.x()) + f64::from(second_size.x())) * 0.5;
    if x_distance > x_reach {
        return false;
    }

    let y_distance = (f64::from(first.y()) - f64::from(second.y())).abs();
    let y_reach = (f64::from(first_size.y()) + f64::from(second_size.y())) * 0.5;
    y_distance <= y_reach
}

fn circle_rectangle_overlap(
    circle: Vec2,
    circle_radius: f32,
    rectangle: Vec2,
    rectangle_size: Vec2,
) -> bool {
    let x_distance = (f64::from(circle.x()) - f64::from(rectangle.x())).abs();
    let y_distance = (f64::from(circle.y()) - f64::from(rectangle.y())).abs();
    let half_width = f64::from(rectangle_size.x()) * 0.5;
    let half_height = f64::from(rectangle_size.y()) * 0.5;
    let outside_x = (x_distance - half_width).max(0.0);
    let outside_y = (y_distance - half_height).max(0.0);
    let radius = f64::from(circle_radius);
    outside_x.mul_add(outside_x, outside_y * outside_y) <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        identity::{ApplicationId, WorldGeneration},
        system::{Stage, StageFactories},
    };
    use bevy_ecs::{
        prelude::{Res, ResMut, Resource},
        world::World,
    };
    use sim_engine::Color;

    #[derive(Resource, Clone, Copy)]
    struct CorruptedSource(LogicEntity);

    #[derive(Resource, Clone, Copy)]
    struct CorruptedSourceGeometry {
        transform: Transform2d,
        circle: CircleCollider2d,
        rectangle: RectangleCollider2d,
    }

    #[derive(Resource, Default)]
    struct CorruptedSourceObservation {
        circle_iterator: Option<QueryEntityError>,
        circle_boolean: Option<QueryEntityError>,
        circle_with_rectangle_iterator: Option<QueryEntityError>,
        circle_with_rectangle_boolean: Option<QueryEntityError>,
        rectangle_iterator: Option<QueryEntityError>,
        rectangle_boolean: Option<QueryEntityError>,
        rectangle_with_circle_iterator: Option<QueryEntityError>,
        rectangle_with_circle_boolean: Option<QueryEntityError>,
    }

    fn observe_corrupted_overlap_source(
        source: Res<CorruptedSource>,
        geometry: Res<CorruptedSourceGeometry>,
        circles: CircleOverlapEntities,
        rectangles: RectangleOverlapEntities,
        mut observation: ResMut<CorruptedSourceObservation>,
    ) {
        observation.circle_iterator = circles
            .iter_overlapping(source.0, &geometry.transform, &geometry.circle)
            .err();
        observation.circle_boolean = circles
            .has_overlap(source.0, &geometry.transform, &geometry.circle)
            .err();
        observation.circle_with_rectangle_iterator = circles
            .iter_overlapping_with_rectangle(source.0, &geometry.transform, &geometry.rectangle)
            .err();
        observation.circle_with_rectangle_boolean = circles
            .has_overlap_with_rectangle(source.0, &geometry.transform, &geometry.rectangle)
            .err();
        observation.rectangle_iterator = rectangles
            .iter_overlapping(source.0, &geometry.transform, &geometry.rectangle)
            .err();
        observation.rectangle_boolean = rectangles
            .has_overlap(source.0, &geometry.transform, &geometry.rectangle)
            .err();
        observation.rectangle_with_circle_iterator = rectangles
            .iter_overlapping_with_circle(source.0, &geometry.transform, &geometry.circle)
            .err();
        observation.rectangle_with_circle_boolean = rectangles
            .has_overlap_with_circle(source.0, &geometry.transform, &geometry.circle)
            .err();
    }

    fn transform(x: f32, y: f32) -> Transform2d {
        Transform2d::new(Vec2::new(x, y)).expect("test translation should be finite")
    }

    #[test]
    fn overlap_source_lookups_reject_mismatched_private_provenance() {
        let application = ApplicationId::from_raw(1);
        let active = WorldGeneration::new(application, 1);
        let corrupted = WorldGeneration::new(application, 99);
        let circle = CircleCollider2d::new(1.0).expect("test radius should be valid");
        let rectangle =
            RectangleCollider2d::new(Vec2::splat(2.0)).expect("test rectangle should be valid");
        let mut world = World::new();
        world.insert_resource(WorldIdentityState::new(application, active));
        world.insert_resource(CorruptedSourceGeometry {
            transform: Transform2d::default(),
            circle,
            rectangle,
        });
        world.insert_resource(CorruptedSourceObservation::default());
        let raw = world
            .spawn((
                Transform2d::default(),
                circle,
                rectangle,
                ManagedEntity::for_generation(corrupted),
            ))
            .id();
        let source = LogicEntity::new(application, active, raw);
        world.insert_resource(CorruptedSource(source));
        world.spawn((
            Transform2d::default(),
            circle,
            rectangle,
            ManagedEntity::for_generation(active),
        ));
        let mut factories = StageFactories::default();
        factories.add(observe_corrupted_overlap_source);
        let mut stage = factories
            .instantiate(&mut world, Stage::FixedUpdate)
            .expect("overlap corruption System should initialize");

        stage
            .run(&mut world)
            .expect("overlap corruption System should run");

        let expected = Some(QueryEntityError::DoesNotMatch { entity: source });
        let observation = world.resource::<CorruptedSourceObservation>();
        assert_eq!(observation.circle_iterator, expected);
        assert_eq!(observation.circle_boolean, expected);
        assert_eq!(observation.circle_with_rectangle_iterator, expected);
        assert_eq!(observation.circle_with_rectangle_boolean, expected);
        assert_eq!(observation.rectangle_iterator, expected);
        assert_eq!(observation.rectangle_boolean, expected);
        assert_eq!(observation.rectangle_with_circle_iterator, expected);
        assert_eq!(observation.rectangle_with_circle_boolean, expected);
    }

    #[test]
    fn radius_validation_and_mutation_are_atomic() {
        for invalid in [0.0, -0.0, -1.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(matches!(
                CircleCollider2d::new(invalid),
                Err(CircleColliderError::InvalidRadius { value }) if value.to_bits() == invalid.to_bits()
            ));
        }

        let mut collider = CircleCollider2d::new(2.0).expect("initial radius should be valid");
        assert!(collider.set_radius(f32::NAN).is_err());
        assert_eq!(collider.radius(), 2.0);
        assert!(collider.set_radius(3.0).is_ok());
        assert_eq!(collider.radius(), 3.0);
    }

    #[test]
    fn circle_collider_copies_visual_radius_once_and_preserves_bits() {
        let color = Color::rgb8(68, 144, 255);
        for radius in [f32::from_bits(1), 1.25, f32::MAX] {
            let visual = CircleVisual::new(radius, color).expect("visual radius should be valid");
            let collider = CircleCollider2d::from_visual(&visual);
            let (paired_visual, paired_collider) = visual.with_matching_collider();
            let validated = CircleCollider2d::new(visual.radius())
                .expect("visual geometry must satisfy collider invariants");

            assert_eq!(collider, validated);
            assert_eq!(paired_visual, visual);
            assert_eq!(paired_collider, collider);
            assert_eq!(collider.radius().to_bits(), radius.to_bits());
        }

        let initial = CircleVisual::new(2.0, color).expect("visual should be valid");
        let (mut visual, mut collider) = initial.with_matching_collider();
        visual.set_radius(3.0).expect("new visual radius is valid");
        assert_eq!(collider.radius(), 2.0);
        collider
            .set_radius(4.0)
            .expect("new collider radius is valid");
        assert_eq!(visual.radius(), 3.0);
        assert_eq!(collider.radius(), 4.0);
    }

    #[test]
    fn overlap_includes_tangent_contained_and_same_center_circles() {
        let first = CircleCollider2d::new(2.0).expect("radius should be valid");
        let second = CircleCollider2d::new(1.0).expect("radius should be valid");
        let origin = transform(0.0, 0.0);

        assert!(first.overlaps(&origin, &second, &transform(3.0, 0.0)));
        assert!(!first.overlaps(&origin, &second, &transform(3.001, 0.0)));
        assert!(first.overlaps(&origin, &second, &transform(0.5, 0.0)));
        assert!(first.overlaps(&origin, &second, &origin));
    }

    #[test]
    fn circle_overlap_preserves_closed_radial_bounds() {
        let first = CircleCollider2d::new(2.0).expect("radius should be valid");
        let second = CircleCollider2d::new(3.0).expect("radius should be valid");
        let origin = transform(0.0, 0.0);
        let next_after_four = f32::from_bits(4.0_f32.to_bits() + 1);
        let next_after_five = f32::from_bits(5.0_f32.to_bits() + 1);

        let diagonal_tangent = transform(3.0, 4.0);
        assert!(first.overlaps(&origin, &second, &diagonal_tangent));
        assert!(second.overlaps(&diagonal_tangent, &first, &origin));

        let diagonal_miss_inside_axis_bounds = transform(3.0, next_after_four);
        assert!(!first.overlaps(&origin, &second, &diagonal_miss_inside_axis_bounds));
        assert!(!second.overlaps(&diagonal_miss_inside_axis_bounds, &first, &origin));

        for axis_tangent in [transform(5.0, 0.0), transform(0.0, 5.0)] {
            assert!(first.overlaps(&origin, &second, &axis_tangent));
            assert!(second.overlaps(&axis_tangent, &first, &origin));
        }
        for axis_miss in [
            transform(next_after_five, 0.0),
            transform(0.0, next_after_five),
        ] {
            assert!(!first.overlaps(&origin, &second, &axis_miss));
            assert!(!second.overlaps(&axis_miss, &first, &origin));
        }
    }

    #[test]
    fn circle_overlap_widens_subnormal_values_before_arithmetic() {
        let minimum = f32::from_bits(1);
        let combined = f32::from_bits(2);
        let outside = f32::from_bits(3);
        let collider =
            CircleCollider2d::new(minimum).expect("positive subnormal radius should be valid");
        let origin = transform(-0.0, 0.0);

        assert!(collider.overlaps(&origin, &collider, &transform(combined, -0.0)));
        assert!(!collider.overlaps(&origin, &collider, &transform(outside, 0.0)));
        assert!(!collider.overlaps(&origin, &collider, &transform(combined, combined)));
        assert!(collider.overlaps(&origin, &collider, &transform(0.0, -0.0)));
    }

    #[test]
    fn circle_overlap_keeps_fused_precision_at_the_closed_boundary() {
        let delta = 2.0_f32.powi(-26);
        let first = CircleCollider2d::new(5.0).expect("radius should be valid");
        let second = CircleCollider2d::new(delta).expect("radius should be valid");
        let first_transform = transform(3.0, 4.0);
        let second_transform = transform(-3.0 * delta, delta);

        // Exact squared distance exceeds squared reach by `9 * delta^2`.
        // Separately rounding `x*x` before adding `y*y` loses that difference
        // and produces a false positive at this boundary.
        assert!(!first.overlaps(&first_transform, &second, &second_transform));
        assert!(!second.overlaps(&second_transform, &first, &first_transform));
    }

    #[test]
    fn circle_overlap_fast_path_matches_the_fused_reference() {
        fn reference(first: Vec2, first_radius: f32, second: Vec2, second_radius: f32) -> bool {
            let x = f64::from(first.x()) - f64::from(second.x());
            let y = f64::from(first.y()) - f64::from(second.y());
            let radius = f64::from(first_radius) + f64::from(second_radius);
            x.mul_add(x, y * y) <= radius * radius
        }

        let mut state = 0x8d26_4e05_u32;
        let mut checked = 0_usize;
        while checked < 100_000 {
            let mut next = || {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from_bits(state)
            };
            let first = Vec2::new(next(), next());
            let second = Vec2::new(next(), next());
            let first_radius = f32::from_bits(next().to_bits() & 0x7fff_ffff);
            let second_radius = f32::from_bits(next().to_bits() & 0x7fff_ffff);
            if !first.is_finite()
                || !second.is_finite()
                || !first_radius.is_finite()
                || !second_radius.is_finite()
                || first_radius <= 0.0
                || second_radius <= 0.0
            {
                continue;
            }

            assert_eq!(
                circles_overlap(first, first_radius, second, second_radius),
                reference(first, first_radius, second, second_radius),
                "mismatch for {first:?}/{first_radius:?} and {second:?}/{second_radius:?}"
            );
            checked += 1;
        }
    }

    #[test]
    fn extreme_finite_coordinates_do_not_create_an_infinity_false_positive() {
        let collider = CircleCollider2d::new(f32::MAX).expect("finite radius should be valid");
        let negative = transform(-f32::MAX, -f32::MAX);
        let positive = transform(f32::MAX, f32::MAX);
        let axis_tangent = transform(f32::MAX, -f32::MAX);

        assert!(!collider.overlaps(&negative, &collider, &positive));
        assert!(collider.overlaps(&negative, &collider, &axis_tangent));
    }

    #[test]
    fn mixed_overlap_has_symmetric_closed_face_and_corner_bounds() {
        let circle = CircleCollider2d::new(5.0).expect("circle should be valid");
        let rectangle =
            RectangleCollider2d::new(Vec2::splat(2.0)).expect("rectangle should be valid");
        let rectangle_transform = transform(0.0, 0.0);
        let before_four = f32::from_bits(4.0_f32.to_bits() - 1);
        let after_four = f32::from_bits(4.0_f32.to_bits() + 1);

        for (circle_transform, expected) in [
            (transform(0.0, 0.0), true),
            (transform(-0.0, 0.0), true),
            (transform(6.0, 0.0), true),
            (transform(5.0, 4.0), true),
            (transform(5.0, before_four), true),
            (transform(5.0, after_four), false),
            (transform(f32::from_bits(6.0_f32.to_bits() + 1), 0.0), false),
            (transform(0.0, f32::from_bits(6.0_f32.to_bits() + 1)), false),
        ] {
            assert_eq!(
                circle.overlaps_rectangle(&circle_transform, &rectangle, &rectangle_transform),
                expected
            );
            assert_eq!(
                rectangle.overlaps_circle(&rectangle_transform, &circle, &circle_transform),
                expected
            );
        }

        let containing_circle =
            CircleCollider2d::new(20.0).expect("containing circle should be valid");
        assert!(containing_circle.overlaps_rectangle(
            &rectangle_transform,
            &rectangle,
            &rectangle_transform
        ));
    }

    #[test]
    fn mixed_overlap_widens_subnormal_and_extreme_geometry_before_arithmetic() {
        let minimum = f32::from_bits(1);
        let subnormal_circle =
            CircleCollider2d::new(minimum).expect("subnormal radius should be valid");
        let subnormal_rectangle =
            RectangleCollider2d::new(Vec2::splat(minimum)).expect("subnormal size should be valid");
        let origin = transform(-0.0, 0.0);
        let touching = transform(minimum, -0.0);
        let missing = transform(f32::from_bits(2), 0.0);
        assert!(subnormal_circle.overlaps_rectangle(&touching, &subnormal_rectangle, &origin));
        assert!(!subnormal_circle.overlaps_rectangle(&missing, &subnormal_rectangle, &origin));
        assert_eq!(
            subnormal_circle.overlaps_rectangle(&touching, &subnormal_rectangle, &origin),
            subnormal_rectangle.overlaps_circle(&origin, &subnormal_circle, &touching)
        );

        let maximum_circle =
            CircleCollider2d::new(f32::MAX).expect("maximum radius should be valid");
        let maximum_rectangle =
            RectangleCollider2d::new(Vec2::splat(f32::MAX)).expect("maximum size should be valid");
        let negative = transform(-f32::MAX, -f32::MAX);
        let positive = transform(f32::MAX, f32::MAX);
        assert!(maximum_circle.overlaps_rectangle(&positive, &maximum_rectangle, &positive));
        assert!(!maximum_circle.overlaps_rectangle(&negative, &maximum_rectangle, &positive));
        assert_eq!(
            maximum_circle.overlaps_rectangle(&negative, &maximum_rectangle, &positive),
            maximum_rectangle.overlaps_circle(&positive, &maximum_circle, &negative)
        );
    }

    #[test]
    fn rectangle_size_validation_and_mutation_are_atomic() {
        let invalid_sizes = [
            Vec2::new(0.0, 1.0),
            Vec2::new(-0.0, 1.0),
            Vec2::new(-1.0, 1.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, -0.0),
            Vec2::new(1.0, -1.0),
            Vec2::new(f32::NAN, 1.0),
            Vec2::new(1.0, f32::NAN),
            Vec2::new(f32::INFINITY, 1.0),
            Vec2::new(1.0, f32::NEG_INFINITY),
        ];
        for invalid in invalid_sizes {
            assert!(matches!(
                RectangleCollider2d::new(invalid),
                Err(RectangleColliderError::InvalidSize { value }) if value.x().to_bits() == invalid.x().to_bits()
                    && value.y().to_bits() == invalid.y().to_bits()
            ));
        }

        let initial = Vec2::new(2.0, 4.0);
        let mut collider = RectangleCollider2d::new(initial).expect("initial size should be valid");
        assert!(collider.set_size(Vec2::new(3.0, f32::NAN)).is_err());
        assert_eq!(collider.size(), initial);
        assert!(collider.set_size(Vec2::new(3.0, 5.0)).is_ok());
        assert_eq!(collider.size(), Vec2::new(3.0, 5.0));
    }

    #[test]
    fn rectangle_collider_copies_only_visual_size_once_and_preserves_bits() {
        let color = Color::rgb8(54, 68, 92);
        for size in [
            Vec2::new(f32::from_bits(1), f32::MAX),
            Vec2::new(2.5, 7.25),
            Vec2::new(f32::MAX, f32::from_bits(1)),
        ] {
            let visual = RectangleVisual::new(size, color).expect("visual size should be valid");
            let collider = RectangleCollider2d::from_visual(&visual);
            let (paired_visual, paired_collider) = visual.with_matching_collider();
            let validated = RectangleCollider2d::new(visual.size())
                .expect("visual geometry must satisfy collider invariants");

            assert_eq!(collider, validated);
            assert_eq!(paired_visual, visual);
            assert_eq!(paired_collider, collider);
            assert_eq!(collider.size().x().to_bits(), size.x().to_bits());
            assert_eq!(collider.size().y().to_bits(), size.y().to_bits());
        }

        let size = Vec2::new(4.0, 2.0);
        let square = RectangleVisual::new(size, color).expect("visual should be valid");
        let rounded =
            RectangleVisual::rounded(size, color, 20.0).expect("rounded visual should be valid");
        assert_eq!(rounded.corner_radius(), 20.0);
        assert_eq!(
            RectangleCollider2d::from_visual(&rounded),
            RectangleCollider2d::from_visual(&square)
        );
        assert_eq!(
            rounded.with_matching_collider().1,
            square.with_matching_collider().1
        );

        let (mut visual, mut collider) = rounded.with_matching_collider();
        visual
            .set_size(Vec2::new(8.0, 6.0))
            .expect("new visual size is valid");
        assert_eq!(collider.size(), size);
        collider
            .set_size(Vec2::new(3.0, 5.0))
            .expect("new collider size is valid");
        assert_eq!(visual.size(), Vec2::new(8.0, 6.0));
        assert_eq!(collider.size(), Vec2::new(3.0, 5.0));
    }

    #[test]
    fn rectangle_overlap_has_closed_symmetric_axis_aligned_bounds() {
        let first =
            RectangleCollider2d::new(Vec2::new(2.0, 4.0)).expect("first size should be valid");
        let second =
            RectangleCollider2d::new(Vec2::new(4.0, 2.0)).expect("second size should be valid");
        let origin = transform(0.0, 0.0);

        for touching in [
            transform(3.0, 0.0),
            transform(0.0, 3.0),
            transform(3.0, 3.0),
        ] {
            assert!(first.overlaps(&origin, &second, &touching));
            assert!(second.overlaps(&touching, &first, &origin));
        }
        assert!(first.overlaps(&origin, &second, &origin));
        assert!(first.overlaps(&origin, &second, &transform(0.5, 0.5)));
        assert!(!first.overlaps(&origin, &second, &transform(3.001, 0.0)));
        assert!(!first.overlaps(&origin, &second, &transform(0.0, 3.001)));
    }

    #[test]
    fn rectangle_overlap_widens_extreme_and_subnormal_values_before_arithmetic() {
        let maximum = RectangleCollider2d::new(Vec2::splat(f32::MAX))
            .expect("maximum finite size should be valid");
        let negative = transform(-f32::MAX, -f32::MAX);
        let positive = transform(f32::MAX, f32::MAX);
        assert!(!maximum.overlaps(&negative, &maximum, &positive));
        assert!(maximum.overlaps(&negative, &maximum, &negative));

        let subnormal_value = f32::from_bits(1);
        let subnormal = RectangleCollider2d::new(Vec2::splat(subnormal_value))
            .expect("positive subnormal size should be valid");
        assert!(subnormal.overlaps(
            &transform(0.0, 0.0),
            &subnormal,
            &transform(subnormal_value, subnormal_value)
        ));

        let wider_subnormal = RectangleCollider2d::new(Vec2::splat(f32::from_bits(2)))
            .expect("wider positive subnormal size should be valid");
        assert!(!subnormal.overlaps(
            &transform(0.0, 0.0),
            &wider_subnormal,
            &transform(f32::from_bits(2), f32::from_bits(2))
        ));
    }
}

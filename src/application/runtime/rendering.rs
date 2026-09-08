//! Cached extraction queries and fixed-step visual interpolation history.
//!
//! Query caches belong to each RuntimeWorld and survive its stage barriers.
//! Captures retain previous endpoints so failed fixed work can restore visual
//! history without rolling back canonical mutations.

use bevy_ecs::{
    entity::Entity,
    entity_disabling::Disabled,
    query::{Allow, QueryState, Without},
    world::World,
};

use crate::{
    ExtractionError,
    assets::ImageAssetRegistry,
    extraction::{
        CircleSource, ExtractionBuffers, ExtractionParameters, LineSource, RectangleSource,
        ScreenImageSource, ScreenRectangleSource, ScreenSource,
    },
    identity::ManagedEntity,
    render::RenderLimits,
    screen::{ScreenImageVisual, ScreenRectangleVisual},
    visual::{
        ActiveCamera2d, CircleVisual, LineVisual, RectangleVisual, Transform2d, WorldBackground,
    },
};

use super::RuntimeWorld;

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
type ScreenImageExtractionQuery = QueryState<
    (Entity, &'static ManagedEntity, &'static ScreenImageVisual),
    (Allow<Disabled>, Without<Disabled>),
>;
type TransformInterpolationQuery =
    QueryState<(Entity, &'static ManagedEntity, &'static mut Transform2d), Allow<Disabled>>;
type CameraSnapQuery = QueryState<&'static mut ActiveCamera2d, Allow<Disabled>>;

pub(super) struct InterpolationQueries {
    cameras: CameraInterpolationQuery,
    transforms: TransformInterpolationQuery,
    snap_cameras: CameraSnapQuery,
}

impl InterpolationQueries {
    pub(super) fn new(world: &mut World) -> Self {
        Self {
            cameras: world.query_filtered(),
            transforms: world.query_filtered(),
            snap_cameras: world.query_filtered(),
        }
    }
}

pub(super) struct ExtractionQueries {
    cameras: CameraExtractionQuery,
    circles: CircleExtractionQuery,
    rectangles: RectangleExtractionQuery,
    lines: LineExtractionQuery,
    screen_rectangles: ScreenRectangleExtractionQuery,
    screen_images: ScreenImageExtractionQuery,
}

impl ExtractionQueries {
    pub(super) fn new(world: &mut World) -> Self {
        Self {
            cameras: world.query_filtered(),
            circles: world.query_filtered(),
            rectangles: world.query_filtered(),
            lines: world.query_filtered(),
            screen_rectangles: world.query_filtered(),
            screen_images: world.query_filtered(),
        }
    }
}

pub(super) fn snap_interpolation(world: &mut World) {
    let mut transforms = world.query_filtered::<&mut Transform2d, Allow<Disabled>>();
    for mut transform in transforms.iter_mut(world) {
        transform.snap_interpolation();
    }
    let mut cameras = world.query_filtered::<&mut ActiveCamera2d, Allow<Disabled>>();
    for mut camera in cameras.iter_mut(world) {
        camera.snap_interpolation();
    }
}

pub(super) fn snap_runtime_interpolation(runtime: &mut RuntimeWorld) {
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

pub(super) fn capture_and_begin_fixed_interpolation(
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

pub(super) fn restore_previous_translations(
    world: &mut World,
    before: &[(crate::identity::LogicEntity, sim_engine::Vec2)],
) {
    for (entity, previous) in before {
        if let Some(mut transform) = world.get_mut::<Transform2d>(entity.entity()) {
            transform.restore_previous_translation(*previous);
        }
    }
}

pub(super) fn restore_previous_camera_centers(
    world: &mut World,
    before: &[(crate::identity::LogicEntity, sim_engine::Vec2)],
) {
    for (entity, previous) in before {
        if let Some(mut camera) = world.get_mut::<ActiveCamera2d>(entity.entity()) {
            camera.restore_previous_center(*previous);
        }
    }
}

pub(super) fn stage_runtime_world(
    runtime: &mut RuntimeWorld,
    alpha: f32,
    limits: RenderLimits,
    extraction: &mut ExtractionBuffers,
    images: &ImageAssetRegistry,
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
        .map(|(raw, entity, visual)| {
            ScreenSource::Rectangle(ScreenRectangleSource::new(entity.handle(raw), visual))
        });
    let screen_images = runtime
        .extraction_queries
        .screen_images
        .iter(&runtime.world)
        .map(|(raw, entity, visual)| {
            ScreenSource::Image(ScreenImageSource::new(entity.handle(raw), visual, images))
        });
    extraction.stage(
        ExtractionParameters::new(runtime.generation, background, alpha, limits),
        cameras,
        circles,
        rectangles,
        lines,
        screen_rectangles.chain(screen_images),
    )
}

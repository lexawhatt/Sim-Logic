use bevy_ecs::entity::Entity;
use sim_engine::{Color, LogicalViewport, Rotation3d, Transform3d, Vec3, WorldLength};

use crate::identity::{ApplicationId, LogicEntity, WorldGeneration};

use super::*;

fn point(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z).unwrap()
}

fn cuboid() -> CuboidVisual3d {
    CuboidVisual3d::new(Vec3::ZERO, point(2.0, 1.0, 3.0), Color::WHITE).unwrap()
}

fn view() -> View3d {
    View3d::new(point(4.0, 6.0, 12.0), Vec3::ZERO).unwrap()
}

fn identity(sequence: u64, row: u32) -> LogicEntity {
    let application = ApplicationId::from_raw(1);
    LogicEntity::new(
        application,
        WorldGeneration::new(application, sequence),
        Entity::from_raw_u32(row).unwrap(),
    )
}

#[test]
fn invalid_geometry_and_transparency_leave_values_unchanged() {
    let mut visual = cuboid();
    let original = visual;
    assert!(visual.set_color(Color::rgba(1.0, 0.0, 0.0, 0.5)).is_err());
    assert_eq!(visual, original);
    let collapsed = Transform3d::new(
        point(f32::MAX, 0.0, 0.0),
        Rotation3d::IDENTITY,
        point(1.0, 1.0, 1.0),
    )
    .unwrap();
    assert_eq!(
        visual.set_transform(collapsed),
        Err(CuboidVisualError::CollapsedGeometry)
    );
    assert_eq!(visual, original);
    assert!(CuboidVisual3d::new(Vec3::ZERO, point(-1.0, 1.0, 1.0), Color::WHITE).is_err());
}

#[test]
fn corners_follow_rotation_and_positive_extents() {
    let mut visual = cuboid();
    let rotation = Rotation3d::from_euler_xyz(0.0, 0.7, 0.2).unwrap();
    visual
        .set_transform(
            Transform3d::new(point(1.0, 2.0, 3.0), rotation, point(2.0, 3.0, 4.0)).unwrap(),
        )
        .unwrap();
    let corners = visual.corners().unwrap();
    for (index, corner) in corners.iter().enumerate() {
        assert!(!corners[..index].contains(corner));
    }
    let center_x = corners.iter().map(|corner| corner.x()).sum::<f32>() / 8.0;
    assert!((center_x - 1.0).abs() < 1e-5);
}

#[test]
fn view_setters_are_atomic_and_aspect_follows_viewport() {
    let mut camera_view = view();
    let original = camera_view;
    assert!(camera_view.set_pose(Vec3::ZERO, Vec3::ZERO).is_err());
    assert!(camera_view.set_background(Color::TRANSPARENT).is_err());
    assert!(
        camera_view
            .set_perspective(
                f32::NAN,
                WorldLength::new(0.1).unwrap(),
                WorldLength::new(100.0).unwrap()
            )
            .is_err()
    );
    assert_eq!(camera_view, original);
    for (width, height) in [(1280.0, 720.0), (64.0, 2048.0), (4096.0, 64.0)] {
        let viewport = LogicalViewport::new(width, height).unwrap();
        let camera = camera_view.camera(viewport).unwrap();
        assert_eq!(camera.projection().aspect_ratio(), width / height);
        assert!(
            camera
                .project_world(Vec3::ZERO, viewport)
                .unwrap()
                .inside_view()
        );
    }
}

#[test]
fn suppressed_views_do_not_visit_cuboid_sources() {
    let mut buffer = ThreeDExtractionBuffer::new();
    for descriptor in [
        None,
        Some({
            let mut disabled = view();
            disabled.set_enabled(false);
            disabled
        }),
    ] {
        let sources = std::iter::from_fn(|| -> Option<CuboidSource> {
            panic!("disabled view must not visit sources")
        });
        buffer
            .extract(
                identity(1, 0).world_generation(),
                ThreeDRenderLimits::default(),
                descriptor,
                sources,
            )
            .unwrap();
        assert!(buffer.snapshot().is_none());
        assert!(buffer.resolved().is_empty());
    }
}

#[test]
fn extraction_checks_provenance_and_exact_object_triangle_limits() {
    let mut buffer = ThreeDExtractionBuffer::new();
    let generation = identity(1, 0).world_generation();
    let limits = ThreeDRenderLimits::new(1, 12, 1);
    let visible = cuboid();
    assert!(matches!(
        buffer.extract(
            generation,
            limits,
            Some(view()),
            [CuboidSource::new(identity(2, 0), &visible)]
        ),
        Err(ThreeDExtractionError::StaleEntity { .. })
    ));
    assert!(matches!(
        buffer.extract(
            generation,
            ThreeDRenderLimits::new(1, 11, 1),
            Some(view()),
            [CuboidSource::new(identity(1, 0), &visible)]
        ),
        Err(ThreeDExtractionError::LimitExceeded {
            resource: ThreeDLimitResource::Triangles,
            ..
        })
    ));
    assert!(matches!(
        buffer.extract(
            generation,
            limits,
            Some(view()),
            [
                CuboidSource::new(identity(1, 0), &visible),
                CuboidSource::new(identity(1, 1), &visible)
            ]
        ),
        Err(ThreeDExtractionError::LimitExceeded {
            resource: ThreeDLimitResource::Cuboids,
            ..
        })
    ));
    assert!(buffer.snapshot().is_none());
}

#[test]
fn extraction_reuses_bounded_records_and_sorts_sources() {
    let mut buffer = ThreeDExtractionBuffer::new();
    let generation = identity(1, 0).world_generation();
    let visual = cuboid();
    let mut hidden = visual;
    hidden.set_visible(false);
    let limits = ThreeDRenderLimits::new(2, 24, 1);
    let sources = [
        CuboidSource::new(identity(1, 9), &visual),
        CuboidSource::new(identity(1, 2), &visual),
        CuboidSource::new(identity(2, 0), &hidden),
    ];
    buffer
        .extract(generation, limits, Some(view()), sources)
        .unwrap();
    assert!(
        buffer.resolved()[0].source().stable_bits() < buffer.resolved()[1].source().stable_bits()
    );
    let pointer = buffer.resolved().as_ptr();
    for _ in 0..8 {
        buffer
            .extract(generation, limits, Some(view()), sources)
            .unwrap();
        assert_eq!(buffer.resolved().as_ptr(), pointer);
    }
    buffer.clear();
    assert!(buffer.resolved().is_empty());
    assert!(buffer.snapshot().is_none());
}

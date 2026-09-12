//! Engine 0.3 boundary checks independent of a window or GPU device.

use super::*;
use crate::identity::{ApplicationId, WorldGeneration};
use bevy_ecs::entity::Entity;
use sim_engine::{Color, SurfaceStyle3d};

fn source(generation: u64, row: u32) -> LogicEntity {
    let application = ApplicationId::from_raw(7);
    LogicEntity::new(
        application,
        WorldGeneration::new(application, generation),
        Entity::from_raw_u32(row).unwrap(),
    )
}

fn state(source: LogicEntity) -> AppliedState {
    AppliedState {
        source,
        transform: Transform3d::IDENTITY,
        style: MeshStyle3d::surface(SurfaceStyle3d::opaque(Color::WHITE).unwrap()),
        visible: true,
    }
}

#[test]
fn same_geometry_refreshes_source_after_reorder_and_world_replacement() {
    let mut applied = state(source(1, 2));
    for current in [source(1, 3), source(2, 3), source(3, 0)] {
        applied
            .update(state(current), |_| -> Result<(), ()> {
                panic!("identity-only changes must not call Engine setters")
            })
            .unwrap();
        assert_eq!(applied.source, current);
        assert!(applied.visible);
    }
}

#[test]
fn source_lookup_uses_complete_identity_and_ignores_hidden_slots() {
    // Engine object handles cannot be manually constructed. Exercise the same
    // equality-based lookup with provenance-bearing test keys, not fake Engine
    // IDs or unsafe layout assumptions.
    let slots = [
        ((1, 7), source(1, 0), true),
        ((2, 7), source(2, 0), true),
        ((2, 8), source(2, 1), false),
    ];
    assert_eq!(find_object_source((1, 7), slots), Some(source(1, 0)));
    assert_eq!(find_object_source((2, 7), slots), Some(source(2, 0)));
    assert_eq!(find_object_source((2, 8), slots), None);
    assert_eq!(find_object_source((3, 7), slots), None);
}

#[test]
fn scene_wide_failures_preserve_the_engine_category_without_attribution() {
    for error in [
        Mesh3dRenderError::RendererMismatch,
        Mesh3dRenderError::InvalidGeometryTransform,
        Mesh3dRenderError::UnportableSurfaceTopology,
        Mesh3dRenderError::InstanceCapacityTooLarge,
        Mesh3dRenderError::CameraTargetAspectMismatch,
        Mesh3dRenderError::InvalidEdgeProjection,
        Mesh3dRenderError::GeneratedGeometryCapacityTooLarge,
    ] {
        assert!(matches!(
            map_render_error(error, &[]),
            DesktopThreeDError::Render(preserved) if preserved == error
        ));
    }
}

#[test]
fn engine_scene_budget_uses_logic_objects_and_finite_untextured_memory_caps() {
    let defaults = Scene3dBudget::default();
    let budget = engine_scene_budget(ThreeDRenderLimits::new(123, 1476, 640 * 360)).unwrap();
    assert_eq!(budget.max_objects(), 123);
    assert_eq!(budget.max_storage_bytes(), defaults.max_storage_bytes());
    assert_eq!(budget.max_mesh_cpu_bytes(), defaults.max_mesh_cpu_bytes());
    assert_eq!(budget.max_mesh_gpu_bytes(), defaults.max_mesh_gpu_bytes());
    assert_eq!(budget.max_texture_cpu_bytes(), 0);
    assert_eq!(budget.max_texture_gpu_bytes(), 0);
    let scene = Scene3d::with_budget(Color::BLACK, budget).unwrap();
    assert_eq!(scene.budget(), budget);
    assert_eq!(scene.statistics().object_count(), 0);
}

#[test]
fn background_only_engine_minimum_does_not_enable_logic_objects() {
    let limits = ThreeDRenderLimits::new(0, 0, 640 * 360);
    assert_eq!(engine_scene_budget(limits).unwrap().max_objects(), 1);
    assert!(check_limits(0, 640, 360, limits).is_ok());
    assert!(matches!(
        check_limits(1, 640, 360, limits),
        Err(DesktopThreeDError::LimitExceeded {
            resource: ThreeDLimitResource::Cuboids,
            requested: 1,
            limit: 0,
        })
    ));
    let budget = engine_render_budget(0, limits);
    assert_eq!(budget.max_generated_vertices(), 0);
    assert_eq!(budget.max_generated_triangles(), 0);
}

#[test]
fn generated_allowance_reserves_space_for_retained_cube_triangles() {
    for count in 1..=32 {
        for spare in [0, 1, 12, 84, 4096] {
            let limit = count * 12 + spare;
            let budget = engine_render_budget(count, ThreeDRenderLimits::new(count, limit, 1));
            assert_eq!(budget.max_generated_triangles(), 12 + spare);
            assert_eq!(budget.max_generated_vertices(), 3 * (12 + spare));
            // One or more crossing cubes leave at most count-1 cubes using
            // retained triangles. No permitted combination exceeds the cap.
            for crossing in 1..=count {
                let total = (count - crossing) * 12 + budget.max_generated_triangles();
                assert!(total <= limit);
            }
        }
    }
}

#[test]
fn generated_budget_arithmetic_stays_bounded_at_extreme_author_limits() {
    let limits = ThreeDRenderLimits::new(usize::MAX, usize::MAX, u64::MAX);
    let defaults = Mesh3dRenderBudget::default();
    for count in [0, 1, 2, usize::MAX / 12, usize::MAX] {
        let budget = engine_render_budget(count, limits);
        assert!(budget.max_generated_vertices() <= defaults.max_generated_vertices());
        assert!(budget.max_generated_triangles() <= defaults.max_generated_triangles());
        assert!(budget.max_generated_upload_bytes() <= defaults.max_generated_upload_bytes());
    }
    let invalid_count = engine_render_budget(usize::MAX, limits);
    assert_eq!(invalid_count.max_generated_triangles(), 0);
    assert_eq!(invalid_count.max_generated_vertices(), 0);
}

#[test]
fn target_replacement_policy_preserves_steady_state_and_invalidates_failures() {
    let descriptor = TargetDescriptor {
        width: 640,
        height: 360,
        viewport: LogicalViewport::new(640.0, 360.0).unwrap(),
    };
    let changed = TargetDescriptor {
        viewport: LogicalViewport::new(320.0, 180.0).unwrap(),
        ..descriptor
    };
    let mut cache = TargetCache::new();
    assert!(!cache.matches(descriptor));
    cache.ensure(descriptor, || Ok::<_, ()>(1)).unwrap();
    assert!(cache.matches(descriptor));
    assert!(!cache.matches(changed));
    assert_eq!(
        cache.ensure(changed, || Err::<i32, _>("allocation")),
        Err("allocation")
    );
    assert!(!cache.matches(descriptor));
    assert!(!cache.matches(changed));
    cache.ensure(changed, || Ok::<_, ()>(2)).unwrap();
    assert!(cache.matches(changed));
    cache.clear();
    assert!(!cache.matches(changed));
}

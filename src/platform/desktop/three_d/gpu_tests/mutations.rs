//! Dev.5 adapter checks: field changes preserve retained scene and mesh ownership.

use super::*;

pub(super) fn run(renderer: &mut WgpuRenderer) -> LogicResult {
    let mut game = game()?;
    // The first object is textured and the second is a plain alias of exactly
    // the same source. Initial insertion must share its GPU geometry too.
    advance(&mut game, PhysicalKeyCode::KeyR)?;
    let mut cache = DesktopThreeD::new();
    prepare(&game, &mut cache, renderer)?;
    let ids = [
        object_id(&cache, game.sources[0])?,
        object_id(&cache, game.sources[1])?,
    ];
    let initial = scene(&cache)?.statistics();
    assert_eq!(initial.mesh_count(), 1);
    assert_eq!(initial.texture_count(), 1);
    assert!(scene(&cache)?.instance(ids[1])?.mesh().material().is_none());

    advance(&mut game, PhysicalKeyCode::Digit5)?;
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(scene(&cache)?.background(), Color::rgba(0.2, 0.3, 0.4, 0.5));
    assert_eq!(scene(&cache)?.statistics(), initial);
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    for (id, source) in ids.into_iter().zip(game.sources) {
        assert_eq!(
            object_id(&cache, source)?,
            id,
            "background must preserve IDs"
        );
    }

    advance(&mut game, PhysicalKeyCode::Digit6)?;
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    let detached = scene(&cache)?.statistics();
    assert_eq!(detached.texture_count(), 0);
    assert_eq!(
        detached.mesh_count(),
        1,
        "detach must preserve shared buffers"
    );
    assert_eq!(detached.mesh_cpu_bytes(), initial.mesh_cpu_bytes());
    assert_eq!(detached.mesh_gpu_bytes(), initial.mesh_gpu_bytes());
    for id in ids {
        assert!(scene(&cache)?.instance(id)?.mesh().material().is_none());
    }

    advance(&mut game, PhysicalKeyCode::Digit7)?;
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(scene(&cache)?.statistics(), initial);
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    assert!(scene(&cache)?.instance(ids[0])?.mesh().material().is_some());
    assert!(scene(&cache)?.instance(ids[1])?.mesh().material().is_none());

    // One logical revision may simultaneously remove the old Lambert normals,
    // UVs and texture. The adapter must detach before the dynamic geometry edit.
    advance(&mut game, PhysicalKeyCode::Digit8)?;
    let snapshot = game
        .runner
        .extracted_frame()
        .ok_or("extracted frame")?
        .three_d()
        .ok_or("3D snapshot")?;
    let error = cache
        .prepare(renderer, snapshot, LIMITS.with_mesh_limits(2, 1))
        .expect_err("one-byte upload allowance rejects after material detach");
    assert!(
        matches!(error, DesktopThreeDError::DynamicMesh { source, .. }
        if source == game.sources[0])
    );
    let rejected = scene(&cache)?.instance(ids[0])?;
    assert!(rejected.mesh().material().is_none());
    assert_eq!(rejected.mesh().source(), game.assets[0].mesh());
    assert_eq!(
        rejected
            .style()
            .surface_style()
            .ok_or("surface missing")?
            .lighting(),
        SurfaceLighting3d::Unlit
    );
    assert_eq!(object_id(&cache, game.sources[0])?, ids[0]);
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    // Retry the exact snapshot. Successful intermediate setters must not be
    // repeated against stale cached assumptions about the removed material.
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates().mesh_updates, 1);
    assert_eq!(cache.updates().texture_updates, 0);
    let changed = scene(&cache)?.instance(ids[0])?.mesh();
    assert!(changed.material().is_none());
    assert!(changed.source().normals().is_empty());
    assert!(changed.source().texture_coordinates().is_empty());
    assert_eq!(
        scene(&cache)?.instance(ids[1])?.mesh().source(),
        game.assets[0].mesh()
    );
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());

    // Rebinding attributes/materials also preserves the original object handle.
    advance(&mut game, PhysicalKeyCode::Digit9)?;
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates().mesh_updates, 1);
    assert_eq!(object_id(&cache, game.sources[0])?, ids[0]);
    assert!(scene(&cache)?.instance(ids[0])?.mesh().material().is_some());
    assert!(scene(&cache)?.instance(ids[1])?.mesh().material().is_none());
    let environment = (
        scene(&cache)?.background(),
        scene(&cache)?.lighting(),
        scene(&cache)?.fog(),
    );
    let mips = mip_pixels(&cache, ids[0])?;
    let before = scene(&cache)?.statistics();
    pollster::block_on(renderer.recover_device_and_surface())?;
    cache.restore(renderer)?;
    prepare(&game, &mut cache, renderer)?;
    assert_eq!(cache.updates(), DesktopThreeDUpdates::default());
    for (id, source) in ids.into_iter().zip(game.sources) {
        assert_eq!(object_id(&cache, source)?, id);
        assert_eq!(
            scene(&cache)?.instance(id)?.mesh().source(),
            game.assets[0].mesh()
        );
    }
    assert_eq!(
        (
            scene(&cache)?.background(),
            scene(&cache)?.lighting(),
            scene(&cache)?.fog(),
        ),
        environment
    );
    assert_eq!(mip_pixels(&cache, ids[0])?, mips);
    assert!(scene(&cache)?.instance(ids[1])?.mesh().material().is_none());
    let after = scene(&cache)?.statistics();
    assert_eq!(after.mesh_count(), before.mesh_count());
    assert_eq!(after.mesh_cpu_bytes(), before.mesh_cpu_bytes());
    assert_eq!(after.mesh_gpu_bytes(), before.mesh_gpu_bytes());
    assert_eq!(after.texture_cpu_bytes(), before.texture_cpu_bytes());
    assert_eq!(after.texture_gpu_bytes(), before.texture_gpu_bytes());
    println!(
        "dev.5 cache mutations: background IDs, plain alias, detach/rebind, rejected attribute edit retry and mutated-scene device recovery preserved"
    );
    Ok(())
}

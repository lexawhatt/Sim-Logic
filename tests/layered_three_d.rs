use sim_engine::{Mesh3d, WireframeStyle3d};
use sim_logic::{SurfaceEffects3d, prelude::*};
use std::{num::NonZeroU32, time::Duration};

const LIMITS: LayeredRenderLimits3d = LayeredRenderLimits3d::new(12, 12, 36, 1_000_000, 2_000_000);
fn nz(value: u32) -> NonZeroU32 {
    NonZeroU32::new(value).unwrap()
}
fn settings() -> LayeredView3d {
    LayeredView3d::new(
        LayerSettings3d::new(LayerResolution3d::DivideSurface(nz(2))),
        LayerSettings3d::default(),
    )
}
fn runner(layered: Option<LayeredView3d>, wire: bool) -> LogicResult<HeadlessRunner<u8>> {
    let mut config = AppConfig::default();
    config.set_three_d_render_limits(
        ThreeDRenderLimits::new(3, 36, 1_000_000).with_layered_limits(LIMITS),
    );
    let mut app = Application::<u8>::new(config)?;
    app.bind_key(PhysicalKeyCode::Space, 1)?;
    app.add_frame_system(
        |input: FrameInput<u8>, mut visuals: Query<&mut CuboidVisual3d>| {
            if input.has_press_occurrence(1) {
                for mut visual in &mut visuals {
                    let next = match visual.render_layer() {
                        RenderLayer3d::Base => RenderLayer3d::Detail,
                        RenderLayer3d::Detail => RenderLayer3d::Base,
                    };
                    visual.set_render_layer(next);
                }
            }
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(0.0, 0.0, 5.0)?, Vec3::ZERO)?;
    view.set_layered(layered);

    let mut base = CuboidVisual3d::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)?, Color::WHITE)?;
    base.set_fog_enabled(true);
    if wire {
        base.set_wireframe(Some(WireframeStyle3d::visible(
            Color::BLACK,
            LogicalPixels::new(1.0)?,
        )?));
    }
    let world = app.register_world("layers", move |world| {
        world.spawn(camera)?;
        world.insert_resource(view)?;
        world.spawn(base)?;
        let mut detail = base;
        detail.set_render_layer(RenderLayer3d::Detail);
        detail.set_wireframe(None);
        world.spawn(detail)?;
        let mut hidden = detail;
        hidden.set_visible(false);
        world.spawn(hidden)?;
        Ok(())
    })?;
    Ok(app.build_headless(world)?)
}
#[test]
fn exact_dimensions_fallback_and_zero_surface() -> LogicResult {
    let view = LogicalViewport::new(800.0, 600.0)?;
    assert_eq!(
        settings().resolve((1600, 1200), view)?,
        Some(LayeredDimensions3d {
            base: (800, 600),
            detail: (1600, 1200),
            fallback: false
        })
    );
    assert_eq!(settings().resolve((0, 0), view)?, None);
    let odd = LogicalViewport::new(801.0, 601.0)?;
    assert_eq!(
        settings().resolve((801, 601), odd),
        Err(LayeredRejection3d::Resolution)
    );
    assert_eq!(
        settings()
            .with_resolution_fallback(LayerResolutionFallback3d::FullSurface)
            .resolve((801, 601), odd)?
            .unwrap()
            .base,
        (801, 601)
    );
    let fixed = LayeredView3d::new(
        LayerSettings3d::new(LayerResolution3d::FixedPhysical(nz(400), nz(400))),
        LayerSettings3d::default(),
    );
    assert_eq!(
        fixed.resolve((800, 600), view),
        Err(LayeredRejection3d::Resolution)
    );
    let reversed = LayeredView3d::new(
        LayerSettings3d::default(),
        LayerSettings3d::new(LayerResolution3d::DivideSurface(nz(2))),
    );
    assert_eq!(
        reversed.resolve((800, 600), view),
        Err(LayeredRejection3d::Resolution)
    );
    Ok(())
}
#[test]
fn partition_metadata_and_migration_survive_complete_extraction() -> LogicResult {
    let mut runner = runner(Some(settings()), false)?;
    let first = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .three_d()
        .ok_or("3d")?;
    assert_eq!(first.validate_layered(LIMITS)?, [12, 12]);
    let source = first.cuboids()[0].source();
    let original = first.cuboids()[0].visual().render_layer();
    assert!(first.cuboids()[0].visual().fog_enabled());
    let event = [InputEvent::key(
        PhysicalKeyCode::Space,
        ButtonState::Pressed,
    )];
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::ZERO,
        &event,
        LogicalViewport::new(800.0, 600.0)?,
    ));
    assert!(matches!(outcome,FrameOutcome::Advanced(ref report) if report.failure().is_none()));
    let next = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .three_d()
        .ok_or("3d")?;
    assert_eq!(next.validate_layered(LIMITS)?, [12, 12]);
    let moved = next
        .cuboids()
        .iter()
        .find(|record| record.source() == source)
        .ok_or("same source")?;
    assert_ne!(moved.visual().render_layer(), original);
    assert_eq!(moved.source().world_generation(), runner.world_generation());
    assert_eq!(next.cuboids().len(), 2);
    Ok(())
}
#[test]
fn aggregate_budget_charges_base_twice_and_layer_caps_are_independent() -> LogicResult {
    let runner = runner(Some(settings()), false)?;
    let snapshot = runner.extracted_frame().unwrap().three_d().unwrap();
    assert_eq!(
        snapshot.validate_layered(LayeredRenderLimits3d::new(12, 12, 35, 0, 0)),
        Err(LayeredRejection3d::Triangles {
            layer: None,
            requested: 36,
            limit: 35
        })
    );
    assert_eq!(
        snapshot.validate_layered(LayeredRenderLimits3d::new(11, 12, 36, 0, 0)),
        Err(LayeredRejection3d::Triangles {
            layer: Some(RenderLayer3d::Base),
            requested: 12,
            limit: 11
        })
    );
    assert_eq!(snapshot.validate_layered(LIMITS)?, [12, 12]);
    Ok(())
}
#[test]
fn incompatible_wireframe_is_source_attributed_but_ordinary_mode_ignores_layers() -> LogicResult {
    let layered = runner(Some(settings()), true)?;
    let snapshot = layered.extracted_frame().unwrap().three_d().unwrap();
    let error = snapshot.validate_layered(LIMITS).unwrap_err();
    assert!(
        matches!(error,LayeredRejection3d::UnsupportedSurface {layer:RenderLayer3d::Base,source} if source.world_generation() == layered.world_generation())
    );
    let ordinary = runner(None, true)?;
    assert_eq!(
        ordinary
            .extracted_frame()
            .unwrap()
            .three_d()
            .unwrap()
            .validate_layered(LayeredRenderLimits3d::default())?,
        [0, 0]
    );
    Ok(())
}
#[test]
fn native_effects_never_upgrade_portable_policy() -> LogicResult {
    let effects = SurfaceEffects3d::new().with_affine_uv(true);
    let settings = LayeredView3d::new(
        LayerSettings3d::default().with_effects(effects),
        LayerSettings3d::default(),
    );
    let rejected = runner(Some(settings), false)?;
    assert_eq!(
        rejected
            .extracted_frame()
            .unwrap()
            .three_d()
            .unwrap()
            .validate_layered(LIMITS),
        Err(LayeredRejection3d::EffectsPolicy(RenderLayer3d::Base))
    );
    let native = LayeredView3d::new(
        settings.layers()[0].with_surface_policy(ThreeDSurfacePolicy::Native),
        LayerSettings3d::default(),
    );
    assert_eq!(
        runner(Some(native), false)?
            .extracted_frame()
            .unwrap()
            .three_d()
            .unwrap()
            .validate_layered(LIMITS)?,
        [12, 12]
    );
    Ok(())
}
#[test]
fn layer_assignment_belongs_to_instance_not_shared_mesh() -> LogicResult {
    let mesh = MeshAsset3d::new(Mesh3d::new(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        vec![0, 1, 2],
    )?)?;
    let a = MeshVisual3d::with_surface(
        mesh.clone(),
        Transform3d::IDENTITY,
        SurfaceStyle3d::opaque(Color::WHITE)?,
    )?;
    let mut b = a.clone();
    b.set_render_layer(RenderLayer3d::Detail);
    assert_eq!(a.render_layer(), RenderLayer3d::Base);
    assert_eq!(b.render_layer(), RenderLayer3d::Detail);
    assert!(a.asset().shares_storage(b.asset()));
    Ok(())
}

#[test]
fn masked_mesh_is_accepted_and_blend_is_rejected_with_its_source() -> LogicResult {
    for blended in [false, true] {
        let mesh = MeshAsset3d::new(Mesh3d::new(
            vec![Vec3::ZERO, Vec3::X, Vec3::Y],
            vec![0, 1, 2],
        )?)?;
        let style = if blended {
            SurfaceStyle3d::blend(Color::rgba(1.0, 1.0, 1.0, 0.5))?
        } else {
            SurfaceStyle3d::mask(Color::WHITE, 0.5)?
        };
        let mut visual = MeshVisual3d::with_surface(mesh, Transform3d::IDENTITY, style)?;
        visual.set_render_layer(RenderLayer3d::Detail);
        let mut config = AppConfig::default();
        config.set_three_d_render_limits(
            ThreeDRenderLimits::new(0, 1, 4096).with_mesh_limits(1, 4096),
        );
        let mut app = Application::<u8>::new(config)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let mut view = View3d::new(Vec3::new(0.0, 0.0, 5.0)?, Vec3::ZERO)?;
        view.set_layered(Some(settings()));
        let world = app.register_world("alpha", move |world| {
            world.spawn(camera)?;
            world.insert_resource(view)?;
            world.spawn(visual.clone())?;
            Ok(())
        })?;
        let runner = app.build_headless(world)?;
        let snapshot = runner.extracted_frame().unwrap().three_d().unwrap();
        let admission = snapshot.validate_layered(LIMITS);
        if blended {
            assert_eq!(
                admission,
                Err(LayeredRejection3d::UnsupportedSurface {
                    layer: RenderLayer3d::Detail,
                    source: snapshot.meshes()[0].source()
                })
            );
        } else {
            assert_eq!(admission?, [0, 1]);
            assert_eq!(snapshot.layer_meshes(RenderLayer3d::Detail).count(), 1);
        }
    }
    Ok(())
}

#[cfg(feature = "desktop")]
#[test]
fn continue_ui_without_feedback_is_rejected_before_window_creation() -> LogicResult {
    let mut app = Application::<u8>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(0.0, 0.0, 5.0)?, Vec3::ZERO)?;
    view.set_layered(Some(
        settings().with_failure_policy(LayeredFailurePolicy3d::ContinueUi),
    ));
    let world = app.register_world("invalid-feedback", move |world| {
        world.spawn(camera)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let result = app.run_desktop(
        world,
        DesktopConfig::new("must not create a window", 64.0, 64.0)?,
    );
    assert!(matches!(
        result,
        Err(sim_logic::desktop::DesktopRunError::LayeredConfiguration(
            sim_logic::desktop::DesktopThreeDError::MissingLayeredFeedback
        ))
    ));
    Ok(())
}

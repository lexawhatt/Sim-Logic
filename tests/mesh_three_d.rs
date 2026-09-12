use std::time::Duration;

use sim_engine::{Mesh3d, MeshEdge3d, TextureCoordinate2d};
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

fn topology(offset: f32) -> LogicResult<Mesh3d> {
    Ok(Mesh3d::new(
        vec![
            Vec3::new(offset, 0.0, 0.0)?,
            Vec3::new(offset + 1.0, 0.0, 0.0)?,
            Vec3::new(offset, 1.0, 0.0)?,
        ],
        vec![0, 1, 2],
    )?)
}

fn mesh() -> LogicResult<MeshVisual3d> {
    Ok(MeshVisual3d::new(
        MeshAsset3d::new(topology(0.0)?)?,
        Transform3d::IDENTITY,
        Color::WHITE,
    )?)
}

fn view() -> LogicResult<View3d> {
    Ok(View3d::new(Vec3::new(3.0, 4.0, 6.0)?, Vec3::ZERO)?)
}

fn app(limits: ThreeDRenderLimits) -> LogicResult<Application<u8>> {
    let mut config = AppConfig::default();
    config.set_three_d_render_limits(limits);
    Ok(Application::new(config)?)
}

fn advance(
    runner: &mut HeadlessRunner<u8>,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        Duration::from_millis(20),
        events,
        LogicalViewport::new(800.0, 600.0)?,
    )) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

#[test]
fn assets_share_clone_identity_but_do_not_guess_content_equality() -> LogicResult {
    let source = topology(0.0)?;
    let first = MeshAsset3d::new(source.clone())?;
    let shared = MeshAsset3d::new(source)?;
    let equal_contents = MeshAsset3d::new(topology(0.0)?)?;
    assert!(first.shares_storage(&first.clone()));
    assert!(first.shares_storage(&shared));
    assert!(!first.shares_storage(&equal_contents));
    assert_eq!(first.mesh(), equal_contents.mesh());
    Ok(())
}

#[test]
fn unsupported_geometry_and_atomic_value_rejections_are_explicit() -> LogicResult {
    let edges = Mesh3d::with_display_edges(
        vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)?],
        vec![],
        vec![MeshEdge3d::new(0, 1)?],
    )?;
    assert_eq!(
        MeshAsset3d::new(edges).err(),
        Some(MeshVisualError::UnsupportedTopology)
    );
    let source = topology(0.0)?;
    let textured = Mesh3d::textured(
        source.vertices().to_vec(),
        vec![TextureCoordinate2d::new(0.0, 0.0)?; 3],
        vec![0, 1, 2],
        vec![],
    )?;
    // dev.4 preserves UV attributes; an actual texture is optional and attached
    // independently through MeshVisual3d::set_texture.
    assert_eq!(
        MeshAsset3d::new(textured)?
            .mesh()
            .texture_coordinates()
            .len(),
        3
    );
    let mut visual = mesh()?;
    let original = visual.clone();
    assert!(visual.set_color(Color::TRANSPARENT).is_err());
    assert_eq!(visual, original);
    let collapsed = Transform3d::new(
        Vec3::new(f32::MAX, 0.0, 0.0)?,
        Rotation3d::IDENTITY,
        Vec3::new(1.0, 1.0, 1.0)?,
    )?;
    assert_eq!(
        visual.set_transform(collapsed),
        Err(MeshVisualError::CollapsedGeometry)
    );
    assert_eq!(visual, original);
    Ok(())
}

#[test]
fn mesh_is_standard_and_hidden_or_disabled_instances_consume_no_budget() -> LogicResult {
    let visual = mesh()?;
    let bytes = visual.asset().source_bytes();
    let mut app = app(ThreeDRenderLimits::new(0, 1, 1).with_mesh_limits(1, bytes))?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let view = view()?;
    let world = app.register_world("mesh", move |world| {
        world.spawn(camera)?;
        world.spawn(visual.clone())?;
        world.spawn((visual.clone(), Disabled))?;
        let mut hidden = visual.clone();
        hidden.set_visible(false);
        world.spawn(hidden)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let runner = app.build_headless(world)?;
    let frame = runner.extracted_frame().ok_or("snapshot")?;
    assert_eq!(runner.components::<MeshVisual3d>().count(), 3);
    assert_eq!(frame.resolved_meshes().len(), 1);
    assert_eq!(frame.three_d().ok_or("view")?.source_triangle_count(), 1);
    assert!(frame.resolved_cuboids().is_empty());
    assert_eq!(
        frame.resolved_meshes()[0].source().world_generation(),
        runner.world_generation()
    );
    Ok(())
}

#[test]
fn source_bytes_use_capacity_and_shared_instances_are_conservatively_charged() -> LogicResult {
    let mut vertices = Vec::with_capacity(128);
    vertices.extend_from_slice(topology(0.0)?.vertices());
    let asset = MeshAsset3d::new(Mesh3d::new(vertices, vec![0, 1, 2])?)?;
    assert!(asset.source_bytes() >= 128 * std::mem::size_of::<Vec3>());
    let visual = MeshVisual3d::new(asset.clone(), Transform3d::IDENTITY, Color::WHITE)?;
    for max_bytes in [asset.source_bytes() * 2 - 1, asset.source_bytes() * 2] {
        let mut app = app(ThreeDRenderLimits::new(0, 2, 1).with_mesh_limits(2, max_bytes))?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let view = view()?;
        let visual = visual.clone();
        let world = app.register_world("shared", move |world| {
            world.spawn(camera)?;
            world.spawn(visual.clone())?;
            world.spawn(visual.clone())?;
            world.insert_resource(view)?;
            Ok(())
        })?;
        assert_eq!(
            app.build_headless(world).is_ok(),
            max_bytes == asset.source_bytes() * 2
        );
    }
    Ok(())
}

#[test]
fn meshes_are_opt_in_and_share_triangle_limits_with_cuboids() -> LogicResult {
    for (meshes, triangles, accepted) in [(0, 13, false), (1, 12, false), (1, 13, true)] {
        let visual = mesh()?;
        let mut app = app(ThreeDRenderLimits::new(1, triangles, 1)
            .with_mesh_limits(meshes, visual.asset().source_bytes()))?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let view = view()?;
        let cube = CuboidVisual3d::new(Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)?, Color::WHITE)?;
        let world = app.register_world("mixed", move |world| {
            world.spawn(camera)?;
            world.spawn(cube)?;
            world.spawn(visual.clone())?;
            world.insert_resource(view)?;
            Ok(())
        })?;
        assert_eq!(app.build_headless(world).is_ok(), accepted);
    }
    Ok(())
}

#[test]
fn revision_update_preserves_entity_and_failed_extraction_preserves_snapshot() -> LogicResult {
    let original = mesh()?;
    let original_asset = original.asset().clone();
    let replacement = MeshAsset3d::new(topology(2.0)?)?;
    let expected = replacement.clone();
    let mut app = app(ThreeDRenderLimits::new(0, 1, 1).with_mesh_limits(1, 1024))?;
    app.bind_key(PhysicalKeyCode::Space, 1)?;
    app.add_fallible_frame_system(
        move |input: FrameInput<u8>,
              mut meshes: Query<&mut MeshVisual3d>,
              mut commands: Commands|
              -> LogicResult {
            if input.has_press_occurrence(1) {
                for mut mesh in &mut meshes {
                    mesh.set_asset(replacement.clone())?;
                }
            }
            if input.has_release_occurrence(1) {
                commands.spawn(mesh()?)?;
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let view = view()?;
    let world = app.register_world("editing", move |world| {
        world.spawn(camera)?;
        world.spawn(original.clone())?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(world)?;
    let source = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_meshes()[0]
        .source();
    for _ in 0..4 {
        assert!(advance(&mut runner, &[])?.failure().is_none());
        assert!(
            runner
                .extracted_frame()
                .ok_or("snapshot")?
                .resolved_meshes()[0]
                .visual()
                .asset()
                .shares_storage(&original_asset)
        );
    }
    assert!(
        advance(
            &mut runner,
            &[InputEvent::key(
                PhysicalKeyCode::Space,
                ButtonState::Pressed
            )]
        )?
        .failure()
        .is_none()
    );
    let revised = &runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_meshes()[0];
    assert_eq!(revised.source(), source);
    assert!(revised.visual().asset().shares_storage(&expected));
    let failed = advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::Extraction(ExtractionError::ThreeD(
            ThreeDExtractionError::LimitExceeded {
                resource: ThreeDLimitResource::Meshes,
                ..
            }
        )))
    ));
    let preserved = &runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_meshes()[0];
    assert_eq!(preserved.source(), source);
    assert!(preserved.visual().asset().shares_storage(&expected));
    Ok(())
}

#[test]
fn missing_or_disabled_view_does_not_require_mesh_allowances() -> LogicResult {
    for enabled in [None, Some(false)] {
        let mut app = app(ThreeDRenderLimits::default())?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let visual = mesh()?;
        let mut view = view()?;
        view.set_enabled(false);
        let world = app.register_world("off", move |world| {
            world.spawn(camera)?;
            world.spawn(visual.clone())?;
            if enabled.is_some() {
                world.insert_resource(view)?;
            }
            Ok(())
        })?;
        let runner = app.build_headless(world)?;
        assert!(
            runner
                .extracted_frame()
                .ok_or("snapshot")?
                .resolved_meshes()
                .is_empty()
        );
    }
    Ok(())
}

#[test]
fn world_replacement_publishes_only_new_mesh_sources_and_assets() -> LogicResult {
    let old_visual = mesh()?;
    let new_asset = MeshAsset3d::new(topology(2.0)?)?;
    let expected = new_asset.clone();
    let new_visual = MeshVisual3d::new(new_asset, Transform3d::IDENTITY, Color::BLACK)?;
    let mut app = app(ThreeDRenderLimits::new(0, 1, 1).with_mesh_limits(1, 1024))?;
    app.bind_key(PhysicalKeyCode::Enter, 1)?;
    app.add_world_replacement_on_press_system();
    let camera = ActiveCamera2d::centered(1.0)?;
    let view = view()?;
    let target = app.register_world("new-mesh", move |world| {
        world.spawn(camera)?;
        world.spawn(new_visual.clone())?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let initial = app.register_world("old-mesh", move |world| {
        world.spawn(camera)?;
        world.spawn(old_visual.clone())?;
        world.insert_resource(view)?;
        world.insert_resource(WorldReplacementOnPress::new(1_u8, target))?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    assert!(runner.resource::<WorldReplacementOnPress<u8>>().is_some());
    let old = runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_meshes()[0]
        .source();
    let report = advance(
        &mut runner,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(
        matches!(report.transition(), FrameTransition::Committed { .. }),
        "{report:?}"
    );
    let record = &runner
        .extracted_frame()
        .ok_or("snapshot")?
        .resolved_meshes()[0];
    assert_ne!(record.source().world_generation(), old.world_generation());
    assert_eq!(
        record.source().world_generation(),
        runner.world_generation()
    );
    assert!(record.visual().asset().shares_storage(&expected));
    Ok(())
}

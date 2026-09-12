//! CPU-only contracts for the Engine 0.4 presentation descriptors.

use sim_engine::{
    AmbientLight3d, Fog3d, Lighting3d, Mesh3d, Mesh3dAttributes, SurfaceAlphaMode3d,
    SurfaceLighting3d, SurfaceSidedness3d, SurfaceStyle3d, TextureAddressMode3d,
    TextureCoordinate2d, TextureUvTransform3d,
};
use sim_logic::{
    prelude::*,
    screen::{ImageFilter, ImageRegion},
    three_d::{TextureAsset3d, TextureVisual3d, TextureVisualError, ThreeDSurfacePolicy},
};

fn asset(uvs: bool, normals: bool) -> LogicResult<MeshAsset3d> {
    let mut attributes =
        Mesh3dAttributes::new().with_vertex_colors(vec![Color::rgba(0.5, 0.75, 1.0, 0.5); 3])?;
    if uvs {
        attributes = attributes.with_texture_coordinates(vec![
            TextureCoordinate2d::new(0.0, 0.0)?,
            TextureCoordinate2d::new(1.0, 0.0)?,
            TextureCoordinate2d::new(0.0, 1.0)?,
        ]);
    }
    if normals {
        attributes = attributes.with_normals(vec![Vec3::Z; 3])?;
    }
    Ok(MeshAsset3d::new(Mesh3d::with_attributes(
        vec![Vec3::ZERO, Vec3::X, Vec3::Y],
        vec![0, 1, 2],
        vec![],
        attributes,
    )?)?)
}

fn texture() -> LogicResult<TextureVisual3d> {
    Ok(TextureVisual3d::new(TextureAsset3d::rgba8(
        3,
        5,
        vec![128; 3 * 5 * 4],
        3 * 5 * 4,
    )?)
    .with_mipmaps(true))
}

#[test]
fn view_preserves_strict_defaults_and_accepts_explicit_environment() -> LogicResult {
    let mut view = View3d::new(Vec3::new(3.0, 4.0, 6.0)?, Vec3::ZERO)?;
    assert_eq!(view.surface_policy(), ThreeDSurfacePolicy::StrictPortable);
    assert_eq!(view.lighting(), Lighting3d::default());
    assert_eq!(view.fog(), None);
    view.set_surface_policy(ThreeDSurfacePolicy::Native);
    view.set_background(Color::TRANSPARENT)?;
    let lighting = Lighting3d::new(AmbientLight3d::new(Color::WHITE, 0.25)?);
    let fog = Fog3d::new(Color::WHITE, 10.0, 0.05)?;
    view.set_lighting(lighting);
    view.set_fog(Some(fog));
    view.set_orthographic(
        WorldLength::new(5.0)?,
        WorldLength::new(0.1)?,
        WorldLength::new(100.0)?,
    )?;
    let viewport = LogicalViewport::new(800.0, 400.0)?;
    assert!(!view.camera(viewport)?.projection().is_perspective());
    assert_eq!(view.camera(viewport)?.projection().aspect_ratio(), 2.0);
    assert_eq!(view.lighting(), lighting);
    assert_eq!(view.fog(), Some(fog));
    let original = view;
    assert!(
        view.set_orthographic(
            WorldLength::new(5.0)?,
            WorldLength::new(100.0)?,
            WorldLength::new(1.0)?
        )
        .is_err()
    );
    assert_eq!(view, original);
    view.set_perspective(1.0, WorldLength::new(0.1)?, WorldLength::new(100.0)?)?;
    assert!(view.camera(viewport)?.projection().is_perspective());
    assert_eq!(view.surface_policy(), ThreeDSurfacePolicy::Native);
    assert_eq!(view.background(), Color::TRANSPARENT);
    Ok(())
}

#[test]
fn surface_tint_changes_preserve_all_material_settings() -> LogicResult {
    for surface in [
        SurfaceStyle3d::mask(Color::WHITE, 0.45)?,
        SurfaceStyle3d::blend(Color::WHITE)?,
    ] {
        let surface = surface
            .with_lighting(SurfaceLighting3d::Lambert)
            .with_sidedness(SurfaceSidedness3d::FrontOnly)
            .with_fog(true);
        let mut visual =
            MeshVisual3d::with_surface(asset(true, true)?, Transform3d::IDENTITY, surface)?;
        visual.set_texture(Some(texture()?))?;
        visual.set_color(Color::rgba(0.2, 0.3, 0.4, 0.5))?;
        assert_eq!(visual.surface().alpha_mode(), surface.alpha_mode());
        assert_eq!(visual.surface().mask_cutoff(), surface.mask_cutoff());
        assert_eq!(visual.surface().lighting(), SurfaceLighting3d::Lambert);
        assert_eq!(visual.surface().sidedness(), SurfaceSidedness3d::FrontOnly);
        assert!(visual.surface().fog_enabled());
        let original = visual.clone();
        assert!(
            visual
                .set_color(Color::rgba(f32::NAN, 0.0, 0.0, 0.0))
                .is_err()
        );
        assert_eq!(visual, original);
        assert_eq!(
            visual.set_asset(asset(true, false)?),
            Err(MeshVisualError::MissingNormals)
        );
        assert_eq!(visual, original);
        assert_eq!(
            visual.set_asset(asset(false, true)?),
            Err(MeshVisualError::MissingTextureCoordinates)
        );
        assert_eq!(visual, original);
    }
    Ok(())
}

#[test]
fn legacy_mesh_defaults_remain_unlit_opaque_and_attribute_checks_are_atomic() -> LogicResult {
    let mut visual = MeshVisual3d::new(asset(false, false)?, Transform3d::IDENTITY, Color::WHITE)?;
    assert_eq!(visual.surface().alpha_mode(), SurfaceAlphaMode3d::Opaque);
    assert_eq!(visual.surface().lighting(), SurfaceLighting3d::Unlit);
    assert_eq!(visual.texture(), None);
    let original = visual.clone();
    assert_eq!(
        visual.set_surface(
            SurfaceStyle3d::opaque(Color::WHITE)?.with_lighting(SurfaceLighting3d::Lambert)
        ),
        Err(MeshVisualError::MissingNormals)
    );
    assert_eq!(visual, original);
    assert_eq!(
        visual.set_texture(Some(texture()?)),
        Err(MeshVisualError::MissingTextureCoordinates)
    );
    assert_eq!(visual, original);
    assert!(visual.set_color(Color::TRANSPARENT).is_err());
    assert_eq!(visual, original);
    Ok(())
}

#[test]
fn texture_snapshots_preserve_aliases_and_strided_patches_are_exact() -> LogicResult {
    let old = TextureAsset3d::rgba8(3, 5, vec![0; 60], 60)?;
    let shared = old.clone();
    assert!(old.shares_storage(&shared));
    let region = ImageRegion::new(1, 1, 1, 2)?;
    let revised =
        old.with_region_update(region, 8, &[1, 2, 3, 4, 99, 99, 99, 99, 5, 6, 7, 8], 60)?;
    assert_eq!(old.pixels(), &[0; 60]);
    assert_eq!(shared.pixels(), old.pixels());
    assert!(!revised.shares_storage(&old));
    assert!(revised.is_direct_revision_of(&old));
    assert_eq!(revised.updated_region(), Some(region));
    assert_eq!(&revised.pixels()[16..20], &[1, 2, 3, 4]);
    assert_eq!(&revised.pixels()[28..32], &[5, 6, 7, 8]);
    assert_eq!(&revised.pixels()[20..28], &[0; 8]);
    let branch = old.with_region_update(region, 4, &[9; 8], 60)?;
    assert!(branch.is_direct_revision_of(&old));
    assert!(!branch.is_direct_revision_of(&revised));
    let next = revised.with_region_update(region, 4, &[10; 8], 60)?;
    assert!(next.is_direct_revision_of(&revised));
    assert!(!next.is_direct_revision_of(&old));
    Ok(())
}

#[test]
fn texture_validation_covers_capacities_patch_bounds_and_mip_totals() -> LogicResult {
    assert_eq!(
        TextureAsset3d::rgba8(0, 1, vec![], 0).err(),
        Some(TextureVisualError::InvalidDimensions)
    );
    assert!(matches!(
        TextureAsset3d::rgba8(1, 1, vec![0; 3], 4),
        Err(TextureVisualError::PixelLength { .. })
    ));
    let mut oversized = Vec::with_capacity(128);
    oversized.extend_from_slice(&[0; 4]);
    assert!(matches!(
        TextureAsset3d::rgba8(1, 1, oversized, 127),
        Err(TextureVisualError::SourceLimit { .. })
    ));
    let old = TextureAsset3d::rgba8(3, 5, vec![0; 60], 60)?;
    let region = ImageRegion::new(1, 1, 1, 2)?;
    assert_eq!(
        old.with_region_update(region, 3, &[0; 7], 60).err(),
        Some(TextureVisualError::InvalidPatchLayout)
    );
    assert_eq!(
        old.with_region_update(region, 8, &[0; 16], 60).err(),
        Some(TextureVisualError::InvalidPatchLayout)
    );
    assert_eq!(
        old.with_region_update(ImageRegion::new(3, 0, 1, 1)?, 4, &[0; 4], 60)
            .err(),
        Some(TextureVisualError::RegionOutOfBounds)
    );
    assert!(matches!(
        old.with_region_update(region, 4, &[0; 8], 59),
        Err(TextureVisualError::SourceLimit { .. })
    ));
    assert_eq!(old.pixels(), &[0; 60]);
    let descriptor = TextureVisual3d::new(old)
        .with_mipmaps(true)
        .with_filter(ImageFilter::Linear)
        .with_address_mode(TextureAddressMode3d::Repeat)
        .with_uv_transform(TextureUvTransform3d::new(Vec2::new(-4.0, 2.0), Vec2::ZERO)?);
    assert_eq!(descriptor.texel_bytes()?, 60 + 8 + 4);
    assert_eq!(descriptor.filter(), ImageFilter::Linear);
    assert_eq!(descriptor.address_mode(), TextureAddressMode3d::Repeat);
    assert!(
        descriptor
            .clone()
            .with_tint(Color::rgba(0.0, 0.0, 0.0, 1.1))
            .is_err()
    );
    assert_eq!(
        TextureVisual3d::new(TextureAsset3d::rgba8(1, 7, vec![0; 28], 28)?)
            .with_mipmaps(true)
            .texel_bytes()?,
        28 + 12 + 4
    );
    Ok(())
}

#[test]
fn textured_extraction_charges_each_visible_instance_and_full_mip_chain() -> LogicResult {
    for (source_limit, gpu_limit, accepted) in
        [(119, 144, false), (120, 143, false), (120, 144, true)]
    {
        let topology = asset(true, true)?;
        let mut visual = MeshVisual3d::with_surface(
            topology,
            Transform3d::IDENTITY,
            SurfaceStyle3d::blend(Color::WHITE)?,
        )?;
        visual.set_texture(Some(texture()?))?;
        let mut config = AppConfig::default();
        config.set_three_d_render_limits(
            ThreeDRenderLimits::new(0, 2, 1)
                .with_mesh_limits(2, visual.asset().source_bytes() * 2)
                .with_texture_limits(source_limit, gpu_limit),
        );
        let mut app = Application::<u8>::new(config)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let view = View3d::new(Vec3::new(3.0, 4.0, 6.0)?, Vec3::ZERO)?;
        let world = app.register_world("textures", move |world| {
            world.spawn(camera)?;
            world.insert_resource(view)?;
            world.spawn(visual.clone())?;
            world.spawn(visual.clone())?;
            let mut hidden = visual.clone();
            hidden.set_visible(false);
            world.spawn(hidden)?;
            world.spawn((
                visual.clone(),
                sim_logic::bevy_ecs::entity_disabling::Disabled,
            ))?;
            Ok(())
        })?;
        assert_eq!(app.build_headless(world).is_ok(), accepted);
    }
    Ok(())
}

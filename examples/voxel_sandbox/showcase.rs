//! Small non-colliding material studies, independent of saved block identities.

use sim_engine::{
    Mesh3d, Mesh3dAttributes, SurfaceLighting3d, SurfaceStyle3d, TextureCoordinate2d,
};
use sim_logic::{prelude::*, screen::ImageRegion};

use super::{
    materials::{Palette, Settings},
    model::Block,
    scene::{Local, Phase},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Showcase {
    PaintedBoard,
    GlassFront,
    GlassBack,
    ScaledPanel,
}

#[derive(Default, Resource)]
pub struct PatchState {
    applied: u64,
}

pub fn build(palette: &Palette) -> LogicResult<Vec<(Showcase, MeshVisual3d)>> {
    let panel = panel_mesh(0.0)?;
    let slanted = panel_mesh(0.3)?;
    let mut objects = Vec::new();
    for (kind, asset, position, scale, color) in [
        (
            Showcase::PaintedBoard,
            panel.clone(),
            [9.5, 6.5, 8.5],
            [2.0, 1.8, 1.0],
            Color::WHITE,
        ),
        (
            Showcase::GlassBack,
            panel.clone(),
            [12.5, 6.5, 7.8],
            [2.5, 2.0, 1.0],
            Color::rgba(0.2, 0.65, 1.0, 0.45),
        ),
        (
            Showcase::GlassFront,
            panel,
            [13.0, 6.5, 8.8],
            [2.5, 2.0, 1.0],
            Color::rgba(1.0, 0.55, 0.15, 0.4),
        ),
        (
            Showcase::ScaledPanel,
            slanted,
            [16.0, 6.5, 8.0],
            [2.3, 0.8, 0.4],
            Color::rgb(0.3, 0.8, 0.6),
        ),
    ] {
        let surface = match kind {
            Showcase::GlassFront | Showcase::GlassBack => SurfaceStyle3d::blend(color)?,
            _ => SurfaceStyle3d::opaque(color)?,
        }
        .with_lighting(if kind == Showcase::PaintedBoard {
            SurfaceLighting3d::Unlit
        } else {
            SurfaceLighting3d::Lambert
        })
        .with_fog(kind != Showcase::PaintedBoard);
        let mut visual = MeshVisual3d::with_surface(
            asset,
            Transform3d::new(
                Vec3::new(position[0], position[1], position[2])?,
                Rotation3d::IDENTITY,
                Vec3::new(scale[0], scale[1], scale[2])?,
            )?,
            surface,
        )?;
        visual.set_visible(false);
        if kind == Showcase::PaintedBoard {
            // Initially shared with terrain. First edit must isolate this object;
            // later edits follow its private revision chain and can reuse buffers.
            visual.set_texture(Some(
                palette
                    .texture(Block::Stone, true)?
                    .with_filter(ImageFilter::Linear)
                    .with_uv_transform(TextureUvTransform3d::new(
                        Vec2::new(-2.0, 3.0),
                        Vec2::new(0.25, -0.5),
                    )?),
            ))?;
        } else if kind == Showcase::GlassFront {
            visual.set_texture(Some(palette.texture(Block::Leaves, true)?))?;
        }
        objects.push((kind, visual));
    }
    Ok(objects)
}

fn panel_mesh(slant: f32) -> LogicResult<MeshAsset3d> {
    let normal = Vec3::new(-slant, 0.0, 1.0)?;
    Ok(MeshAsset3d::new(Mesh3d::with_attributes(
        vec![
            Vec3::new(-0.5, -0.5, 0.0)?,
            Vec3::new(0.5, -0.5, slant)?,
            Vec3::new(0.5, 0.5, slant)?,
            Vec3::new(-0.5, 0.5, 0.0)?,
        ],
        vec![0, 1, 2, 0, 2, 3],
        Vec::new(),
        Mesh3dAttributes::new()
            .with_texture_coordinates(vec![
                TextureCoordinate2d::new(0.0, 1.0)?,
                TextureCoordinate2d::new(1.0, 1.0)?,
                TextureCoordinate2d::new(1.0, 0.0)?,
                TextureCoordinate2d::new(0.0, 0.0)?,
            ])
            .with_normals(vec![normal; 4])?
            .with_vertex_colors(vec![
                Color::rgb(0.8, 0.8, 0.8),
                Color::WHITE,
                Color::WHITE,
                Color::rgb(0.8, 0.8, 0.8),
            ])?,
    )?)?)
}

pub fn update(
    local: Res<Local>,
    settings: Res<Settings>,
    mut state: ResMut<PatchState>,
    mut visuals: Query<(&Showcase, &mut MeshVisual3d)>,
) -> LogicResult {
    for (_, mut visual) in &mut visuals {
        visual.set_visible(settings.studies && local.phase == Phase::Ready);
    }
    if local.phase != Phase::Ready || state.applied == settings.patch_requests {
        return Ok(());
    }
    for (kind, mut visual) in &mut visuals {
        if *kind != Showcase::PaintedBoard {
            continue;
        }
        let old = visual.texture().ok_or("painted board texture missing")?;
        let color = if settings.patch_requests.is_multiple_of(2) {
            [20, 240, 130, 255]
        } else {
            [245, 40, 180, 255]
        };
        // Four useful RGBA pixels followed by a four-byte stride gap. The last
        // row omits padding, matching Engine's explicit strided-update contract.
        let mut patch = [0; 76];
        for row in 0..4 {
            for x in 0..4 {
                patch[row * 20 + x * 4..row * 20 + x * 4 + 4].copy_from_slice(&color);
            }
        }
        let revised =
            old.asset()
                .with_region_update(ImageRegion::new(6, 6, 4, 4)?, 20, &patch, 1024)?;
        let texture = old.clone().with_asset(revised);
        visual.set_texture(Some(texture))?;
    }
    state.applied = settings.patch_requests;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studies_keep_separate_blend_objects_and_scaled_lambert_normals() -> LogicResult {
        let palette = Palette::new()?;
        let objects = build(&palette)?;
        assert_eq!(objects.len(), 4);
        for (kind, visual) in objects {
            assert_eq!(visual.asset().mesh().triangle_count(), 2);
            assert_eq!(visual.asset().mesh().normals().len(), 4);
            assert_eq!(
                visual.surface().sidedness(),
                sim_engine::SurfaceSidedness3d::TwoSided
            );
            if matches!(kind, Showcase::GlassFront | Showcase::GlassBack) {
                assert_eq!(
                    visual.surface().alpha_mode(),
                    sim_engine::SurfaceAlphaMode3d::Blend
                );
            }
            if kind == Showcase::PaintedBoard {
                assert!(
                    visual
                        .texture()
                        .ok_or("board")?
                        .asset()
                        .shares_storage(palette.texture(Block::Stone, true)?.asset())
                );
                assert_eq!(visual.surface().lighting(), SurfaceLighting3d::Unlit);
                assert!(!visual.surface().fog_enabled());
            }
        }
        Ok(())
    }

    #[test]
    fn input_toggles_change_visuals_and_patch_only_the_study_board() -> LogicResult {
        use super::super::{app, projection};
        use std::time::Duration;

        let (app, initial) =
            app::build_application(Default::default(), "unused-visual-test.save".into())?;
        let mut runner = app.build_headless(initial)?;
        let advance =
            |runner: &mut HeadlessRunner<app::Action>, input: &[InputEvent]| -> LogicResult {
                let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
                    Duration::ZERO,
                    input,
                    LogicalViewport::new(1100.0, 720.0)?,
                )) else {
                    return Err("visual test frame rejected".into());
                };
                assert!(report.failure().is_none(), "{:?}", report.failure());
                Ok(())
            };
        for _ in 0..100 {
            advance(&mut runner, &[])?;
        }
        let board = runner
            .components::<Showcase>()
            .find(|(_, kind)| **kind == Showcase::PaintedBoard)
            .map(|(entity, _)| entity)
            .ok_or("board")?;
        let before = runner
            .component::<MeshVisual3d>(board)?
            .texture()
            .ok_or("board texture")?
            .asset()
            .clone();
        let terrain = runner
            .components::<projection::ChunkPart>()
            .find(|(_, part)| part.block == Block::Stone)
            .map(|(entity, _)| entity)
            .ok_or("stone part")?;
        assert!(
            runner
                .component::<MeshVisual3d>(terrain)?
                .texture()
                .ok_or("stone texture")?
                .asset()
                .shares_storage(&before)
        );
        let press = |key| {
            [
                InputEvent::key(key, ButtonState::Pressed),
                InputEvent::key(key, ButtonState::Released),
            ]
        };
        advance(&mut runner, &press(PhysicalKeyCode::KeyT))?;
        let revised = runner
            .component::<MeshVisual3d>(board)?
            .texture()
            .ok_or("board texture")?
            .asset()
            .clone();
        assert!(revised.is_direct_revision_of(&before));
        assert!(
            runner
                .component::<MeshVisual3d>(terrain)?
                .texture()
                .ok_or("stone texture")?
                .asset()
                .shares_storage(&before)
        );
        advance(&mut runner, &press(PhysicalKeyCode::KeyT))?;
        let next = runner
            .component::<MeshVisual3d>(board)?
            .texture()
            .ok_or("board texture")?
            .asset();
        assert!(next.is_direct_revision_of(&revised));
        assert_ne!(next.pixels(), revised.pixels());
        advance(&mut runner, &press(PhysicalKeyCode::KeyL))?;
        assert_eq!(
            runner
                .component::<MeshVisual3d>(terrain)?
                .surface()
                .lighting(),
            SurfaceLighting3d::Unlit
        );
        advance(&mut runner, &press(PhysicalKeyCode::KeyF))?;
        assert!(runner.resource::<View3d>().ok_or("view")?.fog().is_none());
        advance(&mut runner, &press(PhysicalKeyCode::KeyM))?;
        assert!(
            !runner
                .component::<MeshVisual3d>(terrain)?
                .texture()
                .ok_or("texture")?
                .mipmaps()
        );
        advance(&mut runner, &press(PhysicalKeyCode::KeyV))?;
        assert!(
            runner
                .resource::<View3d>()
                .ok_or("view")?
                .orthographic_span()
                .is_some()
        );
        // Visual inspection never consumes inventory, edits blocks or writes saves.
        let session = runner.app_resource::<app::Session>().ok_or("session")?;
        assert_eq!(session.edits, 0);
        assert_eq!(session.game.inventory.count(Block::Stone), 32);
        Ok(())
    }
}

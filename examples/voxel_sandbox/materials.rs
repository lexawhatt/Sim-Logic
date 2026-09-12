//! Presentation assets/settings only; none of these values enter game saves.

use sim_engine::{
    AmbientLight3d, DirectionalLight3d, Fog3d, Lighting3d, SurfaceLighting3d, SurfaceSidedness3d,
    SurfaceStyle3d, TextureAddressMode3d,
};
use sim_logic::{prelude::*, screen::ImageFilter};

use super::{
    app::Action,
    model::{Block, RegionId},
    projection::ChunkPart,
    scene::{Local, Phase},
    showcase::Showcase,
};

pub struct Palette(pub [TextureVisual3d; 32]);

impl Palette {
    pub fn new() -> LogicResult<Self> {
        let mut tiles = Vec::new();
        for block in Block::SOLID {
            let mut pixels = Vec::with_capacity(16 * 16 * 4);
            for y in 0..16_u32 {
                for x in 0..16_u32 {
                    let value = match block {
                        Block::Wood | Block::OakPlanks | Block::DarkPlanks => {
                            if (x + y / 5) % 5 == 0 { 155 } else { 245 }
                        }
                        Block::Stone | Block::Cobblestone | Block::Bricks | Block::Sandstone => {
                            if x % 8 == 0 || y % 8 == 0 {
                                165
                            } else {
                                225 + ((x * 7 + y * 3) % 30) as u8
                            }
                        }
                        Block::Glass | Block::BlueGlass | Block::Ice => {
                            if x == 0 || y == 0 || x == 15 || y == 15 {
                                245
                            } else {
                                180
                            }
                        }
                        Block::Lamp => {
                            if (x / 4 + y / 4) % 2 == 0 {
                                255
                            } else {
                                190
                            }
                        }
                        _ => 210 + ((x * 13 + y * 7 + x * y) % 45) as u8,
                    };
                    let alpha = if block == Block::Leaves && ((x / 2 + y / 2) % 3 == 0) {
                        0
                    } else {
                        255
                    };
                    pixels.extend_from_slice(&[value, value, value, alpha]);
                }
            }
            tiles.push(
                TextureVisual3d::new(TextureAsset3d::rgba8(16, 16, pixels, 1024)?)
                    .with_mipmaps(true)
                    .with_filter(ImageFilter::Nearest)
                    .with_address_mode(TextureAddressMode3d::Repeat),
            );
        }
        Ok(Self(
            tiles
                .try_into()
                .map_err(|_| "complete block texture palette required")?,
        ))
    }

    pub fn texture(&self, block: Block, mipmaps: bool) -> LogicResult<TextureVisual3d> {
        let index = Block::SOLID
            .iter()
            .position(|candidate| *candidate == block)
            .ok_or("Air has no texture")?;
        Ok(self.0[index].clone().with_mipmaps(mipmaps))
    }
}

#[derive(Resource)]
pub struct Settings {
    pub studies: bool,
    pub lighting: bool,
    pub fog: bool,
    pub mipmaps: bool,
    pub orthographic: bool,
    pub patch_requests: u64,
    applied: Option<(bool, bool)>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            studies: false,
            lighting: true,
            fog: true,
            mipmaps: true,
            orthographic: false,
            patch_requests: 0,
            applied: None,
        }
    }
}

impl Settings {
    pub fn handle_action(&mut self, action: Action) -> Option<&'static str> {
        match action {
            Action::Studies => {
                self.studies = !self.studies;
                Some(if self.studies {
                    "Renderer study panels shown [F4]."
                } else {
                    "Renderer study panels hidden [F4]."
                })
            }
            Action::Lighting => {
                self.lighting = !self.lighting;
                Some(if self.lighting {
                    "Lambert lighting enabled [L]."
                } else {
                    "Unlit surfaces [L]."
                })
            }
            Action::Fog => {
                self.fog = !self.fog;
                Some(if self.fog {
                    "Distance fog enabled [F]; it does not cull geometry."
                } else {
                    "Distance fog disabled [F]."
                })
            }
            Action::Mipmaps => {
                self.mipmaps = !self.mipmaps;
                Some(if self.mipmaps {
                    "Complete texture mip chains enabled [M]."
                } else {
                    "Texture mip zero only [M]."
                })
            }
            Action::TexturePatch => {
                self.studies = true;
                self.patch_requests = self.patch_requests.saturating_add(1);
                Some("Patched the gray study board [T]; shared stone terrain stays unchanged.")
            }
            Action::Projection => {
                self.orthographic = !self.orthographic;
                Some(if self.orthographic {
                    "Orthographic inspection [V]. Center ray still controls block picking."
                } else {
                    "Perspective camera [V]."
                })
            }
            _ => None,
        }
    }
}

pub fn surface(block: Block, shade: u8, settings: &Settings) -> LogicResult<SurfaceStyle3d> {
    // Retain mild face tint for the Unlit comparison. Real illumination comes
    // from normals + the scene's directional light, not fake per-frame colors.
    let color = block.color(shade);
    let surface = if block.translucent() {
        SurfaceStyle3d::blend(Color::rgba(color.red(), color.green(), color.blue(), 0.38))?
    } else if block == Block::Leaves {
        SurfaceStyle3d::mask(color, 0.5)?
    } else {
        SurfaceStyle3d::opaque(color)?
    };
    Ok(surface
        .with_sidedness(SurfaceSidedness3d::FrontOnly)
        .with_lighting(if settings.lighting && block != Block::Lamp {
            SurfaceLighting3d::Lambert
        } else {
            SurfaceLighting3d::Unlit
        })
        .with_fog(true))
}

pub fn environment(region: RegionId) -> LogicResult<(Lighting3d, Fog3d)> {
    let (ambient, sun, direction, fog_color, density) = match region {
        RegionId::Meadow => (
            Color::rgb(0.72, 0.86, 1.0),
            Color::rgb(1.0, 0.97, 0.86),
            Vec3::new(-0.4, 1.0, 0.6)?,
            Color::rgb8(119, 178, 221),
            0.025,
        ),
        RegionId::Canyon => (
            Color::rgb(1.0, 0.72, 0.56),
            Color::rgb(1.0, 0.83, 0.62),
            Vec3::new(0.7, 1.0, -0.3)?,
            Color::rgb8(185, 136, 116),
            0.055,
        ),
    };
    Ok((
        Lighting3d::new(AmbientLight3d::new(ambient, 0.4)?)
            .with_directional(Some(DirectionalLight3d::new(direction, sun, 0.75)?)),
        Fog3d::new(fog_color, 9.0, density)?,
    ))
}

pub fn apply(
    local: Res<Local>,
    mut settings: ResMut<Settings>,
    mut visuals: Query<(Option<&Showcase>, Option<&ChunkPart>, &mut MeshVisual3d)>,
) -> LogicResult {
    let current = (settings.lighting, settings.mipmaps);
    if local.phase != Phase::Ready || settings.applied == Some(current) {
        return Ok(());
    }
    for (showcase, part, mut visual) in &mut visuals {
        let illumination = if settings.lighting
            && showcase != Some(&Showcase::PaintedBoard)
            && part.is_none_or(|part| part.block != Block::Lamp)
        {
            SurfaceLighting3d::Lambert
        } else {
            SurfaceLighting3d::Unlit
        };
        let surface = visual.surface().with_lighting(illumination);
        visual.set_surface(surface)?;
        if let Some(texture) = visual.texture() {
            let texture = texture.clone().with_mipmaps(settings.mipmaps);
            visual.set_texture(Some(texture))?;
        }
    }
    settings.applied = Some(current);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_tiles_preserve_alpha_and_complete_mip_budgets() -> LogicResult {
        let palette = Palette::new()?;
        for (index, block) in Block::SOLID.into_iter().enumerate() {
            let texture = palette.texture(block, true)?;
            assert_eq!(texture.asset().source_bytes(), 1024);
            assert_eq!(texture.texel_bytes()?, 1024 + 256 + 64 + 16 + 4);
            assert_eq!(texture.address_mode(), TextureAddressMode3d::Repeat);
            assert!(texture.asset().shares_storage(palette.0[index].asset()));
            assert_eq!(
                texture
                    .asset()
                    .pixels()
                    .chunks_exact(4)
                    .any(|pixel| pixel[3] == 0),
                block == Block::Leaves
            );
            for other in palette.0.iter().take(index) {
                assert!(!texture.asset().shares_storage(other.asset()));
            }
        }
        let (meadow, meadow_fog) = environment(RegionId::Meadow)?;
        let (canyon, canyon_fog) = environment(RegionId::Canyon)?;
        assert_ne!(meadow, canyon);
        assert_ne!(meadow_fog, canyon_fog);
        Ok(())
    }
}

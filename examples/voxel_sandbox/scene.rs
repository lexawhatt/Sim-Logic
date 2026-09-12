//! Factories build only a valid loading screen; saved terrain is projected later.

use super::{
    materials::{self, Palette, Settings},
    model::{Movement, RegionId},
    showcase::{self, Showcase},
    view::{self, Button, Label, Layout, Menu, Panel},
};
use sim_logic::{prelude::*, three_d::ThreeDSurfacePolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Queued,
    Ready,
}

#[derive(Resource)]
pub struct Local {
    pub region: RegionId,
    pub generation: WorldGeneration,
    pub phase: Phase,
    pub pointer: PointerButton<Button>,
    pub movement: Movement,
    pub look: [f32; 2],
    pub last_pointer: Option<PointerSample>,
    pub middle_drag: bool,
    pub departing: bool,
    pub paused: bool,
    pub pause_pending: bool,
    pub menu: Menu,
    pub debug: bool,
    pub flying: bool,
    pub flight_vertical: f32,
    pub input_time: std::time::Duration,
    pub last_jump: Option<std::time::Duration>,
    pub projected_epoch: u64,
    pub chunk_revisions: Vec<(usize, u64)>,
    pub rebuilt_chunks: u64,
}

#[derive(Component)]
pub struct SelectionEdge(pub usize);

pub struct Recipe {
    region: RegionId,
    panels: Vec<(Panel, ScreenRectangleVisual)>,
    labels: Vec<(Label, ScreenTextVisual)>,
    camera: ActiveCamera2d,
    view: View3d,
    outline: CuboidVisual3d,
    studies: Vec<(Showcase, MeshVisual3d)>,
}

impl Recipe {
    pub fn new(region: RegionId, font: &TextFont, palette: &Palette) -> LogicResult<Self> {
        let layout = Layout::new(LogicalViewport::new(1100.0, 720.0)?);
        let panels = view::panels(layout)?;
        let labels = view::labels(font, layout)?;
        let mut view = View3d::new(Vec3::new(12.5, 7.0, 12.5)?, Vec3::new(12.5, 5.0, 8.5)?)?;
        // Filled voxel surfaces need ordinary hardware clipping for arbitrary
        // player poses. The library's strict scientific default is unchanged.
        view.set_surface_policy(ThreeDSurfacePolicy::Native);
        view.set_background(match region {
            RegionId::Meadow => Color::rgb8(119, 178, 221),
            RegionId::Canyon => Color::rgb8(185, 136, 116),
        })?;
        view.set_enabled(false);
        let (lighting, fog) = materials::environment(region)?;
        view.set_lighting(lighting);
        view.set_fog(Some(fog));
        let mut outline = CuboidVisual3d::new(
            Vec3::ZERO,
            Vec3::new(1.0, 0.012, 0.012)?,
            Color::rgb8(255, 221, 82),
        )?;
        outline.set_visible(false);
        Ok(Self {
            region,
            panels,
            labels,
            camera: ActiveCamera2d::centered(1.0)?,
            view,
            outline,
            studies: showcase::build(palette)?,
        })
    }

    pub fn spawn(&self, world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
        let camera = world.spawn(self.camera)?;
        world.insert_resource(self.view)?;
        world.insert_resource(super::presentation::HudCache::default())?;
        world.insert_resource(Settings::default())?;
        world.insert_resource(showcase::PatchState::default())?;
        for (kind, visual) in &self.studies {
            world.spawn((*kind, visual.clone()))?;
        }
        for (panel, visual) in &self.panels {
            world.spawn((*panel, *visual))?;
        }
        for (label, visual) in &self.labels {
            world.spawn((*label, visual.clone()))?;
        }
        for index in 0..12 {
            world.spawn((SelectionEdge(index), self.outline))?;
        }
        world.insert_resource(Local {
            region: self.region,
            generation: camera.world_generation(),
            phase: Phase::Loading,
            pointer: PointerButton::new(MouseButton::Left),
            movement: Movement::default(),
            look: [0.0; 2],
            last_pointer: None,
            middle_drag: false,
            departing: false,
            paused: false,
            pause_pending: false,
            menu: Menu::None,
            debug: false,
            flying: false,
            flight_vertical: 0.0,
            input_time: std::time::Duration::ZERO,
            last_jump: None,
            projected_epoch: 0,
            chunk_revisions: Vec::new(),
            rebuilt_chunks: 0,
        })?;
        Ok(())
    }
}

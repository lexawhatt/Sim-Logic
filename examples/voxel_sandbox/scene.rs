//! Factories build only a valid loading screen; saved terrain is projected later.

use super::{
    model::{Movement, RegionId},
    view::{self, Button, Label, Layout, Panel},
};
use sim_logic::prelude::*;

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
    pub projected_epoch: u64,
    pub chunk_revisions: [u64; super::model::CHUNK_COUNT],
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
}

impl Recipe {
    pub fn new(region: RegionId, font: &TextFont) -> LogicResult<Self> {
        let layout = Layout::new(LogicalViewport::new(1100.0, 720.0)?);
        let mut panels = Vec::new();
        for panel in [
            Panel::Header,
            Panel::Footer,
            Panel::CrossHorizontal,
            Panel::CrossVertical,
        ]
        .into_iter()
        .chain(view::BUTTONS.map(Panel::Button))
        {
            panels.push((
                panel,
                view::rectangle(panel, layout, Color::rgb8(22, 31, 43))?,
            ));
        }
        let mut labels = Vec::new();
        for (label, caption) in [
            (Label::Title, "TWIN FIELDS / Sim;Logic"),
            (Label::Status, "Preparing saved region..."),
            (
                Label::Help,
                "WASD move / Arrows or middle-drag look / Space jump / LMB break / RMB build",
            ),
            (Label::Notice, "Loading terrain..."),
            (Label::Target, ""),
        ]
        .into_iter()
        .chain(view::BUTTONS.map(|button| (Label::Button(button), "...")))
        {
            labels.push((label, view::text(font, label, caption, layout)?));
        }
        let mut view = View3d::new(Vec3::new(12.5, 7.0, 12.5)?, Vec3::new(12.5, 5.0, 8.5)?)?;
        view.set_background(match region {
            RegionId::Meadow => Color::rgb8(119, 178, 221),
            RegionId::Canyon => Color::rgb8(185, 136, 116),
        })?;
        view.set_enabled(false);
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
        })
    }

    pub fn spawn(&self, world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
        let camera = world.spawn(self.camera)?;
        world.insert_resource(self.view)?;
        world.insert_resource(super::presentation::HudCache::default())?;
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
            projected_epoch: 0,
            chunk_revisions: [0; super::model::CHUNK_COUNT],
            rebuilt_chunks: 0,
        })?;
        Ok(())
    }
}

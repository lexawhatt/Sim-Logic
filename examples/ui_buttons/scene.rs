//! Immutable World recipes and the board's local interaction state.

use std::time::Duration;

use sim_logic::prelude::*;

use super::view;

/// Nonoverlapping, visible button roles; identity is the owning LogicEntity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Button {
    /// Adds one persistent UI click on a successful release.
    Count,
    /// Pauses or resumes fixed ticks and background placement.
    Pause,
    /// Changes COUNT eligibility without removing its visible disabled label.
    ToggleCount,
    /// Replaces the active World with the other factory.
    NextWorld,
}

impl Button {
    pub(super) fn eligible(self, count_enabled: bool) -> bool {
        self != Self::Count || count_enabled
    }
}

/// Scene-local markers disappear when a replacement commits.
#[derive(Component)]
pub struct Marker;

/// Local state and generation-qualified capture are reconstructed on replacement.
#[derive(Resource)]
pub struct Board {
    /// Zero for the garden factory, one for the harbor factory.
    pub world_index: usize,
    /// Fixed ticks in this World, frozen during pause.
    pub ticks: u32,
    /// Exact fixed simulation duration, displayed only at whole-second changes.
    pub elapsed: Duration,
    /// Host UI eligibility, independent of presentation visibility.
    pub count_enabled: bool,
    /// Stable managed identity of this World's COUNT button.
    pub count_target: LogicEntity,
    /// The only controller fed by the central FrameUpdate loop.
    pub pointer: PointerButton<LogicEntity>,
}

pub(super) fn accent(index: usize) -> Color {
    if index == 0 {
        Color::rgb8(87, 220, 156)
    } else {
        Color::rgb8(88, 177, 255)
    }
}

pub(super) fn background_contains(pointer: PointerSample) -> bool {
    let position = pointer.position().to_vec2();
    position.x() >= 0.0
        && position.x() < pointer.viewport().width()
        && position.y() >= 430.0
        && position.y() < pointer.viewport().height()
}

pub(super) struct Scene {
    index: usize,
    camera: ActiveCamera2d,
    background: WorldBackground,
    labels: Vec<(view::Label, ScreenTextVisual)>,
    panel: ScreenRectangleVisual,
    buttons: [(Button, ScreenRectangleVisual); 4],
    indicators: [(view::Indicator, ScreenRectangleVisual); 2],
}

impl Scene {
    pub(super) fn new(index: usize, body: &TextFont, title: &TextFont) -> LogicResult<Self> {
        Ok(Self {
            index,
            camera: ActiveCamera2d::centered(20.0)?,
            background: WorldBackground::new(if index == 0 {
                Color::rgb8(12, 29, 26)
            } else {
                Color::rgb8(12, 21, 37)
            })?,
            labels: view::labels(index, body, title)?,
            panel: view::panel()?,
            buttons: view::buttons()?,
            indicators: view::indicators()?,
        })
    }

    pub(super) fn spawn(&self, world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
        world.spawn(self.camera)?;
        world.insert_resource(self.background)?;
        world.spawn(self.panel)?;
        for (kind, label) in &self.labels {
            world.spawn((*kind, label.clone()))?;
        }
        let targets = world.spawn_array(self.buttons)?;
        world.spawn_array(self.indicators)?;
        world.insert_resource(Board {
            world_index: self.index,
            ticks: 0,
            elapsed: Duration::ZERO,
            count_enabled: true,
            count_target: targets[0],
            pointer: PointerButton::new(MouseButton::Left),
        })?;
        Ok(())
    }
}

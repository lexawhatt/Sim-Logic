//! One input owner routes UI and game actions; fixed ticks consume accepted motion.

use super::{
    materials::Palette,
    model::{RegionId, SaveGame},
    projection,
    scene::{Local, Phase, Recipe, SelectionEdge},
    view::{self, Label, Panel},
};
use sim_logic::prelude::*;
use std::path::PathBuf;

#[path = "app/routing.rs"]
mod routing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Forward,
    Backward,
    Left,
    Right,
    LookLeft,
    LookRight,
    LookUp,
    LookDown,
    Jump,
    Descend,
    Break,
    Place,
    LookDrag,
    Travel,
    Pause,
    Save,
    Load,
    Exit,
    Lighting,
    Fog,
    Mipmaps,
    TexturePatch,
    Projection,
    Creative,
    Debug,
    Studies,
    Slot(usize),
}

pub struct PendingLoad {
    pub source: WorldGeneration,
    pub game: SaveGame,
}

pub struct Session {
    pub game: SaveGame,
    pub epoch: u64,
    pub notice: String,
    pub notice_revision: u64,
    pub edits: u64,
    pub ticks: u64,
    pub save_path: PathBuf,
    pub pending_load: Option<PendingLoad>,
}

impl Session {
    /// The same message may be a new occurrence (for example, saving twice).
    pub fn notify(&mut self, notice: impl Into<String>) {
        self.notice = notice.into();
        self.notice_revision = self.notice_revision.wrapping_add(1);
    }
}

pub(crate) struct Routes(pub [WorldFactoryId; 2]);

#[derive(Clone, Copy)]
pub(super) struct Edited;

fn count_edits(events: EventReader<Edited>, mut session: AppResMut<Session>) {
    session.edits = session.edits.saturating_add(events.iter().count() as u64);
}

fn fixed(
    input: FixedInput<Action>,
    time: FixedTime,
    mut local: ResMut<Local>,
    mut session: AppResMut<Session>,
) {
    if local.phase != Phase::Ready || local.departing || local.menu != view::Menu::None {
        return;
    }
    if input.focus_lost() {
        local.movement = Default::default();
        local.flight_vertical = 0.0;
        local.look = [0.0; 2];
        session.game.player.stop();
        return;
    }
    let seconds = time.delta().as_secs_f32();
    session
        .game
        .player
        .turn(local.look[0] * seconds, local.look[1] * seconds);
    if local.flying && session.game.creative {
        session
            .game
            .fly_step(local.movement, local.flight_vertical, seconds);
    } else {
        local.flying = false;
        session.game.step(local.movement, seconds);
    }
    local.movement.jump = false;
    session.ticks = session.ticks.saturating_add(1);
}

pub fn build_application(
    time: TimeConfig,
    save_path: PathBuf,
) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    config.set_entity_limit(6000)?;
    config.set_command_limit(8192)?;
    config.set_input_event_limit(128)?;
    config.set_event_limit(128)?;
    config.set_render_limits(view::limits());
    config.set_text_limits(TextLimits::new(3, view::FONT.len() * 3));
    // Streaming keeps at most 50 chunks. Every chunk can have one mesh per
    // block/shade; pathological edited checkerboards still have finite budgets.
    config.set_three_d_render_limits(
        ThreeDRenderLimits::new(12, 330_000, 8_388_608)
            .with_mesh_limits(5000, 128 * 1024 * 1024)
            .with_texture_limits(8 * 1024 * 1024, 8 * 1024 * 1024),
    );
    let mut app = Application::new(config)?;
    app.approve_components::<(
        Panel,
        Label,
        SelectionEdge,
        projection::ChunkPart,
        projection::ChunkStamp,
        super::showcase::Showcase,
    )>()?;
    app.register_app_resource(Session {
        game: SaveGame::new(0x51_4d_10),
        epoch: 1,
        notice: "Click to capture mouse. E / F5 inventory; F3 debug; Escape menu.".into(),
        notice_revision: 0,
        edits: 0,
        ticks: 0,
        save_path,
        pending_load: None,
    })?;
    app.register_app_resource(PointerCapture::default())?;
    let font = app.register_font(view::FONT.to_vec(), TextSettings::new(17.0)?)?;
    let small = app.register_font(view::FONT.to_vec(), TextSettings::new(12.0)?)?;
    let tiny = app.register_font(view::FONT.to_vec(), TextSettings::new(9.0)?)?;
    app.register_app_resource(view::Fonts([font.clone(), small, tiny]))?;
    let [meadow_id, canyon_id] = RegionId::ALL;
    let palette = Palette::new()?;
    let meadow = Recipe::new(meadow_id, &font, &palette)?;
    let canyon = Recipe::new(canyon_id, &font, &palette)?;
    app.register_app_resource(palette)?;
    let initial = app.register_world("voxel-meadow", move |world| meadow.spawn(world))?;
    let alternate = app.register_world("voxel-canyon", move |world| canyon.spawn(world))?;
    app.register_app_resource(Routes([initial, alternate]))?;
    for (key, action) in [
        (PhysicalKeyCode::KeyW, Action::Forward),
        (PhysicalKeyCode::KeyS, Action::Backward),
        (PhysicalKeyCode::KeyA, Action::Left),
        (PhysicalKeyCode::KeyD, Action::Right),
        (PhysicalKeyCode::ArrowLeft, Action::LookLeft),
        (PhysicalKeyCode::ArrowRight, Action::LookRight),
        (PhysicalKeyCode::ArrowUp, Action::LookUp),
        (PhysicalKeyCode::ArrowDown, Action::LookDown),
        (PhysicalKeyCode::Space, Action::Jump),
        (PhysicalKeyCode::ShiftLeft, Action::Descend),
        (PhysicalKeyCode::KeyP, Action::Pause),
        (PhysicalKeyCode::KeyN, Action::Travel),
        (PhysicalKeyCode::F5, Action::Creative),
        (PhysicalKeyCode::KeyE, Action::Creative),
        (PhysicalKeyCode::F7, Action::Save),
        (PhysicalKeyCode::F3, Action::Debug),
        (PhysicalKeyCode::F4, Action::Studies),
        (PhysicalKeyCode::F9, Action::Load),
        (PhysicalKeyCode::Escape, Action::Exit),
        (PhysicalKeyCode::KeyL, Action::Lighting),
        (PhysicalKeyCode::KeyF, Action::Fog),
        (PhysicalKeyCode::KeyM, Action::Mipmaps),
        (PhysicalKeyCode::KeyT, Action::TexturePatch),
        (PhysicalKeyCode::KeyV, Action::Projection),
        (PhysicalKeyCode::Digit1, Action::Slot(0)),
        (PhysicalKeyCode::Digit2, Action::Slot(1)),
        (PhysicalKeyCode::Digit3, Action::Slot(2)),
        (PhysicalKeyCode::Digit4, Action::Slot(3)),
        (PhysicalKeyCode::Digit5, Action::Slot(4)),
        (PhysicalKeyCode::Digit6, Action::Slot(5)),
        (PhysicalKeyCode::Digit7, Action::Slot(6)),
        (PhysicalKeyCode::Digit8, Action::Slot(7)),
        (PhysicalKeyCode::Digit9, Action::Slot(8)),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, Action::Break)?;
    app.bind_mouse_button(MouseButton::Right, Action::Place)?;
    app.bind_mouse_button(MouseButton::Middle, Action::LookDrag)?;
    app.add_fixed_system(fixed);
    app.add_fallible_frame_system(routing::route);
    app.add_frame_system(count_edits);
    app.add_fallible_frame_system(projection::project);
    app.add_fallible_frame_system(super::materials::apply);
    app.add_fallible_frame_system(super::showcase::update);
    app.add_fallible_frame_system(super::presentation::present);
    Ok((app, initial))
}

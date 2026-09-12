//! Input and runtime wiring. Economic and combat rules are not UI code.

use std::time::Duration;

use sim_logic::prelude::*;

mod interaction;

use super::{
    drawing,
    navigation::{MapDrag, MapView},
    simulation::{Game, NEUTRAL, Phase, WATER},
    view,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Click,
    Pan,
    ResetView,
    Expand,
    Pause,
    Restart,
    NewMap,
    Less,
    More,
    Ten,
    Quarter,
    Half,
    ThreeQuarters,
    All,
    Debug,
    Cheats,
    Grant,
    Bots,
    Win,
    Step,
    Exit,
}

pub struct Session {
    pub game: Game,
    pub percent: u8,
    pub paused: bool,
    pub debug: bool,
    pub cheats: bool,
    pub assisted: bool,
    pub hover: Option<usize>,
    pub notice: &'static str,
    pub notice_seconds: f32,
    pub frame_ms: f32,
    pub map_view: MapView,
    map_drag: MapDrag,
    middle_held: bool,
    slider_drag: bool,
}

impl Session {
    fn new(seed: u64) -> Self {
        Self {
            game: Game::new(seed),
            percent: 25,
            paused: false,
            debug: false,
            cheats: false,
            assisted: false,
            hover: None,
            notice: "CLICK LAND TO CHOOSE YOUR START",
            notice_seconds: 5.0,
            frame_ms: 0.0,
            map_view: MapView::default(),
            map_drag: MapDrag::default(),
            middle_held: false,
            slider_drag: false,
        }
    }

    fn notify(&mut self, message: &'static str) {
        self.notice = message;
        self.notice_seconds = 3.0;
    }

    fn order(&mut self, target: u8) {
        if self.paused || self.game.phase() != Phase::Running {
            return;
        }
        if target == WATER || target == 0 {
            self.notify("SELECT NEUTRAL LAND OR A BORDERING RIVAL");
            return;
        }
        let result = if target == NEUTRAL {
            self.game.expand(self.percent)
        } else {
            self.game.order(0, target, self.percent)
        };
        self.notify(match result {
            Ok(()) => "EXPEDITION SENT - KEEP SOME TROOPS AT HOME",
            Err(super::simulation::OrderError::NoBorder) => {
                "NO SHARED BORDER - EXPAND TOWARD THIS TARGET"
            }
            Err(super::simulation::OrderError::CampaignActive) => {
                "YOUR EXPEDITION IS STILL MARCHING"
            }
            Err(super::simulation::OrderError::InsufficientTroops) => {
                "NOT ENOUGH TROOPS - LET YOUR RESERVE GROW"
            }
            Err(_) => "ORDER NOT AVAILABLE",
        });
    }
}

fn simulate(mut session: AppResMut<Session>) {
    // Also guard locally for manual stepping and same-frame UI policies.
    if !session.paused {
        session.game.tick();
    }
}

pub fn build_application(seed: u64) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    build_with_session(Session::new(seed))
}

/// Prepared early battle for quick demos. No automation continues in the window.
pub fn build_demo(seed: u64, debug: bool) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut session = Session::new(seed);
    let home = session
        .game
        .owners()
        .iter()
        .enumerate()
        .filter(|(_, owner)| **owner == NEUTRAL)
        .min_by_key(|(cell, _)| {
            (cell % super::simulation::WIDTH).abs_diff(super::simulation::WIDTH / 2)
                + (cell / super::simulation::WIDTH).abs_diff(super::simulation::HEIGHT / 2)
        })
        .map(|(cell, _)| cell)
        .ok_or("generated map has no starting land")?;
    session.game.start(home);
    for tick in 0..240 {
        if tick % 15 == 0 {
            let _ = session.game.expand(25);
        }
        session.game.tick();
    }
    session.debug = debug;
    session.notify("DEMO POSITION - YOU CONTROL THE MINT TERRITORY");
    build_with_session(session)
}

fn build_with_session(session: Session) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(100), 8)?);
    config.set_entity_limit(drawing::MAX_RECTS + drawing::MAX_GLYPHS + 1)?;
    config.set_command_limit(drawing::MAX_RECTS + drawing::MAX_GLYPHS + 4)?;
    config.set_render_limits(view::limits());
    config.set_image_asset_limits(ImageAssetLimits::new(
        1,
        drawing::FONT_WIDTH,
        drawing::FONT_HEIGHT,
        drawing::FONT_BYTES,
    ));
    let mut app = Application::new(config)?;
    app.register_app_resource(session)?;
    let font = drawing::register_font(&mut app)?;
    app.approve_components::<(view::RectSlot, view::GlyphSlot)>()?;
    for (key, action) in [
        (PhysicalKeyCode::Escape, Action::Exit),
        (PhysicalKeyCode::Space, Action::Expand),
        (PhysicalKeyCode::KeyP, Action::Pause),
        (PhysicalKeyCode::KeyR, Action::Restart),
        (PhysicalKeyCode::KeyN, Action::NewMap),
        (PhysicalKeyCode::KeyV, Action::ResetView),
        (PhysicalKeyCode::ArrowLeft, Action::Less),
        (PhysicalKeyCode::ArrowRight, Action::More),
        (PhysicalKeyCode::Digit1, Action::Ten),
        (PhysicalKeyCode::Digit2, Action::Quarter),
        (PhysicalKeyCode::Digit3, Action::Half),
        (PhysicalKeyCode::Digit4, Action::ThreeQuarters),
        (PhysicalKeyCode::Digit5, Action::All),
        (PhysicalKeyCode::F3, Action::Debug),
        (PhysicalKeyCode::F4, Action::Cheats),
        (PhysicalKeyCode::F5, Action::Grant),
        (PhysicalKeyCode::F6, Action::Bots),
        (PhysicalKeyCode::F8, Action::Win),
        (PhysicalKeyCode::F9, Action::Step),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, Action::Click)?;
    app.bind_mouse_button(MouseButton::Middle, Action::Pan)?;
    app.add_fallible_frame_system(interaction::interact);
    app.add_fixed_system(simulate);
    app.add_fallible_frame_system(view::draw);
    let camera = ActiveCamera2d::centered(1.0)?;
    let world = app.register_world("frontier", move |world| {
        world.spawn(camera)?;
        world.insert_resource(
            WorldBackground::new(Color::rgb8(10, 15, 22))
                .map_err(|e| WorldBuildError::user(e.to_string()))?,
        )?;
        view::spawn(world, font)
    })?;
    Ok((app, world))
}

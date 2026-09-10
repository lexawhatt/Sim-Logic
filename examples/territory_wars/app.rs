//! Input and runtime wiring. Economic and combat rules are not UI code.

use std::time::Duration;

use sim_logic::prelude::*;

use super::{
    drawing,
    layout::{self, Layout},
    simulation::{Game, NEUTRAL, Phase, WATER},
    view,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Click,
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

fn interact(
    input: FrameInput<Action>,
    time: FrameTime,
    mut session: AppResMut<Session>,
    mut commands: Commands,
) -> LogicResult {
    session.notice_seconds = (session.notice_seconds - time.seconds_f32()).max(0.0);
    session.frame_ms = time.seconds_f32() * 1000.0;
    if input.has_press_occurrence(Action::Exit) {
        commands.request_exit()?;
        return Ok(());
    }
    // Restart/seed changes own this frame; stale simultaneous clicks never
    // choose a start or spend troops in the newly generated map.
    let click_new_map = input.pressed(Action::Click).any(|edge| {
        edge.pointer().is_some_and(|p| {
            let (x, y) = Layout::new(p.viewport()).unproject(p.position());
            layout::NEW_MAP.contains(x, y)
        })
    });
    let new_map = input.has_press_occurrence(Action::NewMap) || click_new_map;
    if new_map || input.has_press_occurrence(Action::Restart) {
        let seed = if new_map {
            session.game.seed().wrapping_add(1)
        } else {
            session.game.seed()
        };
        *session = Session::new(seed);
        commands.set_paused(false)?;
        return Ok(());
    }
    if input.has_press_occurrence(Action::Debug) {
        session.debug = !session.debug;
    }
    if input.has_press_occurrence(Action::Cheats) {
        session.cheats = !session.cheats;
        session.debug = true;
        let notice = if session.cheats {
            "CHEATS ARMED - F5 TROOPS / F6 AI ORDERS / F8 WIN"
        } else {
            "CHEATS DISARMED - ASSISTED MARK IS RETAINED"
        };
        session.notify(notice);
    }
    if session.cheats && session.game.phase() == Phase::Running {
        if input.has_press_occurrence(Action::Grant) {
            session.game.grant_troops(0, 50_000);
            session.assisted = true;
            session.notify("CHEAT: TROOPS FILLED UP TO LAND CAPACITY");
        }
        if input.has_press_occurrence(Action::Bots) {
            let enabled = !session.game.bots_enabled();
            session.game.set_bots_enabled(enabled);
            session.assisted = true;
            session.notify("CHEAT: AI ORDERS TOGGLED - EXISTING ARMIES STILL MOVE");
        }
        if input.has_press_occurrence(Action::Win) {
            session.game.force_player_victory();
            session.assisted = true;
            session.notify("CHEAT: VICTORY - THIS RUN IS ASSISTED");
        }
    }
    let click_pause = input.pressed(Action::Click).any(|edge| {
        edge.pointer().is_some_and(|p| {
            let (x, y) = Layout::new(p.viewport()).unproject(p.position());
            layout::PAUSE.contains(x, y)
        })
    });
    if input.has_press_occurrence(Action::Pause) || click_pause {
        session.paused = !session.paused;
        session.slider_drag = false;
        commands.set_paused(session.paused)?;
        return Ok(());
    }
    if session.paused && input.has_press_occurrence(Action::Step) {
        session.game.tick();
    }
    for (action, percent) in [
        (Action::Ten, 10),
        (Action::Quarter, 25),
        (Action::Half, 50),
        (Action::ThreeQuarters, 75),
        (Action::All, 100),
    ] {
        if input.has_press_occurrence(action) {
            session.percent = percent;
        }
    }
    if input.has_press_occurrence(Action::Less) {
        session.percent = session.percent.saturating_sub(5).max(5);
    }
    if input.has_press_occurrence(Action::More) {
        session.percent = session.percent.saturating_add(5).min(100);
    }
    if input.has_press_occurrence(Action::Expand) {
        session.order(NEUTRAL);
    }

    // Every edge carries the viewport and cursor of its occurrence, including
    // a quick complete drag inside one frame. A missing release position is a
    // cancellation, never a click at the previous cursor location.
    for edge in input.edges() {
        if edge.action() != Action::Click {
            continue;
        }
        if edge.state() == ButtonState::Released {
            if session.slider_drag
                && let Some(pointer) = edge.pointer()
            {
                let (x, _) = Layout::new(pointer.viewport()).unproject(pointer.position());
                session.percent = layout::attack_percent(x);
            }
            session.slider_drag = false;
            continue;
        }
        let Some(pointer) = edge.pointer() else {
            continue;
        };
        let (x, y) = Layout::new(pointer.viewport()).unproject(pointer.position());
        if layout::DEBUG.contains(x, y) {
            session.debug = !session.debug;
        } else if layout::SLIDER.contains(x, y) {
            session.slider_drag = true;
            session.percent = layout::attack_percent(x);
        } else if layout::EXPAND.contains(x, y) {
            session.order(NEUTRAL);
        } else if let Some(cell) = layout::cell_at(x, y) {
            if session.game.phase() == Phase::Choosing {
                if session.game.start(cell) {
                    session.notify("GROW YOUR RESERVE. CLICK NEUTRAL LAND TO EXPAND.");
                } else {
                    session.notify("CHOOSE A LAND TILE, NOT THE SEA");
                }
            } else {
                let target = session.game.owners()[cell];
                session.order(target);
            }
        }
    }
    session.hover = input.pointer().and_then(|p| {
        let (x, y) = Layout::new(p.viewport()).unproject(p.position());
        layout::cell_at(x, y)
    });
    if input.pointer().is_none() || !input.held(Action::Click) {
        session.slider_drag = false;
    } else if session.slider_drag
        && let Some(pointer) = input.pointer()
    {
        let (x, _) = Layout::new(pointer.viewport()).unproject(pointer.position());
        session.percent = layout::attack_percent(x);
    }
    Ok(())
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
    app.add_fallible_frame_system(interact);
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

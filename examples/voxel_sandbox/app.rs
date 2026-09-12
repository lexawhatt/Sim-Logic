//! One input owner routes UI and game actions; fixed ticks consume accepted motion.

use super::{
    model::{Block, Movement, RegionId, SaveGame},
    projection,
    scene::{Local, Phase, Recipe, SelectionEdge},
    view::{self, Button, Label, Panel},
};
use sim_logic::prelude::*;
use std::path::PathBuf;

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
    Break,
    Place,
    LookDrag,
    Travel,
    Pause,
    Save,
    Load,
    Exit,
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
    pub edits: u64,
    pub ticks: u64,
    pub save_path: PathBuf,
    pub pending_load: Option<PendingLoad>,
}

struct Routes([WorldFactoryId; 2]);

#[derive(Clone, Copy)]
struct Edited;

fn count_edits(events: EventReader<Edited>, mut session: AppResMut<Session>) {
    session.edits = session.edits.saturating_add(events.iter().count() as u64);
}

fn fixed(time: FixedTime, mut local: ResMut<Local>, mut session: AppResMut<Session>) {
    if local.phase != Phase::Ready || local.departing {
        return;
    }
    let seconds = time.delta().as_secs_f32();
    session
        .game
        .player
        .turn(local.look[0] * seconds, local.look[1] * seconds);
    session.game.step(local.movement, seconds);
    local.movement.jump = false;
    session.ticks = session.ticks.saturating_add(1);
}

fn stop(local: &mut Local, session: &mut Session) {
    local.movement = Movement::default();
    local.look = [0.0; 2];
    local.middle_drag = false;
    local.last_pointer = None;
    local.pointer.cancel();
    session.game.player.stop();
}

fn activate(
    button: Button,
    intent: TransitionIntentToken,
    paused: &mut bool,
    local: &mut Local,
    session: &mut Session,
    routes: &Routes,
    commands: &mut Commands,
) -> LogicResult {
    match button {
        Button::Slot(index) => {
            session.game.inventory.select(Block::SOLID[index]);
        }
        Button::Pause => {
            *paused = !*paused;
            stop(local, session);
            commands.set_paused(*paused)?;
        }
        Button::Travel => {
            stop(local, session);
            commands.replace_world(intent, routes.0[local.region.other().index()])?;
            local.departing = true;
        }
        Button::Save => {
            session.notice = match session.game.save(&session.save_path) {
                Ok(()) => "Saved both regions and inventory.".into(),
                Err(error) => format!("Save failed: {error}"),
            };
        }
        Button::Load => match SaveGame::load(&session.save_path) {
            Ok(game) => {
                let target = routes.0[game.active.index()];
                commands.replace_world(intent, target)?;
                session.pending_load = Some(PendingLoad {
                    source: local.generation,
                    game,
                });
                stop(local, session);
                local.departing = true;
            }
            Err(error) => {
                session.notice = format!("Load failed: {error}");
            }
        },
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one owner routes ordered UI and gameplay input"
)]
fn route(
    input: FrameInput<Action>,
    time: FrameTime,
    routes: AppRes<Routes>,
    mut local: ResMut<Local>,
    mut session: AppResMut<Session>,
    mut commands: Commands,
    mut edited: EventWriter<Edited>,
) -> LogicResult {
    // A failed replacement leaves this old World alive. Re-enable its router;
    // projection later discards any uninstalled pending load.
    local.departing = false;
    if session
        .pending_load
        .as_ref()
        .is_some_and(|pending| pending.source == local.generation)
    {
        session.pending_load = None;
        session.notice = "Load transition did not commit; current game kept.".into();
    }
    let mut paused = time.is_paused();
    let ready = local.phase == Phase::Ready;
    for edge in input.edges() {
        let hit = if ready && !local.departing {
            edge.pointer().and_then(view::hit)
        } else {
            None
        };
        let outcome = local.pointer.process(edge, hit);
        if local.departing {
            continue;
        }
        if edge.is_cancelled() {
            local.middle_drag = false;
            local.last_pointer = None;
            if edge.cancellation_reason() == Some(InputCancellationReason::FocusLost) {
                local.movement.jump = false;
            }
        }
        if let Some(PointerButtonEvent::Clicked { target, intent, .. }) = outcome.event() {
            activate(
                target,
                intent,
                &mut paused,
                &mut local,
                &mut session,
                &routes,
                &mut commands,
            )?;
        }
        if edge.state() != ButtonState::Pressed {
            continue;
        }
        if edge.action() == Action::Exit {
            stop(&mut local, &mut session);
            commands.request_exit()?;
            local.departing = true;
            continue;
        }
        if !ready {
            continue;
        }
        if edge.action() == Action::Jump && !paused {
            local.movement.jump = true;
        }
        let button = match edge.action() {
            Action::Travel => Some(Button::Travel),
            Action::Pause => Some(Button::Pause),
            Action::Save => Some(Button::Save),
            Action::Load => Some(Button::Load),
            Action::Slot(index) => Some(Button::Slot(index)),
            _ => None,
        };
        if let Some(button) = button {
            activate(
                button,
                edge.intent(),
                &mut paused,
                &mut local,
                &mut session,
                &routes,
                &mut commands,
            )?;
        } else if !paused && !outcome.claimed() && edge.pointer().is_some_and(view::game_area) {
            let result = match edge.action() {
                Action::Break => Some(session.game.break_target()),
                Action::Place => Some(session.game.place_target()),
                Action::LookDrag => {
                    local.middle_drag = true;
                    local.last_pointer = edge.pointer();
                    None
                }
                _ => None,
            };
            if let Some(result) = result {
                match result {
                    Ok(_) => {
                        session.notice = "".into();
                        edited.send(Edited)?;
                    }
                    Err(error) => session.notice = error.to_string(),
                }
            }
        }
    }
    local.paused = paused;
    if !ready || paused || local.departing {
        stop(&mut local, &mut session);
        return Ok(());
    }
    local.movement.forward =
        f32::from(input.held(Action::Forward)) - f32::from(input.held(Action::Backward));
    local.movement.strafe =
        f32::from(input.held(Action::Right)) - f32::from(input.held(Action::Left));
    // The ordered loop queues jump once for a later fixed tick. A later pause
    // or focus cancellation in the same batch can still invalidate it.
    local.look = [
        (f32::from(input.held(Action::LookRight)) - f32::from(input.held(Action::LookLeft))) * 1.8,
        (f32::from(input.held(Action::LookUp)) - f32::from(input.held(Action::LookDown))) * 1.5,
    ];
    if !input.held(Action::LookDrag) {
        local.middle_drag = false;
    }
    if local.middle_drag
        && let (Some(previous), Some(current)) = (local.last_pointer, input.pointer())
        && previous.viewport() == current.viewport()
    {
        let a = previous.position().to_vec2();
        let b = current.position().to_vec2();
        session
            .game
            .player
            .turn((b.x() - a.x()) * 0.004, (a.y() - b.y()) * 0.004);
    }
    local.last_pointer = input.pointer();
    Ok(())
}

pub fn build_application(
    time: TimeConfig,
    save_path: PathBuf,
) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    config.set_entity_limit(600)?;
    config.set_command_limit(1024)?;
    config.set_input_event_limit(128)?;
    config.set_event_limit(128)?;
    config.set_render_limits(view::limits());
    config.set_text_limits(TextLimits::new(3, view::FONT.len() * 3));
    // At most 18 chunks * 5 materials * 3 directional shades. Checkerboard
    // terrain can expose 6 faces for every solid cell; source/upload work is
    // bounded independently of the final screen composition.
    config.set_three_d_render_limits(
        ThreeDRenderLimits::new(12, 120_000, 8_388_608).with_mesh_limits(270, 32 * 1024 * 1024),
    );
    let mut app = Application::new(config)?;
    app.approve_components::<(
        Panel,
        Label,
        SelectionEdge,
        projection::ChunkPart,
        projection::ChunkStamp,
    )>()?;
    app.register_app_resource(Session {
        game: SaveGame::new(0x51_4d_10),
        epoch: 1,
        notice: "Build something here. N takes you to the other region.".into(),
        edits: 0,
        ticks: 0,
        save_path,
        pending_load: None,
    })?;
    let font = app.register_font(view::FONT.to_vec(), TextSettings::new(17.0)?)?;
    let small = app.register_font(view::FONT.to_vec(), TextSettings::new(12.0)?)?;
    let tiny = app.register_font(view::FONT.to_vec(), TextSettings::new(9.0)?)?;
    app.register_app_resource(view::Fonts([font.clone(), small, tiny]))?;
    let [meadow_id, canyon_id] = RegionId::ALL;
    let meadow = Recipe::new(meadow_id, &font)?;
    let canyon = Recipe::new(canyon_id, &font)?;
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
        (PhysicalKeyCode::KeyP, Action::Pause),
        (PhysicalKeyCode::KeyN, Action::Travel),
        (PhysicalKeyCode::F5, Action::Save),
        (PhysicalKeyCode::F9, Action::Load),
        (PhysicalKeyCode::Escape, Action::Exit),
        (PhysicalKeyCode::Digit1, Action::Slot(0)),
        (PhysicalKeyCode::Digit2, Action::Slot(1)),
        (PhysicalKeyCode::Digit3, Action::Slot(2)),
        (PhysicalKeyCode::Digit4, Action::Slot(3)),
        (PhysicalKeyCode::Digit5, Action::Slot(4)),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, Action::Break)?;
    app.bind_mouse_button(MouseButton::Right, Action::Place)?;
    app.bind_mouse_button(MouseButton::Middle, Action::LookDrag)?;
    app.add_fixed_system(fixed);
    app.add_fallible_frame_system(route);
    app.add_frame_system(count_edits);
    app.add_fallible_frame_system(projection::project);
    app.add_fallible_frame_system(super::presentation::present);
    Ok((app, initial))
}

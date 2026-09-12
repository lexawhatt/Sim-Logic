//! The game has one input owner. Menu clicks never also edit the terrain.

use super::{Action, Edited, PendingLoad, Routes, Session};
use crate::{
    materials::Settings,
    model::{Block, Movement, SaveGame},
    scene::{Local, Phase},
    view::{self, Button, Menu},
};
use sim_logic::prelude::*;

fn stop_gameplay(local: &mut Local, session: &mut Session) {
    local.movement = Movement::default();
    local.flight_vertical = 0.0;
    local.last_jump = None;
    local.look = [0.0; 2];
    local.middle_drag = false;
    local.last_pointer = None;
    session.game.player.stop();
}

/// Invalidate a gesture only when its UI/input owner actually changes.
fn stop(local: &mut Local, session: &mut Session) {
    stop_gameplay(local, session);
    local.pointer.cancel();
}

struct Router<'a, 'w> {
    local: &'a mut Local,
    session: &'a mut Session,
    capture: &'a mut PointerCapture,
    routes: &'a Routes,
    commands: &'a mut Commands<'w>,
    /// Aggregate raw deltas cannot be split around menu edges. Discard the
    /// entire frame's motion whenever ownership changes within that frame.
    motion_invalidated: bool,
}

impl Router<'_, '_> {
    fn menu(&mut self, menu: Menu) -> LogicResult {
        stop(self.local, self.session);
        self.local.menu = menu;
        self.local.paused = menu != Menu::None;
        self.local.pause_pending = true;
        self.commands.set_paused(self.local.paused)?;
        self.motion_invalidated = true;
        if self.local.paused {
            self.capture.release();
        } else {
            self.capture.request_lock();
        }
        Ok(())
    }

    fn activate(&mut self, button: Button, intent: TransitionIntentToken) -> LogicResult {
        match button {
            Button::Slot(index) => {
                self.session.game.inventory.select_slot(index);
            }
            Button::CreativeBlock(index) => {
                if self.local.menu == Menu::Creative
                    && let Some(&block) = Block::SOLID.get(index)
                {
                    let inventory = &mut self.session.game.inventory;
                    inventory.assign_slot(inventory.selected_slot(), block);
                    self.session.notify(format!(
                        "{} selected. E / F5 or Escape to return.",
                        block.name()
                    ));
                }
            }
            Button::Creative => self.menu(Menu::Creative)?,
            Button::Pause => self.menu(if self.local.menu == Menu::None {
                Menu::Pause
            } else {
                Menu::None
            })?,
            Button::Close => self.menu(Menu::None)?,
            Button::Exit => {
                stop(self.local, self.session);
                self.capture.release();
                self.commands.request_exit()?;
                self.local.departing = true;
            }
            Button::Travel => {
                stop(self.local, self.session);
                self.capture.release();
                self.commands
                    .replace_world(intent, self.routes.0[self.local.region.other().index()])?;
                self.local.departing = true;
            }
            Button::Save => {
                let notice = match self.session.game.save(&self.session.save_path) {
                    Ok(()) => "Saved both worlds, builds and hotbar.".to_owned(),
                    Err(error) => format!("Save failed: {error}"),
                };
                self.session.notify(notice);
            }
            Button::Load => match SaveGame::load(&self.session.save_path) {
                Ok(game) => {
                    self.commands
                        .replace_world(intent, self.routes.0[game.active.index()])?;
                    self.session.pending_load = Some(PendingLoad {
                        source: self.local.generation,
                        game,
                    });
                    stop(self.local, self.session);
                    self.capture.release();
                    self.local.departing = true;
                }
                Err(error) => self.session.notify(format!("Load failed: {error}")),
            },
        }
        Ok(())
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one ordered owner for gameplay and menus"
)]
pub(super) fn route(
    input: FrameInput<Action>,
    time: FrameTime,
    routes: AppRes<Routes>,
    mut local: ResMut<Local>,
    mut settings: ResMut<Settings>,
    mut session: AppResMut<Session>,
    mut capture: AppResMut<PointerCapture>,
    mut commands: Commands,
    mut edited: EventWriter<Edited>,
) -> LogicResult {
    local.departing = false;
    if session
        .pending_load
        .as_ref()
        .is_some_and(|pending| pending.source == local.generation)
    {
        session.pending_load = None;
        session.notify("Load transition did not commit; current game kept.");
    }
    // Menu intent is immediate local state, but the timing command commits
    // at the shared barrier. Retry it if another system rejected that batch.
    if local.pause_pending {
        local.paused = local.menu != Menu::None;
        if time.is_paused() == local.paused {
            local.pause_pending = false;
        } else {
            commands.set_paused(local.paused)?;
        }
    } else {
        local.paused = time.is_paused();
    }
    local.input_time = local.input_time.saturating_add(time.delta());
    if local.paused && local.menu == Menu::None {
        local.pointer.cancel();
        local.menu = Menu::Pause;
    }
    let ready = local.phase == Phase::Ready;
    let mut router = Router {
        local: &mut local,
        session: &mut session,
        capture: &mut capture,
        routes: &routes,
        commands: &mut commands,
        motion_invalidated: false,
    };
    if input.focus_lost() {
        router.menu(Menu::Pause)?;
        // Drain cancellation releases even though this frame cannot activate
        // anything. Otherwise the old hold suppresses the first fresh click.
        for edge in input.edges() {
            let _ = router.local.pointer.process(edge, None);
        }
        return Ok(());
    }
    for edge in input.edges() {
        let hit = if ready && !router.local.departing && !router.capture.requested() {
            edge.pointer()
                .and_then(|sample| view::hit(sample, router.local.menu))
        } else {
            None
        };
        let outcome = router.local.pointer.process(edge, hit);
        if router.local.departing {
            continue;
        }
        if edge.is_cancelled() {
            router.local.middle_drag = false;
            router.local.last_pointer = None;
            router.local.movement.jump = false;
            router.motion_invalidated = true;
        }
        if let Some(PointerButtonEvent::Clicked { target, intent, .. }) = outcome.event() {
            router.activate(target, intent)?;
        }
        if edge.state() != ButtonState::Pressed {
            continue;
        }
        match edge.action() {
            Action::Debug => {
                router.local.debug = !router.local.debug;
                continue;
            }
            Action::Exit | Action::Pause => {
                router.menu(if router.local.menu == Menu::None {
                    Menu::Pause
                } else {
                    Menu::None
                })?;
                continue;
            }
            Action::Creative if ready => {
                router.menu(if router.local.menu == Menu::Creative {
                    Menu::None
                } else {
                    Menu::Creative
                })?;
                continue;
            }
            _ => {}
        }
        if !ready {
            continue;
        }
        if let Some(notice) = settings.handle_action(edge.action()) {
            router.session.notify(notice);
            continue;
        }
        let button = match edge.action() {
            Action::Travel => Some(Button::Travel),
            Action::Save => Some(Button::Save),
            Action::Load => Some(Button::Load),
            Action::Slot(index) => Some(Button::Slot(index)),
            _ => None,
        };
        if let Some(button) = button {
            router.activate(button, edge.intent())?;
            continue;
        }
        if router.local.paused || router.local.menu != Menu::None || outcome.claimed() {
            continue;
        }
        if edge.action() == Action::Jump {
            let now = router.local.input_time;
            if router.session.game.creative
                && router.local.last_jump.is_some_and(|last| {
                    now.saturating_sub(last) <= std::time::Duration::from_millis(280)
                })
            {
                router.local.flying = !router.local.flying;
                router.local.movement.jump = false;
                router.local.last_jump = None;
                router.session.game.player.stop();
                router.session.notify(if router.local.flying {
                    "Flight enabled. Space up / Left Shift down; double Space to land."
                } else {
                    "Flight disabled."
                });
            } else {
                router.local.last_jump = Some(now);
                router.local.movement.jump = !router.local.flying;
            }
            continue;
        }
        let over_game = router.capture.requested()
            || edge
                .pointer()
                .is_some_and(|sample| view::game_area(sample, Menu::None));
        if !over_game {
            continue;
        }
        if !router.capture.requested() && matches!(edge.action(), Action::Break | Action::Place) {
            // Acquisition is not a shot. A fresh subsequent click can edit.
            router.capture.request_lock();
            router.motion_invalidated = true;
            router.session.notice.clear();
            continue;
        }
        if router.motion_invalidated {
            continue;
        }
        let result = match edge.action() {
            Action::Break => Some(router.session.game.break_target()),
            Action::Place => Some(router.session.game.place_target()),
            Action::LookDrag => {
                router.local.middle_drag = true;
                router.local.last_pointer = edge.pointer();
                None
            }
            _ => None,
        };
        if let Some(result) = result {
            match result {
                Ok(_) => {
                    router.session.notice.clear();
                    edited.send(Edited)?;
                }
                Err(error) => router.session.notify(error.to_string()),
            }
        }
    }
    if !ready || router.local.paused || router.local.menu != Menu::None || router.local.departing {
        // UI presses normally outlive one frame, including while gameplay is
        // paused. Keep their owner until release or a real cancellation boundary.
        stop_gameplay(router.local, router.session);
        if !ready || router.local.departing {
            router.local.pointer.cancel();
        }
        router.capture.release();
        return Ok(());
    }
    router.local.movement.forward =
        f32::from(input.held(Action::Forward)) - f32::from(input.held(Action::Backward));
    router.local.movement.strafe =
        f32::from(input.held(Action::Right)) - f32::from(input.held(Action::Left));
    router.local.flight_vertical = if router.local.flying {
        f32::from(input.held(Action::Jump)) - f32::from(input.held(Action::Descend))
    } else {
        0.0
    };
    router.local.look = [
        (f32::from(input.held(Action::LookRight)) - f32::from(input.held(Action::LookLeft))) * 1.8,
        (f32::from(input.held(Action::LookUp)) - f32::from(input.held(Action::LookDown))) * 1.5,
    ];
    if !router.motion_invalidated && router.capture.requested() {
        let motion = input.relative_motion();
        router
            .session
            .game
            .player
            .turn((motion.x() * 0.0025) as f32, (-motion.y() * 0.0025) as f32);
    }
    // Arrow keys and middle-drag remain a usable fallback on desktops that
    // cannot grant capture. Absolute motion is never applied alongside raw motion.
    if !input.held(Action::LookDrag) {
        router.local.middle_drag = false;
    }
    if !router.motion_invalidated
        && router.local.middle_drag
        && !router.capture.is_captured()
        && input.relative_motion() == RelativePointerMotion::ZERO
        && let (Some(previous), Some(current)) = (router.local.last_pointer, input.pointer())
        && previous.viewport() == current.viewport()
    {
        let a = previous.position().to_vec2();
        let b = current.position().to_vec2();
        router
            .session
            .game
            .player
            .turn((b.x() - a.x()) * 0.004, (a.y() - b.y()) * 0.004);
    }
    router.local.last_pointer = input.pointer();
    Ok(())
}

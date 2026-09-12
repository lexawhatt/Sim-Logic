//! Ordered pointer gestures and fixed-screen controls.

use super::*;
use crate::territory_wars::layout::{self, Layout};

pub(super) fn interact(
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
    let click_new_map = clicks_in(&input, session.middle_held, layout::NEW_MAP);
    let new_map = input.has_press_occurrence(Action::NewMap) || click_new_map;
    if new_map || input.has_press_occurrence(Action::Restart) {
        let seed = if new_map {
            session.game.seed().wrapping_add(1)
        } else {
            session.game.seed()
        };
        *session = Session::new(seed);
        session.middle_held = input.held(Action::Pan);
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
    let click_pause = clicks_in(&input, session.middle_held, layout::PAUSE);
    if input.has_press_occurrence(Action::Pause) || click_pause {
        session.paused = !session.paused;
        session.slider_drag = false;
        session.map_drag.cancel();
        session.middle_held = input.held(Action::Pan);
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

    if input.has_press_occurrence(Action::ResetView) {
        session.map_view = MapView::default();
        session.map_drag.cancel();
        session.slider_drag = false;
        session.middle_held = input.held(Action::Pan);
        session.hover = None;
        return Ok(());
    }
    pointer_gestures(&input, &mut session);
    Ok(())
}

// Keep whole-frame pause/restart precedence, but a click made during an MMB
// gesture cannot bypass pointer ownership just because it lands on the HUD.
fn clicks_in(input: &FrameInput<Action>, mut middle: bool, area: layout::Area) -> bool {
    input.edges().any(|edge| {
        if edge.action() == Action::Pan {
            middle = edge.state() == ButtonState::Pressed;
        }
        edge.action() == Action::Click
            && edge.state() == ButtonState::Pressed
            && !middle
            && edge.pointer().is_some_and(|p| {
                let (x, y) = Layout::new(p.viewport()).unproject(p.position());
                area.contains(x, y)
            })
    })
}

fn pointer_gestures(input: &FrameInput<Action>, session: &mut Session) {
    if input.focus_lost() {
        session.map_drag.cancel();
        session.slider_drag = false;
    }

    // Every edge carries the viewport and cursor of its occurrence, including
    // a quick complete drag inside one frame. A missing release position is a
    // cancellation, never a click at the previous cursor location.
    for event in input.events() {
        let edge = match event {
            FrameInputEvent::Scroll(scroll) => {
                session
                    .map_drag
                    .advance(scroll.pointer(), &mut session.map_view);
                if !session.slider_drag
                    && let Some(p) = scroll.pointer()
                {
                    let (x, y) = Layout::new(p.viewport()).unproject(p.position());
                    session.map_view.scroll(x, y, scroll.delta());
                }
                continue;
            }
            FrameInputEvent::Action(edge) => edge,
        };
        if !matches!(edge.action(), Action::Click | Action::Pan) {
            continue;
        }
        session
            .map_drag
            .advance(edge.pointer(), &mut session.map_view);
        if edge.action() == Action::Pan {
            session.middle_held = edge.state() == ButtonState::Pressed;
            if session.middle_held && !input.focus_lost() {
                session.slider_drag = false;
                session.map_drag.begin(edge.pointer());
            } else {
                session.map_drag.cancel();
            }
            continue;
        }
        if session.middle_held {
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
        } else if let Some(cell) = session.map_view.cell_at(x, y) {
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
    session
        .map_drag
        .advance(input.pointer(), &mut session.map_view);
    session.middle_held = input.held(Action::Pan);
    if !session.middle_held {
        session.map_drag.cancel();
    }
    session.hover = input.pointer().and_then(|p| {
        let (x, y) = Layout::new(p.viewport()).unproject(p.position());
        session.map_view.cell_at(x, y)
    });
    if input.pointer().is_none() || !input.held(Action::Click) {
        session.slider_drag = false;
    } else if session.slider_drag
        && let Some(pointer) = input.pointer()
    {
        let (x, _) = Layout::new(pointer.viewport()).unproject(pointer.position());
        session.percent = layout::attack_percent(x);
    }
}

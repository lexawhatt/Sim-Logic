//! Example-owned controls, battle report, and read-only mechanics inspector.

use super::{faction_color, map::centered};
use crate::territory_wars::{
    app::Session,
    drawing::Canvas,
    layout::{self, Area},
    simulation::{self, Decision, FACTIONS, NEUTRAL, Phase, WATER, combat, economy},
};
use sim_logic::prelude::*;

fn ink() -> Color {
    Color::rgb8(225, 236, 241)
}
fn muted() -> Color {
    Color::rgb8(133, 158, 173)
}
fn panel() -> Color {
    Color::rgb8(20, 31, 43)
}

pub(super) fn draw(canvas: &mut Canvas, session: &Session) -> LogicResult {
    let player = &session.game.factions()[0];
    canvas.rect(24.0, 26.0, 5.0, 54.0, faction_color(0))?;
    canvas.text(44.0, 25.0, 5.0, "FRONTIER", ink())?;
    canvas.text(46.0, 72.0, 1.25, "SIM;LOGIC / TERRITORY CONQUEST", muted())?;
    metric(
        canvas,
        392.0,
        "YOUR RESERVE",
        player.troops,
        faction_color(0),
    )?;
    metric(
        canvas,
        632.0,
        "LAND CONTROL",
        (player.land * 100 / session.game.land_count().max(1)) as u32,
        ink(),
    )?;
    canvas.text(704.0, 53.0, 2.0, "%", muted())?;
    metric(
        canvas,
        824.0,
        "IN THE FIELD",
        session.game.campaign_remaining(0),
        faction_color(5),
    )?;
    button(canvas, layout::NEW_MAP, "NEW MAP [N]", panel(), ink())?;
    button(
        canvas,
        layout::PAUSE,
        if session.paused {
            "RESUME [P]"
        } else {
            "PAUSE [P]"
        },
        panel(),
        ink(),
    )?;
    canvas.text(
        1080.0,
        84.0,
        1.15,
        if session.assisted {
            "ASSISTED RUN"
        } else {
            "BOTS / SINGLE PLAYER"
        },
        if session.assisted {
            faction_color(1)
        } else {
            muted()
        },
    )?;
    canvas.rect(
        layout::SIDEBAR.x,
        layout::SIDEBAR.y,
        layout::SIDEBAR.width,
        layout::SIDEBAR.height,
        panel(),
    )?;
    if session.debug {
        inspector(canvas, session)?;
    } else {
        report(canvas, session)?;
    }

    canvas.rect(24.0, 776.0, 1392.0, 100.0, panel())?;
    canvas.text(48.0, 797.0, 1.5, "ATTACK STRENGTH", muted())?;
    canvas.number(
        48.0,
        828.0,
        3.0,
        u32::from(session.percent),
        faction_color(0),
    )?;
    canvas.text(112.0, 833.0, 2.0, "%", faction_color(0))?;
    canvas.rect(
        layout::SLIDER.x,
        824.0,
        layout::SLIDER.width,
        8.0,
        Color::rgb8(48, 67, 80),
    )?;
    let slider_width = (f32::from(session.percent) - 5.0) / 95.0 * layout::SLIDER.width;
    canvas.rect(layout::SLIDER.x, 824.0, slider_width, 8.0, faction_color(0))?;
    canvas.rect(
        layout::SLIDER.x + slider_width - 4.0,
        816.0,
        8.0,
        24.0,
        ink(),
    )?;
    canvas.text(264.0, 792.0, 1.15, "5%", muted())?;
    canvas.text(610.0, 792.0, 1.15, "100%", muted())?;
    button(
        canvas,
        layout::EXPAND,
        "EXPAND [SPACE]",
        faction_color(0),
        Color::rgb8(11, 31, 29),
    )?;
    button(
        canvas,
        layout::DEBUG,
        "DEBUG [F3]",
        if session.debug {
            Color::rgb8(55, 76, 94)
        } else {
            Color::rgb8(34, 49, 64)
        },
        ink(),
    )?;
    canvas.text(1170.0, 801.0, 1.25, "TIME / SEC", muted())?;
    canvas.number(
        1170.0,
        828.0,
        2.3,
        (session.game.elapsed_ticks() / 10).min(u64::from(u32::MAX)) as u32,
        ink(),
    )?;
    let notice = if session.notice_seconds > 0.0 {
        session.notice
    } else {
        "CLICK A BORDERING COLOR TO ATTACK"
    };
    canvas.text(28.0, 759.0, 1.2, notice, muted())?;
    canvas.text(
        28.0,
        884.0,
        1.1,
        "1-5 PRESETS / ARROWS ADJUST / R RESTART",
        muted(),
    )?;
    canvas.text(
        800.0,
        884.0,
        1.1,
        "F3 INSPECT / F4 CHEATS / ESC EXIT",
        muted(),
    )?;
    Ok(())
}

fn metric(canvas: &mut Canvas, x: f32, title: &str, value: u32, color: Color) -> LogicResult {
    canvas.text(x, 26.0, 1.3, title, muted())?;
    canvas.number(x, 50.0, 3.2, value, color)
}

fn button(
    canvas: &mut Canvas,
    area: Area,
    label: &str,
    background: Color,
    foreground: Color,
) -> LogicResult {
    canvas.rect(area.x, area.y, area.width, area.height, background)?;
    centered(
        canvas,
        area.x + area.width * 0.5,
        area.y + area.height * 0.5 - 6.0,
        1.6,
        label,
        foreground,
    )
}

fn report(canvas: &mut Canvas, session: &Session) -> LogicResult {
    let game = &session.game;
    let target = session.hover.map_or(NEUTRAL, |cell| game.owners()[cell]);
    let (name, color) = if usize::from(target) < FACTIONS {
        (
            game.factions()[usize::from(target)].name,
            faction_color(usize::from(target)),
        )
    } else if target == WATER {
        ("WATER", muted())
    } else {
        ("UNCLAIMED LAND", muted())
    };
    canvas.text(1032.0, 138.0, 1.3, "TARGET", muted())?;
    canvas.text(1032.0, 164.0, 2.6, name, color)?;
    let caption = if target == 0 {
        "YOUR TERRITORY"
    } else if target != WATER && game.borders(0, target) {
        "BORDERING RIVAL"
    } else {
        "NEED A LAND BORDER"
    };
    let caption = if target == NEUTRAL {
        "UNCLAIMED LAND"
    } else {
        caption
    };
    canvas.text(1032.0, 200.0, 1.25, caption, muted())?;
    let player = &game.factions()[0];
    let dispatch = economy::dispatch(player.troops, session.percent).unwrap_or(0);
    let cost = if usize::from(target) < FACTIONS {
        let defender = &game.factions()[usize::from(target)];
        combat::capture_cost(defender.troops, defender.land)
    } else {
        combat::capture_cost(0, 0)
    };
    canvas.text(1032.0, 235.0, 1.2, "SENDING", muted())?;
    canvas.number(1222.0, 235.0, 1.5, dispatch, faction_color(0))?;
    canvas.text(1032.0, 259.0, 1.2, "EST. CELLS / CURRENT COST", muted())?;
    let estimate = if target == NEUTRAL {
        combat::neutral_capture_estimate(dispatch) as u32
    } else if target == WATER || target == 0 {
        0
    } else {
        dispatch / cost.max(1)
    };
    canvas.number(1292.0, 258.0, 1.5, estimate, ink())?;
    canvas.text(
        1032.0,
        283.0,
        1.0,
        "ESTIMATE ONLY - BORDERS AND DEFENSE CHANGE",
        muted(),
    )?;
    canvas.rect(1032.0, 310.0, 360.0, 1.0, Color::rgb8(48, 65, 77))?;
    canvas.text(1032.0, 331.0, 2.1, "RIVALS", ink())?;
    canvas.text(1250.0, 339.0, 1.0, "LAND / TROOPS", muted())?;
    let mut ranking: [usize; FACTIONS] = std::array::from_fn(|index| index);
    ranking.sort_unstable_by_key(|index| (std::cmp::Reverse(game.factions()[*index].land), *index));
    for (row, &index) in ranking.iter().enumerate() {
        let faction = &game.factions()[index];
        let y = 375.0 + row as f32 * 43.0;
        let color = if faction.land == 0 && game.phase() != Phase::Choosing {
            muted()
        } else {
            faction_color(index)
        };
        canvas.rect(1032.0, y, 4.0, 27.0, color)?;
        canvas.text(1048.0, y + 1.0, 1.5, faction.name, color)?;
        canvas.number(1200.0, y + 1.0, 1.3, faction.land as u32, ink())?;
        canvas.number(1290.0, y + 1.0, 1.3, faction.troops, ink())?;
        canvas.rect(1048.0, y + 23.0, 336.0, 3.0, Color::rgb8(39, 53, 64))?;
        canvas.rect(
            1048.0,
            y + 23.0,
            336.0 * faction.land as f32 / game.land_count().max(1) as f32,
            3.0,
            color,
        )?;
    }
    canvas.text(
        1032.0,
        704.0,
        1.2,
        "CONTROL 70% OR ELIMINATE EVERY RIVAL",
        muted(),
    )?;
    Ok(())
}

fn inspector(canvas: &mut Canvas, session: &Session) -> LogicResult {
    let game = &session.game;
    let player = &game.factions()[0];
    let report = &game.diagnostics()[0];
    let income = economy::income(player.troops, player.land, game.campaign_remaining(0));
    canvas.text(1032.0, 133.0, 2.25, "DEBUG INSPECTOR", faction_color(0))?;
    canvas.text(1032.0, 163.0, 1.2, "ECONOMY / NEXT SECOND", muted())?;
    for (row, (name, value)) in [
        ("TERRITORY", income.territory),
        ("INTEREST", income.interest),
        ("CREDITED", income.credited),
        ("CAPACITY", income.capacity),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 186.0 + row as f32 * 20.0;
        canvas.text(1032.0, y, 1.2, name, muted())?;
        canvas.number(1272.0, y, 1.35, value, ink())?;
    }
    canvas.text(1032.0, 277.0, 1.2, "EXPEDITION", muted())?;
    canvas.number(
        1272.0,
        277.0,
        1.35,
        game.campaign_remaining(0),
        faction_color(5),
    )?;
    canvas.text(1032.0, 301.0, 1.2, "LAST TICK", ink())?;
    canvas.text(1032.0, 323.0, 1.1, "CAPTURED", muted())?;
    canvas.number(
        1140.0,
        323.0,
        1.3,
        report.last_captured as u32,
        faction_color(0),
    )?;
    canvas.text(1214.0, 323.0, 1.1, "SPENT", muted())?;
    canvas.number(1300.0, 323.0, 1.3, report.last_spent, faction_color(1))?;
    canvas.text(1032.0, 361.0, 1.7, "AI DECISIONS", ink())?;
    canvas.text(
        1200.0,
        366.0,
        1.0,
        if game.bots_enabled() { "ON" } else { "OFF" },
        muted(),
    )?;
    canvas.text(1254.0, 366.0, 1.0, "ARMIES", muted())?;
    canvas.number(
        1312.0,
        361.0,
        1.5,
        game.active_campaigns() as u32,
        faction_color(5),
    )?;
    for index in 1..FACTIONS {
        let d = &game.diagnostics()[index];
        let y = 392.0 + (index - 1) as f32 * 38.0;
        canvas.text(
            1032.0,
            y,
            1.2,
            game.factions()[index].name,
            faction_color(index),
        )?;
        let decision = match d.last_decision {
            Decision::Waiting => "WAITING",
            Decision::Expanding => "EXPANDING",
            Decision::Attacking => "ATTACKING",
            Decision::Holding => "HOLDING",
            Decision::Eliminated => "ELIMINATED",
        };
        canvas.text(1130.0, y, 1.05, decision, muted())?;
        let target = d.decision_target.map_or("-", |id| {
            if id == NEUTRAL {
                "NEUTRAL"
            } else {
                game.factions().get(usize::from(id)).map_or("-", |f| f.name)
            }
        });
        canvas.text(1260.0, y, 1.05, target, ink())?;
        canvas.text(1032.0, y + 16.0, 0.9, "TROOPS", muted())?;
        canvas.number(1080.0, y + 16.0, 1.0, game.factions()[index].troops, ink())?;
        canvas.text(1188.0, y + 16.0, 0.9, "IN THE FIELD", muted())?;
        canvas.number(1304.0, y + 16.0, 1.0, game.campaign_remaining(index), ink())?;
    }
    canvas.text(1032.0, 632.0, 1.1, "FRAME DT MS", muted())?;
    canvas.number(
        1145.0,
        632.0,
        1.1,
        session.frame_ms.min(9999.0) as u32,
        ink(),
    )?;
    canvas.text(1210.0, 632.0, 1.1, "CELLS", muted())?;
    canvas.number(
        1290.0,
        632.0,
        1.1,
        (simulation::WIDTH * simulation::HEIGHT) as u32,
        ink(),
    )?;
    canvas.rect(1024.0, 650.0, 376.0, 90.0, Color::rgb8(33, 40, 49))?;
    canvas.text(
        1036.0,
        660.0,
        1.3,
        if session.cheats {
            "CHEATS ARMED"
        } else {
            "CHEATS OFF"
        },
        if session.cheats {
            faction_color(1)
        } else {
            muted()
        },
    )?;
    canvas.text(1036.0, 681.0, 1.05, "F4 ARM / DISARM CHEATS", muted())?;
    canvas.text(1036.0, 697.0, 1.05, "F5 TROOPS / F6 AI ORDERS", muted())?;
    canvas.text(1036.0, 713.0, 1.05, "F8 WIN / F9 STEP WHEN PAUSED", muted())?;
    Ok(())
}

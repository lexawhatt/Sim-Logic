//! Row-run terrain drawing; no image is rebuilt as ownership changes.

use super::{faction_color, shade};
use crate::territory_wars::{
    app::Session,
    drawing::Canvas,
    layout::{self, CELL, MAP, MAP_X, MAP_Y},
    simulation::{FACTIONS, HEIGHT, NEUTRAL, Phase, WATER, WIDTH},
};
use sim_logic::prelude::*;

pub(super) fn draw(canvas: &mut Canvas, session: &Session) -> LogicResult {
    canvas.rect(
        MAP_X - 1.0,
        MAP_Y - 1.0,
        MAP.width + 2.0,
        MAP.height + 2.0,
        Color::rgb8(49, 66, 78),
    )?;
    canvas.rect(MAP_X, MAP_Y, MAP.width, MAP.height, Color::rgb8(17, 30, 43))?;
    for column in (0..WIDTH).step_by(8) {
        canvas.rect(
            MAP_X + column as f32 * CELL,
            MAP_Y,
            1.0,
            MAP.height,
            Color::rgb8(22, 38, 50),
        )?;
    }
    for row in (0..HEIGHT).step_by(8) {
        canvas.rect(
            MAP_X,
            MAP_Y + row as f32 * CELL,
            MAP.width,
            1.0,
            Color::rgb8(22, 38, 50),
        )?;
    }
    let owners = session.game.owners();
    let mut colors = [Color::BLACK; 16];
    for owner in 0..FACTIONS {
        colors[owner * 2] = faction_color(owner);
        colors[owner * 2 + 1] = shade(faction_color(owner), 0.62);
    }
    colors[14] = Color::rgb8(53, 72, 63);
    colors[15] = Color::rgb8(74, 94, 78);
    let key = |cell: usize| -> Option<usize> {
        let owner = owners[cell];
        if owner == WATER {
            return None;
        }
        let x = cell % WIDTH;
        let y = cell / WIDTH;
        let edge = x == 0
            || x + 1 == WIDTH
            || y == 0
            || y + 1 == HEIGHT
            || owners[cell - 1] != owner
            || owners[cell + 1] != owner
            || owners[cell - WIDTH] != owner
            || owners[cell + WIDTH] != owner;
        Some(
            (if owner == NEUTRAL {
                7
            } else {
                usize::from(owner)
            }) * 2
                + usize::from(edge),
        )
    };
    // At most one rectangle per land cell even for a fragmented late-game map.
    for row in 0..HEIGHT {
        let mut x = 0;
        while x < WIDTH {
            let cell = row * WIDTH + x;
            let Some(color) = key(cell) else {
                x += 1;
                continue;
            };
            let begin = x;
            x += 1;
            while x < WIDTH && key(row * WIDTH + x) == Some(color) {
                x += 1;
            }
            canvas.rect(
                MAP_X + begin as f32 * CELL,
                MAP_Y + row as f32 * CELL,
                (x - begin) as f32 * CELL,
                CELL,
                colors[color],
            )?;
        }
    }
    if let Some(cell) = session.hover {
        let (x, y) = layout::cell_center(cell);
        let color = if owners[cell] == WATER {
            Color::rgb8(229, 104, 88)
        } else {
            Color::WHITE
        };
        outline(canvas, x - CELL * 0.5, y - CELL * 0.5, CELL, CELL, color)?;
    }
    if session.game.phase() == Phase::Running && !session.paused {
        labels(canvas, session)?;
    } else {
        let (title, subtitle, hint, accent) = match session.game.phase() {
            Phase::Choosing => (
                "CHOOSE YOUR HOME",
                "CLICK ANY LAND TILE TO BEGIN",
                "GROW. EXPAND. HOLD YOUR BORDERS.",
                faction_color(0),
            ),
            Phase::Won => (
                "VICTORY",
                "THE ISLAND IS YOURS",
                "PRESS R TO PLAY AGAIN",
                faction_color(0),
            ),
            Phase::Lost => (
                "DEFEAT",
                "YOUR LAST BORDER HAS FALLEN",
                "PRESS R TO TRY AGAIN",
                faction_color(1),
            ),
            Phase::Running => (
                "PAUSED",
                "P TO RESUME / F9 SINGLE TICK",
                "DEBUG INSPECTOR REMAINS AVAILABLE",
                faction_color(5),
            ),
        };
        // Starting prompt sits above the coastline, leaving the map clickable.
        let y = if session.game.phase() == Phase::Choosing {
            135.0
        } else {
            360.0
        };
        canvas.rect(
            202.0,
            y,
            604.0,
            126.0,
            Color::rgb8(13, 23, 32).with_alpha(0.96),
        )?;
        canvas.rect(202.0, y, 604.0, 3.0, accent)?;
        centered(canvas, 504.0, y + 20.0, 3.0, title, accent)?;
        centered(
            canvas,
            504.0,
            y + 64.0,
            1.7,
            subtitle,
            Color::rgb8(220, 231, 235),
        )?;
        centered(
            canvas,
            504.0,
            y + 95.0,
            1.4,
            hint,
            Color::rgb8(137, 162, 175),
        )?;
    }
    Ok(())
}

fn labels(canvas: &mut Canvas, session: &Session) -> LogicResult {
    let mut sums = [(0usize, 0usize); FACTIONS];
    for (cell, &owner) in session.game.owners().iter().enumerate() {
        if usize::from(owner) < FACTIONS {
            sums[usize::from(owner)].0 += cell % WIDTH;
            sums[usize::from(owner)].1 += cell / WIDTH;
        }
    }
    for (index, faction) in session.game.factions().iter().enumerate() {
        if faction.land == 0 {
            continue;
        }
        let cx = sums[index].0 as f32 / faction.land as f32;
        let cy = sums[index].1 as f32 / faction.land as f32;
        // Place the badge on owned land nearest the centroid, not in a lake.
        let cell = session
            .game
            .owners()
            .iter()
            .enumerate()
            .filter(|(_, owner)| **owner == index as u8)
            .min_by(|(a, _), (b, _)| {
                let distance = |cell: usize| {
                    ((cell % WIDTH) as f32 - cx).powi(2) + ((cell / WIDTH) as f32 - cy).powi(2)
                };
                distance(*a).total_cmp(&distance(*b))
            })
            .map_or(faction.capital, |(cell, _)| cell);
        let (x, y) = layout::cell_center(cell);
        let x = x.clamp(MAP_X + 54.0, MAP_X + MAP.width - 54.0);
        let y = y.clamp(MAP_Y + 30.0, MAP_Y + MAP.height - 30.0);
        canvas.rect(
            x - 46.0,
            y - 23.0,
            92.0,
            44.0,
            Color::rgb8(9, 17, 23).with_alpha(0.88),
        )?;
        canvas.rect(x - 46.0, y - 23.0, 3.0, 44.0, faction_color(index))?;
        centered(canvas, x, y - 14.0, 1.5, faction.name, faction_color(index))?;
        let digits = faction.troops.checked_ilog10().unwrap_or(0) + 1;
        canvas.number(
            x - digits as f32 * 4.5,
            y + 3.0,
            1.5,
            faction.troops,
            Color::WHITE,
        )?;
    }
    Ok(())
}

pub(super) fn centered(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    scale: f32,
    text: &str,
    color: Color,
) -> LogicResult {
    canvas.text(
        x - (text.len() as f32 * 6.0 - 1.0) * scale * 0.5,
        y,
        scale,
        text,
        color,
    )
}

fn outline(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
) -> LogicResult {
    canvas.rect(x, y, width, 1.0, color)?;
    canvas.rect(x, y + height - 1.0, width, 1.0, color)?;
    canvas.rect(x, y, 1.0, height, color)?;
    canvas.rect(x + width - 1.0, y, 1.0, height, color)
}

//! A bounded software-style raycaster expressed as managed screen rectangles.

use std::{error::Error, fmt};

use sim_engine::SceneBudget;
use sim_logic::prelude::*;

use super::{
    GameAction,
    game::{GameState, Phase, PickupKind},
    level,
};

#[path = "font.rs"]
mod font;

/// Fixed rectangle source, staging, and scene capacity for this example.
/// A frame exceeding this capacity fails explicitly before updating visuals.
pub const MAX_QUADS: usize = 8192;
const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 360.0;
const VIEW_HEIGHT: f32 = 304.0;
const HORIZON: f32 = 143.0;
const RAYS: usize = 320;
const COLUMN_WIDTH: f32 = WIDTH / RAYS as f32;
const CAMERA_PLANE: f32 = 0.8;
const PROJECTION: f32 = WIDTH / (2.0 * CAMERA_PLANE);
const NEAR: f32 = 0.06;
const INK: Color = Color::rgb(0.00242822, 0.00402472, 0.00604883);
const PAPER: Color = Color::rgb(0.65140564, 0.737_910_4, 0.65837482);
const TEAL: Color = Color::rgb(0.10461648, 0.737_910_4, 0.54572446);
const AMBER: Color = Color::rgb(0.871_367_1, 0.428_690_5, 0.07818742);
const RED: Color = Color::rgb(0.775_822_2, 0.06662594, 0.04666509);

#[derive(Clone, Copy)]
struct Quad {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DrawError {
    BudgetExceeded,
    InvalidGeometry,
}

impl fmt::Display for DrawError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded => write!(
                formatter,
                "Iron Maze exceeded its {MAX_QUADS}-rectangle presentation budget"
            ),
            Self::InvalidGeometry => {
                formatter.write_str("Iron Maze produced non-finite presentation geometry")
            }
        }
    }
}

impl Error for DrawError {}

#[derive(Resource)]
struct Canvas {
    quads: Vec<Quad>,
    wall_depth: [f32; RAYS],
}

#[derive(Component)]
struct QuadSlot(usize);

impl Canvas {
    fn new() -> Self {
        Self {
            quads: Vec::with_capacity(MAX_QUADS),
            wall_depth: [f32::INFINITY; RAYS],
        }
    }

    fn rectangle(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    ) -> Result<(), DrawError> {
        if ![x, y, width, height, x + width, y + height]
            .into_iter()
            .all(f32::is_finite)
        {
            return Err(DrawError::InvalidGeometry);
        }
        let right = (x + width).clamp(0.0, WIDTH);
        let bottom = (y + height).clamp(0.0, HEIGHT);
        let x = x.clamp(0.0, WIDTH);
        let y = y.clamp(0.0, HEIGHT);
        if right <= x || bottom <= y {
            return Ok(());
        }
        if self.quads.len() == MAX_QUADS {
            return Err(DrawError::BudgetExceeded);
        }
        self.quads.push(Quad {
            x,
            y,
            width: right - x,
            height: bottom - y,
            color,
        });
        Ok(())
    }

    fn view_rectangle(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    ) -> Result<(), DrawError> {
        self.rectangle(x, y, width, height.min(VIEW_HEIGHT - y), color)
    }

    fn text(
        &mut self,
        x: f32,
        y: f32,
        scale: f32,
        message: &[u8],
        color: Color,
    ) -> Result<(), DrawError> {
        for (index, &character) in message.iter().enumerate() {
            for (row, bits) in font::glyph(character).into_iter().enumerate() {
                let mut column = 0;
                while column < 5 {
                    if bits & (1 << (4 - column)) == 0 {
                        column += 1;
                        continue;
                    }
                    let start = column;
                    while column < 5 && bits & (1 << (4 - column)) != 0 {
                        column += 1;
                    }
                    self.rectangle(
                        x + (index * 6 + start) as f32 * scale,
                        y + row as f32 * scale,
                        (column - start) as f32 * scale,
                        scale,
                        color,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn number(
        &mut self,
        x: f32,
        y: f32,
        scale: f32,
        value: u32,
        color: Color,
    ) -> Result<(), DrawError> {
        // The HUD has three digit cells; saturation is presentation-only.
        let value = value.min(999);
        let digits = [
            b'0' + (value / 100) as u8,
            b'0' + (value / 10 % 10) as u8,
            b'0' + (value % 10) as u8,
        ];
        self.text(x, y, scale, &digits, color)
    }

    fn draw(&mut self, state: &GameState) -> Result<(), DrawError> {
        self.quads.clear();
        self.background()?;
        self.walls(state)?;
        self.sprites(state)?;
        self.weapon(state)?;
        self.crosshair(state)?;
        if state.hurt_flash > 0.0 {
            let flash = (state.hurt_flash * 1.8).clamp(0.0, 0.3);
            self.rectangle(0.0, 0.0, WIDTH, VIEW_HEIGHT, RED.with_alpha(flash))?;
            self.rectangle(0.0, 0.0, 5.0, VIEW_HEIGHT, RED)?;
            self.rectangle(WIDTH - 5.0, 0.0, 5.0, VIEW_HEIGHT, RED)?;
        }
        self.hud(state)?;
        self.overlay(state)
    }

    fn background(&mut self) -> Result<(), DrawError> {
        self.rectangle(0.0, 0.0, WIDTH, HEIGHT, INK)?;
        for band in 0..18 {
            let amount = band as f32 / 18.0;
            self.view_rectangle(
                0.0,
                band as f32 * 8.0,
                WIDTH,
                8.0,
                shade(Color::rgb8(34, 43, 44), 1.0 - amount * 0.62),
            )?;
        }
        for band in 0..40 {
            let y = HORIZON + band as f32 * 4.1;
            self.view_rectangle(
                0.0,
                y,
                WIDTH,
                4.1,
                shade(Color::rgb8(70, 63, 51), 0.22 + band as f32 / 58.0),
            )?;
        }
        // Distance-compressed floor seams provide perspective without textures.
        for depth in [0.7, 1.0, 1.5, 2.2, 3.3, 5.0, 8.0, 13.0] {
            let y = HORIZON + PROJECTION * 0.5 / depth;
            self.view_rectangle(0.0, y, WIDTH, 1.0, Color::rgb8(24, 28, 27))?;
        }
        Ok(())
    }

    fn walls(&mut self, state: &GameState) -> Result<(), DrawError> {
        let direction = Vec2::new(state.player.angle.cos(), state.player.angle.sin());
        let plane = Vec2::new(-direction.y(), direction.x()) * CAMERA_PLANE;
        for column in 0..RAYS {
            let camera_x = 2.0 * (column as f32 + 0.5) / RAYS as f32 - 1.0;
            let ray = direction + plane * camera_x;
            let Some(hit) = level::cast_ray(state.player.position, ray, 40.0) else {
                self.wall_depth[column] = f32::INFINITY;
                continue;
            };
            self.wall_depth[column] = hit.distance;
            let depth = hit.distance.max(NEAR);
            let height = PROJECTION / depth;
            let top = HORIZON - height * 0.5;
            let x = column as f32 * COLUMN_WIDTH;
            let panel = (hit.texture_u * 8.0).floor() as u32;
            let edge = !(0.025..=0.975).contains(&hit.texture_u);
            let material = match hit.tile {
                2 => Color::rgb8(105, 78, 58),
                3 => Color::rgb8(69, 100, 97),
                _ => Color::rgb8(97, 104, 97),
            };
            let light = (1.0 / (1.0 + depth * 0.115)) * if hit.side == 0 { 1.0 } else { 0.76 };
            let pattern = if edge {
                0.48
            } else if panel.is_multiple_of(2) {
                1.0
            } else {
                0.91
            };
            self.view_rectangle(
                x,
                top,
                COLUMN_WIDTH,
                height,
                shade(material, light * pattern),
            )?;
            self.view_rectangle(
                x,
                top + height * 0.07,
                COLUMN_WIDTH,
                height * 0.035,
                shade(Color::rgb8(157, 161, 139), light),
            )?;
            self.view_rectangle(
                x,
                top + height * 0.12,
                COLUMN_WIDTH,
                height * 0.025,
                shade(INK, light),
            )?;
            self.view_rectangle(
                x,
                top + height * 0.68,
                COLUMN_WIDTH,
                height * 0.028,
                shade(INK, light),
            )?;
            self.view_rectangle(
                x,
                top + height * 0.87,
                COLUMN_WIDTH,
                height * 0.09,
                shade(Color::rgb8(55, 52, 43), light),
            )?;
            if hit.tile == 2 {
                let warning = if ((hit.texture_u * 16.0) as u32).is_multiple_of(2) {
                    AMBER
                } else {
                    INK
                };
                self.view_rectangle(
                    x,
                    top + height * 0.81,
                    COLUMN_WIDTH,
                    height * 0.035,
                    shade(warning, light),
                )?;
            } else if (0.17..0.83).contains(&hit.texture_u) {
                self.view_rectangle(
                    x,
                    top + height * 0.105,
                    COLUMN_WIDTH,
                    height * 0.015,
                    shade(TEAL, 0.65 + light * 0.35),
                )?;
            }
        }
        Ok(())
    }

    fn sprites(&mut self, state: &GameState) -> Result<(), DrawError> {
        let direction = Vec2::new(state.player.angle.cos(), state.player.angle.sin());
        let right = Vec2::new(-direction.y(), direction.x());
        let mut sprites = [ProjectedSprite::EMPTY; 11];
        let mut count = 0;
        for enemy in &state.enemies {
            if enemy.health > 0 {
                sprites[count] = ProjectedSprite::new(
                    enemy.position,
                    state.player.position,
                    direction,
                    right,
                    SpriteKind::Enemy(enemy.hit_flash > 0.0),
                );
                count += 1;
            }
        }
        for pickup in &state.pickups {
            if pickup.active {
                sprites[count] = ProjectedSprite::new(
                    pickup.position,
                    state.player.position,
                    direction,
                    right,
                    match pickup.kind {
                        PickupKind::Health => SpriteKind::Health,
                        PickupKind::Ammo => SpriteKind::Ammo,
                    },
                );
                count += 1;
            }
        }
        sprites[count] = ProjectedSprite::new(
            level::EXIT,
            state.player.position,
            direction,
            right,
            SpriteKind::Exit(state.kills == state.enemies.len() as u32),
        );
        count += 1;
        // Eleven entries give a small fixed upper bound; unstable sorting needs no scratch allocation.
        sprites[..count].sort_unstable_by(|left, right| right.depth.total_cmp(&left.depth));
        for sprite in &sprites[..count] {
            if sprite.depth >= NEAR {
                self.sprite(*sprite)?;
            }
        }
        Ok(())
    }

    fn sprite(&mut self, sprite: ProjectedSprite) -> Result<(), DrawError> {
        let rows = sprite.kind.rows();
        let pixel = PROJECTION * sprite.kind.world_height() / sprite.depth / rows.len() as f32;
        let width = rows[0].len() as f32 * pixel;
        let left = sprite.screen_x - width * 0.5;
        let top = HORIZON + PROJECTION * 0.5 / sprite.depth - rows.len() as f32 * pixel;
        if left >= WIDTH
            || left + width <= 0.0
            || top >= VIEW_HEIGHT
            || top + rows.len() as f32 * pixel <= 0.0
        {
            return Ok(());
        }
        let light = (1.0 / (1.0 + sprite.depth * 0.08)).clamp(0.32, 1.0);
        for (row, pixels) in rows.iter().enumerate() {
            let mut column = 0;
            while column < pixels.len() {
                let start = column;
                let symbol = pixels[column];
                column += 1;
                while column < pixels.len() && pixels[column] == symbol {
                    column += 1;
                }
                if symbol != b'.' {
                    let color = sprite.kind.color(symbol, light);
                    self.occluded_rectangle(
                        left + start as f32 * pixel,
                        top + row as f32 * pixel,
                        (column - start) as f32 * pixel,
                        pixel,
                        sprite.depth,
                        color,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn occluded_rectangle(
        &mut self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        depth: f32,
        color: Color,
    ) -> Result<(), DrawError> {
        if y + height <= 0.0 || y >= VIEW_HEIGHT || x + width <= 0.0 || x >= WIDTH {
            return Ok(());
        }
        let left = x.max(0.0);
        let right = (x + width).min(WIDTH);
        let first = (left / COLUMN_WIDTH).floor() as usize;
        let last = (right / COLUMN_WIDTH).ceil() as usize;
        let mut run_start = None;
        for column in first..last.min(RAYS) {
            if depth < self.wall_depth[column] {
                run_start.get_or_insert((column as f32 * COLUMN_WIDTH).max(left));
            } else if let Some(start) = run_start.take() {
                self.view_rectangle(
                    start,
                    y,
                    column as f32 * COLUMN_WIDTH - start,
                    height,
                    color,
                )?;
            }
        }
        if let Some(start) = run_start {
            self.view_rectangle(start, y, right - start, height, color)?;
        }
        Ok(())
    }

    fn weapon(&mut self, state: &GameState) -> Result<(), DrawError> {
        let recoil = if state.muzzle > 0.0 { 8.0 } else { 0.0 };
        let sway = state.player.walk_phase.sin() * 3.0;
        let x = 321.0 + sway;
        let y = 257.0 + recoil + (state.player.walk_phase * 2.0).cos() * 1.5;
        if state.muzzle > 0.0 {
            for (dx, dy, width, height, color) in [
                (-11.0, -71.0, 23.0, 45.0, AMBER),
                (-27.0, -56.0, 57.0, 14.0, AMBER),
                (-18.0, -63.0, 39.0, 28.0, Color::rgb8(255, 214, 117)),
                (-8.0, -58.0, 19.0, 31.0, Color::rgb8(255, 246, 209)),
            ] {
                self.view_rectangle(x + dx, y + dy, width, height, color)?;
            }
        }
        for (dx, dy, width, height, color) in [
            (-50.0, 21.0, 28.0, 47.0, Color::rgb8(92, 68, 50)),
            (34.0, 17.0, 30.0, 48.0, Color::rgb8(119, 87, 59)),
            (-36.0, 3.0, 80.0, 59.0, INK),
            (-28.0, -10.0, 62.0, 59.0, Color::rgb8(54, 66, 65)),
            (-17.0, -34.0, 34.0, 67.0, INK),
            (-13.0, -35.0, 26.0, 58.0, Color::rgb8(101, 113, 103)),
            (-8.0, -31.0, 16.0, 56.0, Color::rgb8(36, 47, 49)),
            (-6.0, -30.0, 4.0, 42.0, Color::rgb8(144, 152, 127)),
            (-16.0, -36.0, 32.0, 6.0, Color::rgb8(28, 36, 37)),
            (-2.0, -42.0, 4.0, 9.0, INK),
            (-24.0, 1.0, 10.0, 29.0, Color::rgb8(136, 111, 70)),
            (20.0, 2.0, 9.0, 28.0, Color::rgb8(98, 85, 58)),
            (-20.0, 31.0, 48.0, 8.0, INK),
            (-15.0, 31.0, 12.0, 3.0, TEAL),
            (2.0, 31.0, 12.0, 3.0, TEAL),
            (-17.0, 44.0, 36.0, 18.0, Color::rgb8(38, 43, 42)),
        ] {
            self.view_rectangle(x + dx, y + dy, width, height, color)?;
        }
        Ok(())
    }

    fn crosshair(&mut self, state: &GameState) -> Result<(), DrawError> {
        if state.phase != Phase::Playing {
            return Ok(());
        }
        for (x, y, width, height) in [
            (307.0, HORIZON, 7.0, 1.0),
            (327.0, HORIZON, 7.0, 1.0),
            (320.0, HORIZON - 12.0, 1.0, 7.0),
            (320.0, HORIZON + 6.0, 1.0, 7.0),
        ] {
            self.rectangle(x + 1.0, y + 1.0, width, height, INK)?;
            self.rectangle(x, y, width, height, PAPER)?;
        }
        Ok(())
    }

    fn hud(&mut self, state: &GameState) -> Result<(), DrawError> {
        self.rectangle(0.0, 0.0, 269.0, 42.0, INK.with_alpha(0.83))?;
        self.rectangle(12.0, 12.0, 3.0, 17.0, TEAL)?;
        self.text(23.0, 12.0, 2.0, b"IRON MAZE", PAPER)?;
        self.text(23.0, 31.0, 1.0, b"SECTOR 07 / CONTAINMENT BREACH", AMBER)?;
        self.minimap(state)?;
        self.rectangle(0.0, VIEW_HEIGHT, WIDTH, HEIGHT - VIEW_HEIGHT, INK)?;
        self.rectangle(0.0, VIEW_HEIGHT, WIDTH, 2.0, Color::rgb8(95, 111, 98))?;
        self.rectangle(
            14.0,
            317.0,
            3.0,
            24.0,
            if state.player.health < 30 { RED } else { TEAL },
        )?;
        self.text(24.0, 314.0, 1.0, b"VITALS", TEAL)?;
        self.number(24.0, 326.0, 2.0, state.player.health, PAPER)?;
        self.rectangle(73.0, 331.0, 68.0, 7.0, Color::rgb8(35, 43, 42))?;
        self.rectangle(
            73.0,
            331.0,
            68.0 * state.player.health.min(100) as f32 / 100.0,
            7.0,
            if state.player.health < 30 { RED } else { TEAL },
        )?;
        self.rectangle(155.0, 315.0, 1.0, 25.0, Color::rgb8(47, 57, 54))?;
        self.text(170.0, 314.0, 1.0, b"ROUNDS", AMBER)?;
        self.number(170.0, 326.0, 2.0, state.player.ammo, PAPER)?;
        self.text(233.0, 314.0, 1.0, b"HOSTILES", RED)?;
        self.text(
            233.0,
            326.0,
            2.0,
            &[b'0' + state.kills.min(6) as u8, b'/', b'6'],
            PAPER,
        )?;
        self.rectangle(293.0, 315.0, 1.0, 25.0, Color::rgb8(47, 57, 54))?;
        self.text(309.0, 315.0, 1.0, b"OBJECTIVE", TEAL)?;
        self.text(
            309.0,
            329.0,
            1.0,
            if state.kills == state.enemies.len() as u32 {
                b"EXIT OPEN - REACH THE TEAL BEACON"
            } else {
                b"CLEAR SIX SENTRIES. FIND THE EXIT."
            },
            PAPER,
        )?;
        self.text(
            16.0,
            349.0,
            1.0,
            b"WASD MOVE  /  LEFT-RIGHT TURN  /  SPACE-CLICK FIRE  /  ENTER RESTART  /  ESC QUIT",
            Color::rgb8(131, 151, 140),
        )?;
        Ok(())
    }

    fn minimap(&mut self, state: &GameState) -> Result<(), DrawError> {
        const SCALE: f32 = 5.0;
        const X: f32 = 546.0;
        const Y: f32 = 13.0;
        self.rectangle(
            X - 5.0,
            Y - 5.0,
            level::WIDTH as f32 * SCALE + 10.0,
            level::HEIGHT as f32 * SCALE + 22.0,
            INK,
        )?;
        for y in 0..level::HEIGHT {
            for x in 0..level::WIDTH {
                let tile = level::cell(x as i32, y as i32);
                if tile > 0 {
                    self.rectangle(
                        X + x as f32 * SCALE,
                        Y + y as f32 * SCALE,
                        SCALE - 1.0,
                        SCALE - 1.0,
                        match tile {
                            2 => Color::rgb8(92, 76, 55),
                            3 => Color::rgb8(55, 97, 90),
                            _ => Color::rgb8(61, 76, 72),
                        },
                    )?;
                }
            }
        }
        for enemy in &state.enemies {
            if enemy.health > 0 {
                self.rectangle(
                    X + enemy.position.x() * SCALE - 1.0,
                    Y + enemy.position.y() * SCALE - 1.0,
                    2.0,
                    2.0,
                    RED,
                )?;
            }
        }
        for pickup in &state.pickups {
            if pickup.active {
                self.rectangle(
                    X + pickup.position.x() * SCALE - 1.0,
                    Y + pickup.position.y() * SCALE - 1.0,
                    2.0,
                    2.0,
                    AMBER,
                )?;
            }
        }
        self.rectangle(
            X + level::EXIT.x() * SCALE - 1.5,
            Y + level::EXIT.y() * SCALE - 1.5,
            3.0,
            3.0,
            TEAL,
        )?;
        let player_x = X + state.player.position.x() * SCALE;
        let player_y = Y + state.player.position.y() * SCALE;
        self.rectangle(player_x - 1.5, player_y - 1.5, 3.0, 3.0, PAPER)?;
        self.rectangle(
            player_x + state.player.angle.cos() * 3.0 - 1.0,
            player_y + state.player.angle.sin() * 3.0 - 1.0,
            2.0,
            2.0,
            PAPER,
        )?;
        self.text(X + 1.0, Y + 85.0, 1.0, b"TACTICAL SCAN", TEAL)
    }

    fn overlay(&mut self, state: &GameState) -> Result<(), DrawError> {
        let (title, subtitle, accent) = match state.phase {
            Phase::Playing => return Ok(()),
            Phase::Won => (
                b"SECTOR SECURED".as_slice(),
                b"ALL SENTRIES DOWN. YOU MADE IT OUT.".as_slice(),
                TEAL,
            ),
            Phase::Dead => (
                b"SIGNAL LOST".as_slice(),
                b"THE MAZE CLAIMS ANOTHER OPERATOR.".as_slice(),
                RED,
            ),
        };
        self.rectangle(0.0, 0.0, WIDTH, VIEW_HEIGHT, INK.with_alpha(0.66))?;
        self.rectangle(118.0, 93.0, 404.0, 124.0, INK)?;
        self.rectangle(118.0, 93.0, 404.0, 3.0, accent)?;
        self.rectangle(118.0, 214.0, 404.0, 3.0, accent)?;
        self.text(
            (WIDTH - title.len() as f32 * 18.0 + 3.0) * 0.5,
            113.0,
            3.0,
            title,
            accent,
        )?;
        self.text(
            (WIDTH - subtitle.len() as f32 * 6.0) * 0.5,
            153.0,
            1.0,
            subtitle,
            PAPER,
        )?;
        self.text(
            200.0,
            184.0,
            1.0,
            b"ENTER RESTART   /   ESC LEAVE THE SECTOR",
            AMBER,
        )
    }
}

#[derive(Clone, Copy)]
struct ProjectedSprite {
    screen_x: f32,
    depth: f32,
    kind: SpriteKind,
}

impl ProjectedSprite {
    const EMPTY: Self = Self {
        screen_x: 0.0,
        depth: -1.0,
        kind: SpriteKind::Health,
    };

    fn new(position: Vec2, player: Vec2, forward: Vec2, right: Vec2, kind: SpriteKind) -> Self {
        let offset = position - player;
        let depth = offset.x() * forward.x() + offset.y() * forward.y();
        let lateral = offset.x() * right.x() + offset.y() * right.y();
        Self {
            screen_x: WIDTH * 0.5 + PROJECTION * lateral / depth.max(NEAR),
            depth,
            kind,
        }
    }
}

#[derive(Clone, Copy)]
enum SpriteKind {
    Enemy(bool),
    Health,
    Ammo,
    Exit(bool),
}

impl SpriteKind {
    fn rows(self) -> &'static [&'static [u8]] {
        match self {
            Self::Enemy(_) => &[
                b"....kkkk....",
                b"...krrrrk...",
                b"...rRrrRr...",
                b"...kyyyyk...",
                b"...kssssk...",
                b"..rrkkkkrr..",
                b".rRRrrrrRRr.",
                b".rRkrrrrkRr.",
                b".srkRrrRkrs.",
                b".sskkkkkkss.",
                b"..kkgGGgkk..",
                b"...kkkkkk...",
                b"...ss..ss...",
                b"...sr..rs...",
                b"..kkk..kkk..",
                b"..sss..sss..",
            ],
            Self::Health => &[
                b"..wwww..",
                b".wWWWWw.",
                b"wWWrrWWw",
                b"wWrrrrWw",
                b"wWrrrrWw",
                b"wWWrrWWw",
                b"wWWWWWWw",
                b".kkkkkk.",
            ],
            Self::Ammo => &[
                b".yyyyyy.",
                b".yYyYyY.",
                b".sYsYsY.",
                b"kkkkkkkk",
                b"sSSSSSSs",
                b"sSyyyySs",
                b"sSSSSSSs",
                b".kkkkkk.",
            ],
            Self::Exit(_) => &[
                b"..tttt..",
                b".tTTTTt.",
                b".tTkkTt.",
                b".tTkkTt.",
                b".tTkTTt.",
                b".tTkTTt.",
                b".tTkkTt.",
                b".tTTTTt.",
                b"..tttt..",
                b"...ss...",
                b"...ss...",
                b"...ss...",
                b"..ssss..",
                b".ssssss.",
                b".kkkkkk.",
                b"kkkkkkkk",
            ],
        }
    }

    fn world_height(self) -> f32 {
        match self {
            Self::Enemy(_) => 0.92,
            Self::Health | Self::Ammo => 0.28,
            Self::Exit(_) => 0.82,
        }
    }

    fn color(self, symbol: u8, light: f32) -> Color {
        if matches!(self, Self::Enemy(true)) && symbol != b'k' {
            return PAPER;
        }
        let color = match symbol {
            b'k' => Color::rgb8(18, 24, 24),
            b'r' => Color::rgb8(165, 48, 40),
            b'R' => Color::rgb8(231, 83, 58),
            b's' => Color::rgb8(64, 77, 72),
            b'S' => Color::rgb8(112, 130, 106),
            b'y' => AMBER,
            b'Y' => Color::rgb8(255, 219, 132),
            b'g' => Color::rgb8(38, 43, 43),
            b'G' => Color::rgb8(127, 138, 112),
            b'w' => Color::rgb8(126, 148, 131),
            b'W' => PAPER,
            b't' => {
                if matches!(self, Self::Exit(false)) {
                    Color::rgb8(51, 92, 86)
                } else {
                    TEAL
                }
            }
            b'T' => {
                if matches!(self, Self::Exit(false)) {
                    Color::rgb8(111, 165, 139)
                } else {
                    Color::rgb8(181, 255, 211)
                }
            }
            _ => INK,
        };
        shade(
            color,
            if matches!(symbol, b'y' | b'Y' | b't' | b'T') {
                light.max(0.8)
            } else {
                light
            },
        )
    }
}

fn shade(color: Color, amount: f32) -> Color {
    Color::rgba(
        color.red() * amount,
        color.green() * amount,
        color.blue() * amount,
        color.alpha(),
    )
}

#[derive(Clone, Copy)]
struct Letterbox {
    scale: f32,
    x: f32,
    y: f32,
}

impl Letterbox {
    fn new(width: f32, height: f32) -> Self {
        let scale = (width / WIDTH).min(height / HEIGHT);
        Self {
            scale,
            x: (width - WIDTH * scale) * 0.5,
            y: (height - HEIGHT * scale) * 0.5,
        }
    }

    fn geometry(self, quad: Quad) -> (LogicalScreenPosition, LogicalScreenVector) {
        let x = self.x + quad.x * self.scale;
        let y = self.y + quad.y * self.scale;
        // Thin distant details can be smaller than one representable step at
        // extreme aspect ratios. Expand only their far bounds by one ULP.
        let right = (self.x + (quad.x + quad.width) * self.scale).max(x.next_up());
        let bottom = (self.y + (quad.y + quad.height) * self.scale).max(y.next_up());
        (
            LogicalScreenPosition::new(x, y),
            LogicalScreenVector::new(right - x, bottom - y),
        )
    }
}

fn update(
    viewport: FrameViewport,
    state: Res<GameState>,
    mut canvas: ResMut<Canvas>,
    mut rectangles: Query<(&QuadSlot, &mut ScreenRectangleVisual)>,
) -> LogicResult {
    canvas.draw(&state)?;
    let mapping = Letterbox::new(viewport.logical().width(), viewport.logical().height());
    for (slot, mut rectangle) in &mut rectangles {
        if let Some(&quad) = canvas.quads.get(slot.0) {
            let (position, size) = mapping.geometry(quad);
            rectangle.set_geometry(position, size)?;
            rectangle.set_color(quad.color)?;
        } else {
            rectangle.set_color(Color::TRANSPARENT)?;
        }
    }
    Ok(())
}

/// Registers the presentation-only pool component and fallible FrameUpdate.
/// Canonical gameplay state is read without changing simulation outcomes.
pub fn register(application: &mut Application<GameAction>) -> LogicResult {
    application.approve_components::<(QuadSlot,)>()?;
    application.add_fallible_frame_system(update);
    Ok(())
}

/// Creates the complete initial 640x360 view and a stable reusable entity pool.
/// Allocation occurs only while preparing a world; frame drawing reuses it.
pub fn spawn(world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
    let mut canvas = Canvas::new();
    canvas
        .draw(&GameState::new())
        .map_err(|error| WorldBuildError::user(error.to_string()))?;
    for slot in 0..MAX_QUADS {
        let mut rectangle = ScreenRectangleVisual::new(
            LogicalScreenPosition::new(0.0, 0.0),
            LogicalScreenVector::new(1.0, 1.0),
            Color::TRANSPARENT,
        )
        .map_err(|error| WorldBuildError::user(error.to_string()))?;
        rectangle
            .set_draw_order_depth(slot as f32)
            .map_err(|error| WorldBuildError::user(error.to_string()))?;
        if let Some(&quad) = canvas.quads.get(slot) {
            let (position, size) = Letterbox::new(WIDTH, HEIGHT).geometry(quad);
            rectangle
                .set_geometry(position, size)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            rectangle
                .set_color(quad.color)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
        }
        world.spawn((QuadSlot(slot), rectangle))?;
    }
    world.insert_resource(canvas)?;
    Ok(())
}

/// Matches the fixed source pool to screen Scene and composed-frame budgets.
/// This game emits screen rectangles only, with no retained texture resources.
pub fn limits() -> RenderLimits {
    let scene = SceneBudget::new(
        MAX_QUADS,
        0,
        MAX_QUADS * 12,
        MAX_QUADS * 512,
        MAX_QUADS * 1024,
        MAX_QUADS * 1024,
        MAX_QUADS,
    );
    let frame = FrameLimits::new(2, MAX_QUADS, MAX_QUADS * 12, MAX_QUADS * 1024, 0, MAX_QUADS);
    RenderLimits::new(0, SceneBudget::new(0, 0, 0, 0, 0, 0, 0), frame)
        .with_max_world_rectangles(0)
        .with_max_world_lines(0)
        .with_max_screen_rectangles(MAX_QUADS)
        .with_screen_scene_budget(scene)
}

#[cfg(test)]
mod tests {
    #[allow(
        unused_imports,
        reason = "the harness-less benchmark includes this module with cfg(test)"
    )]
    use super::*;

    #[test]
    fn rectangle_budget_is_explicit_and_never_reallocates() {
        let mut canvas = Canvas::new();
        let pointer = canvas.quads.as_ptr();
        for _ in 0..MAX_QUADS {
            canvas.rectangle(0.0, 0.0, 1.0, 1.0, PAPER).unwrap();
        }
        assert_eq!(
            canvas.rectangle(0.0, 0.0, 1.0, 1.0, PAPER),
            Err(DrawError::BudgetExceeded)
        );
        assert_eq!(canvas.quads.len(), MAX_QUADS);
        assert_eq!(canvas.quads.as_ptr(), pointer);
        assert_eq!(
            canvas.rectangle(f32::NAN, 0.0, 1.0, 1.0, PAPER),
            Err(DrawError::InvalidGeometry)
        );
    }

    #[test]
    fn occluded_spans_stop_exactly_at_wall_columns() {
        let mut canvas = Canvas::new();
        canvas.wall_depth.fill(1.0);
        canvas.wall_depth[10..20].fill(3.0);
        canvas.wall_depth[25..30].fill(3.0);
        canvas
            .occluded_rectangle(0.0, 10.0, 100.0, 10.0, 2.0, RED)
            .unwrap();
        assert_eq!(canvas.quads.len(), 2);
        assert_eq!((canvas.quads[0].x, canvas.quads[0].width), (20.0, 20.0));
        assert_eq!((canvas.quads[1].x, canvas.quads[1].width), (50.0, 10.0));
    }

    #[test]
    fn reachable_views_and_near_sprites_fit_and_remain_finite() {
        let mut canvas = Canvas::new();
        let pointer = canvas.quads.as_ptr();
        let mut state = GameState::new();
        let mut peak = 0;
        for y in 0..level::HEIGHT {
            for x in 0..level::WIDTH {
                if level::cell(x as i32, y as i32) != 0 {
                    continue;
                }
                for angle in 0..16 {
                    state.player.position = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    state.player.angle = angle as f32 * std::f32::consts::TAU / 16.0;
                    canvas.draw(&state).unwrap();
                    peak = peak.max(canvas.quads.len());
                    assert!(canvas.quads.iter().all(|quad| quad.x >= 0.0
                        && quad.y >= 0.0
                        && quad.x + quad.width <= WIDTH
                        && quad.y + quad.height <= HEIGHT
                        && quad.width > 0.0
                        && quad.height > 0.0));
                }
            }
        }
        for phase in [Phase::Playing, Phase::Won, Phase::Dead] {
            state.phase = phase;
            state.muzzle = 0.1;
            state.hurt_flash = 0.15;
            state.player.position = Vec2::new(1.001, 1.001);
            for angle in 0..32 {
                state.player.angle = angle as f32 * std::f32::consts::TAU / 32.0;
                let forward = Vec2::new(state.player.angle.cos(), state.player.angle.sin());
                for (index, enemy) in state.enemies.iter_mut().enumerate() {
                    enemy.position =
                        state.player.position + forward * (0.061 + index as f32 * 0.08);
                }
                canvas.draw(&state).unwrap();
                peak = peak.max(canvas.quads.len());
            }
        }
        assert_eq!(canvas.quads.as_ptr(), pointer);
        eprintln!("Iron Maze presentation stress peak: {peak} / {MAX_QUADS} rectangles");
        assert!(peak < MAX_QUADS, "peak {peak}");
    }

    #[test]
    fn letterbox_geometry_is_valid_at_extreme_aspect_ratios() {
        let mut canvas = Canvas::new();
        canvas.draw(&GameState::new()).unwrap();
        for (width, height) in [
            (640.0, 360.0),
            (1920.0, 1080.0),
            (300.0, 900.0),
            (1.0, 1.0),
            (1.0, 65535.0),
            (65535.0, 1.0),
        ] {
            let mapping = Letterbox::new(width, height);
            for &quad in &canvas.quads {
                let (position, size) = mapping.geometry(quad);
                ScreenRectangleVisual::new(position, size, quad.color).unwrap();
            }
        }
    }
}

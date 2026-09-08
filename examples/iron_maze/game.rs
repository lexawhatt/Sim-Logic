//! Deterministic game rules; no rendering or operating-system input here.

use sim_logic::prelude::*;
use std::{error::Error, fmt};

use super::level;

pub const ENEMY_COUNT: usize = 6;
pub const PLAYER_RADIUS: f32 = 0.20;
pub const ENEMY_RADIUS: f32 = 0.26;
pub const PLAYER_SPEED: f32 = 2.65;
pub const SHOT_INTERVAL: f32 = 0.24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Playing,
    Won,
    Dead,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Player {
    pub position: Vec2,
    pub angle: f32,
    pub health: u32,
    pub ammo: u32,
    pub walk_phase: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Enemy {
    pub position: Vec2,
    pub health: u32,
    pub hit_flash: f32,
    pub attack_cooldown: f32,
}

impl Enemy {
    pub const fn new(position: Vec2) -> Self {
        Self {
            position,
            health: 3,
            hit_flash: 0.0,
            attack_cooldown: 0.8,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupKind {
    Health,
    Ammo,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pickup {
    pub position: Vec2,
    pub kind: PickupKind,
    pub active: bool,
}

/// One fixed tick's controls. Movement x is strafe, y is forward; turn is
/// clockwise and restricted to -1..1. The weapon repeats at its own cooldown.
#[derive(Debug, Clone, Copy, Default)]
pub struct Controls {
    pub movement: Vec2,
    pub turn: f32,
    pub shoot: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidStep;

impl fmt::Display for InvalidStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("game steps require finite controls and a delta between zero and 0.1 seconds")
    }
}
impl Error for InvalidStep {}

/// Bounded canonical state shared by desktop play and headless tests.
#[derive(Debug, Clone, PartialEq, Resource)]
pub struct GameState {
    pub player: Player,
    pub enemies: [Enemy; ENEMY_COUNT],
    pub pickups: [Pickup; 4],
    pub phase: Phase,
    pub ticks: u64,
    pub muzzle: f32,
    pub hurt_flash: f32,
    pub kills: u32,
    shot_cooldown: f32,
}

impl Default for GameState {
    fn default() -> Self {
        Self::new()
    }
}

impl GameState {
    pub fn new() -> Self {
        Self {
            player: Player {
                position: Vec2::new(2.5, 7.5),
                angle: 0.0,
                health: 100,
                ammo: 60,
                walk_phase: 0.0,
            },
            enemies: [
                Enemy::new(Vec2::new(6.5, 7.5)),
                Enemy::new(Vec2::new(12.5, 7.5)),
                Enemy::new(Vec2::new(4.5, 3.5)),
                Enemy::new(Vec2::new(11.5, 3.5)),
                Enemy::new(Vec2::new(5.5, 12.5)),
                Enemy::new(Vec2::new(12.5, 12.5)),
            ],
            pickups: [
                Pickup {
                    position: Vec2::new(2.5, 2.5),
                    kind: PickupKind::Health,
                    active: true,
                },
                Pickup {
                    position: Vec2::new(13.5, 2.5),
                    kind: PickupKind::Ammo,
                    active: true,
                },
                Pickup {
                    position: Vec2::new(2.5, 13.5),
                    kind: PickupKind::Ammo,
                    active: true,
                },
                Pickup {
                    position: Vec2::new(13.5, 13.5),
                    kind: PickupKind::Health,
                    active: true,
                },
            ],
            phase: Phase::Playing,
            ticks: 0,
            muzzle: 0.0,
            hurt_flash: 0.0,
            kills: 0,
            shot_cooldown: 0.0,
        }
    }

    /// Advances at most 100 ms, rejecting invalid inputs before any mutation.
    pub fn step(&mut self, controls: Controls, delta: f32) -> Result<(), InvalidStep> {
        if !delta.is_finite()
            || !(0.0..=0.1).contains(&delta)
            || !controls.movement.is_finite()
            || !controls.turn.is_finite()
        {
            return Err(InvalidStep);
        }
        if self.phase != Phase::Playing || delta == 0.0 {
            return Ok(());
        }
        self.ticks = self.ticks.saturating_add(1);
        self.muzzle = (self.muzzle - delta).max(0.0);
        self.hurt_flash = (self.hurt_flash - delta).max(0.0);
        self.shot_cooldown = (self.shot_cooldown - delta).max(0.0);
        for enemy in &mut self.enemies {
            enemy.hit_flash = (enemy.hit_flash - delta).max(0.0);
            enemy.attack_cooldown = (enemy.attack_cooldown - delta).max(0.0);
        }
        self.player.angle = (self.player.angle + controls.turn.clamp(-1.0, 1.0) * 2.15 * delta)
            .rem_euclid(std::f32::consts::TAU);
        let local = Vec2::new(
            controls.movement.x().clamp(-1.0, 1.0),
            controls.movement.y().clamp(-1.0, 1.0),
        )
        .normalized();
        let direction = Vec2::new(self.player.angle.cos(), self.player.angle.sin());
        let right = Vec2::new(-direction.y(), direction.x());
        let movement = (direction * local.y() + right * local.x()) * (PLAYER_SPEED * delta);
        let steps = ((movement.length() / 0.08).ceil() as u32).clamp(1, 4);
        let before = self.player.position;
        for _ in 0..steps {
            let candidate =
                level::slide(self.player.position, movement / steps as f32, PLAYER_RADIUS);
            if self.enemies.iter().all(|enemy| {
                enemy.health == 0
                    || (enemy.position - candidate).length() >= PLAYER_RADIUS + ENEMY_RADIUS
            }) {
                self.player.position = candidate;
            }
        }
        self.player.walk_phase = (self.player.walk_phase
            + (self.player.position - before).length() * 8.0)
            .rem_euclid(std::f32::consts::TAU);
        if controls.shoot && self.shot_cooldown == 0.0 && self.player.ammo > 0 {
            self.fire(direction);
        }
        self.update_enemies(delta);
        if self.player.health == 0 {
            self.phase = Phase::Dead;
            return Ok(());
        }
        self.collect_pickups();
        if self.kills as usize == ENEMY_COUNT
            && (self.player.position - level::EXIT).length() < 0.65
        {
            self.phase = Phase::Won;
        }
        Ok(())
    }

    fn fire(&mut self, direction: Vec2) {
        self.player.ammo -= 1;
        self.shot_cooldown = SHOT_INTERVAL;
        self.muzzle = 0.10;
        let wall =
            level::cast_ray(self.player.position, direction, 32.0).map_or(32.0, |hit| hit.distance);
        let mut nearest = wall;
        let mut target = None;
        for (index, enemy) in self.enemies.iter().enumerate() {
            if enemy.health == 0 {
                continue;
            }
            let relative = enemy.position - self.player.position;
            let forward = relative.dot(direction);
            let across = (relative.length_squared() - forward * forward).max(0.0);
            if forward <= 0.0 || across > ENEMY_RADIUS * ENEMY_RADIUS {
                continue;
            }
            let entry = (forward - (ENEMY_RADIUS * ENEMY_RADIUS - across).sqrt()).max(0.0);
            if entry < nearest {
                nearest = entry;
                target = Some(index);
            }
        }
        if let Some(index) = target {
            self.enemies[index].health -= 1;
            self.enemies[index].hit_flash = 0.14;
            if self.enemies[index].health == 0 {
                self.kills += 1;
            }
        }
    }

    fn update_enemies(&mut self, delta: f32) {
        for index in 0..ENEMY_COUNT {
            let enemy = self.enemies[index];
            if enemy.health == 0 {
                continue;
            }
            let offset = self.player.position - enemy.position;
            let distance = offset.length();
            if distance > 11.0 || !level::visible(enemy.position, self.player.position) {
                continue;
            }
            if distance > 0.72 {
                let proposed = level::slide(
                    enemy.position,
                    offset.normalized() * (0.68 * delta),
                    ENEMY_RADIUS,
                );
                if self.enemies.iter().enumerate().all(|(other, enemy)| {
                    other == index
                        || enemy.health == 0
                        || (enemy.position - proposed).length() >= ENEMY_RADIUS * 2.0
                }) && (self.player.position - proposed).length() >= PLAYER_RADIUS + ENEMY_RADIUS
                {
                    self.enemies[index].position = proposed;
                }
            } else if enemy.attack_cooldown == 0.0 {
                self.player.health = self.player.health.saturating_sub(12);
                self.hurt_flash = 0.22;
                self.enemies[index].attack_cooldown = 0.85;
            }
        }
    }

    fn collect_pickups(&mut self) {
        for pickup in &mut self.pickups {
            if !pickup.active || (pickup.position - self.player.position).length() > 0.48 {
                continue;
            }
            let useful = match pickup.kind {
                PickupKind::Health if self.player.health < 100 => {
                    self.player.health = (self.player.health + 35).min(100);
                    true
                }
                PickupKind::Ammo if self.player.ammo < 99 => {
                    self.player.ammo = (self.player.ammo + 24).min(99);
                    true
                }
                _ => false,
            };
            if useful {
                pickup.active = false;
            }
        }
    }
}

use crate::game_example::{game::*, level};
use sim_logic::prelude::*;

fn empty() -> GameState {
    let mut game = GameState::new();
    for enemy in &mut game.enemies {
        enemy.health = 0;
    }
    game
}

#[test]
fn movement_normalizes_diagonals_and_does_not_tunnel_through_map() -> LogicResult {
    let mut straight = empty();
    let mut diagonal = straight.clone();
    let start = straight.player.position;
    straight.step(
        Controls {
            movement: Vec2::Y,
            ..Default::default()
        },
        0.1,
    )?;
    diagonal.step(
        Controls {
            movement: Vec2::ONE,
            ..Default::default()
        },
        0.1,
    )?;
    assert!(((straight.player.position - start).length() - PLAYER_SPEED * 0.1).abs() < 0.00001);
    assert!(((diagonal.player.position - start).length() - PLAYER_SPEED * 0.1).abs() < 0.00001);
    for _ in 0..100 {
        straight.step(
            Controls {
                movement: Vec2::Y,
                ..Default::default()
            },
            0.1,
        )?;
        assert!(level::can_stand(straight.player.position, PLAYER_RADIUS));
    }
    assert!(straight.player.position.x() <= 15.0 - PLAYER_RADIUS);
    assert!(straight.player.position.x() > 14.6);
    Ok(())
}

#[test]
fn invalid_steps_leave_every_field_unchanged_and_idle_stays_finite() -> LogicResult {
    let original = empty();
    for delta in [-1.0, 0.11, f32::NAN, f32::INFINITY] {
        let mut game = original.clone();
        assert!(game.step(Controls::default(), delta).is_err());
        assert_eq!(game, original);
    }
    for controls in [
        Controls {
            movement: Vec2::new(f32::NAN, 0.0),
            ..Default::default()
        },
        Controls {
            turn: f32::INFINITY,
            ..Default::default()
        },
    ] {
        let mut game = original.clone();
        assert!(game.step(controls, 0.01).is_err());
        assert_eq!(game, original);
    }
    let mut idle = original.clone();
    for _ in 0..100 {
        idle.step(Controls::default(), 0.01)?;
    }
    assert_eq!(idle.player.position, original.player.position);
    assert!(idle.player.position.is_finite());
    Ok(())
}

#[test]
fn shots_hit_only_nearest_living_enemy_and_walls_block_them() -> LogicResult {
    let mut game = empty();
    game.enemies[0] = Enemy::new(Vec2::new(7.5, 7.5));
    game.enemies[1] = Enemy::new(Vec2::new(5.5, 7.5));
    game.step(
        Controls {
            shoot: true,
            ..Default::default()
        },
        0.01,
    )?;
    assert_eq!(game.enemies[0].health, 3);
    assert_eq!(game.enemies[1].health, 2);
    assert_eq!(game.player.ammo, 59);
    assert!(game.muzzle > 0.0);
    assert!(game.enemies[1].hit_flash > 0.0);

    let mut blocked = empty();
    blocked.player.position = Vec2::new(6.5, 6.5);
    blocked.enemies[0] = Enemy::new(Vec2::new(8.5, 6.5));
    let before = blocked.enemies[0].position;
    blocked.step(
        Controls {
            shoot: true,
            ..Default::default()
        },
        0.01,
    )?;
    assert_eq!(blocked.enemies[0].health, 3);
    assert_eq!(blocked.enemies[0].position, before);
    assert_eq!(blocked.player.health, 100);
    Ok(())
}

#[test]
fn held_fire_has_a_cooldown_and_death_stops_enemy_actions() -> LogicResult {
    let mut game = empty();
    game.enemies[0] = Enemy::new(Vec2::new(3.1, 7.5));
    game.enemies[0].health = 1;
    game.enemies[0].attack_cooldown = 0.0;
    game.step(
        Controls {
            shoot: true,
            ..Default::default()
        },
        0.01,
    )?;
    assert_eq!(game.kills, 1);
    assert_eq!(game.enemies[0].health, 0);
    assert_eq!(
        game.player.health, 100,
        "a killed enemy cannot attack in the same tick"
    );
    game.step(
        Controls {
            shoot: true,
            ..Default::default()
        },
        0.01,
    )?;
    assert_eq!(game.player.ammo, 59);
    for _ in 0..30 {
        game.step(
            Controls {
                shoot: true,
                ..Default::default()
            },
            0.01,
        )?;
    }
    assert_eq!(game.player.ammo, 58);
    assert_eq!(game.kills, 1);
    Ok(())
}

#[test]
fn enemy_melee_is_cooldown_limited_and_dead_game_freezes() -> LogicResult {
    let mut game = empty();
    game.enemies[0] = Enemy::new(Vec2::new(3.1, 7.5));
    game.enemies[0].attack_cooldown = 0.0;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.player.health, 88);
    for _ in 0..20 {
        game.step(Controls::default(), 0.01)?;
    }
    assert_eq!(game.player.health, 88);
    game.player.health = 12;
    game.enemies[0].attack_cooldown = 0.0;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.phase, Phase::Dead);
    assert_eq!(game.player.health, 0);
    let dead = game.clone();
    game.step(
        Controls {
            movement: Vec2::Y,
            shoot: true,
            turn: 1.0,
        },
        0.1,
    )?;
    assert_eq!(game, dead);
    Ok(())
}

#[test]
fn pickups_apply_once_only_when_useful_and_exit_requires_all_kills() -> LogicResult {
    let mut game = empty();
    game.player.position = game.pickups[0].position;
    game.step(Controls::default(), 0.01)?;
    assert!(game.pickups[0].active);
    game.player.health = 50;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.player.health, 85);
    assert!(!game.pickups[0].active);
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.player.health, 85);
    game.player.position = game.pickups[1].position;
    game.player.ammo = 99;
    game.step(Controls::default(), 0.01)?;
    assert!(game.pickups[1].active);
    game.player.ammo = 70;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.player.ammo, 94);
    assert!(!game.pickups[1].active);
    game.player.position = level::EXIT;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.phase, Phase::Playing);
    game.kills = ENEMY_COUNT as u32;
    game.step(Controls::default(), 0.01)?;
    assert_eq!(game.phase, Phase::Won);
    let won = game.clone();
    game.step(
        Controls {
            shoot: true,
            ..Default::default()
        },
        0.1,
    )?;
    assert_eq!(game, won);
    Ok(())
}

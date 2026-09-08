use crate::game_example::{game::*, level};
use sim_logic::prelude::*;
use std::collections::VecDeque;

#[test]
fn all_objectives_and_spawns_are_clear_and_connected() {
    let game = GameState::new();
    let mut reachable = [[false; level::WIDTH]; level::HEIGHT];
    let mut queue = VecDeque::from([(2, 7)]);
    while let Some((x, y)) = queue.pop_front() {
        if level::cell(x, y) != 0 || reachable[y as usize][x as usize] {
            continue;
        }
        reachable[y as usize][x as usize] = true;
        queue.extend([(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]);
    }
    for position in game
        .enemies
        .iter()
        .map(|enemy| enemy.position)
        .chain(game.pickups.iter().map(|pickup| pickup.position))
        .chain([game.player.position, level::EXIT])
    {
        assert!(level::can_stand(position, ENEMY_RADIUS), "{position:?}");
        assert!(
            reachable[position.y() as usize][position.x() as usize],
            "{position:?}"
        );
    }
    for x in 0..level::WIDTH as i32 {
        assert_ne!(level::cell(x, 0), 0);
        assert_ne!(level::cell(x, level::HEIGHT as i32 - 1), 0);
    }
    for y in 0..level::HEIGHT as i32 {
        assert_ne!(level::cell(0, y), 0);
        assert_ne!(level::cell(level::WIDTH as i32 - 1, y), 0);
    }
}

#[test]
fn ray_distance_preserves_camera_plane_parameter_and_range() {
    let origin = Vec2::new(2.5, 7.5);
    let hit = level::cast_ray(origin, Vec2::X, 32.0).unwrap();
    assert_eq!(hit.distance, 12.5);
    assert_eq!(hit.side, 0);
    assert_eq!(hit.texture_u, 0.5);
    assert_eq!(
        level::cast_ray(origin, Vec2::X * 2.0, 32.0)
            .unwrap()
            .distance,
        6.25
    );
    assert!(level::cast_ray(origin, Vec2::X, 12.4).is_none());
    assert!(level::cast_ray(origin, Vec2::X, 12.5).is_some());
    assert_eq!(
        level::cast_ray(Vec2::new(0.5, 0.5), Vec2::X, 32.0)
            .unwrap()
            .distance,
        0.0
    );
}

#[test]
fn rays_cannot_pass_between_corner_touching_wall_cells() {
    let origin = Vec2::new(2.5, 4.5);
    let hit = level::cast_ray(origin, Vec2::ONE, 32.0).unwrap();
    assert_eq!(hit.distance, 0.5);
    assert_eq!(hit.side, 1);
    assert!(!level::visible(origin, Vec2::new(3.5, 5.5)));
    assert!(!level::visible(Vec2::new(6.5, 6.5), Vec2::new(8.5, 6.5)));
}

#[test]
fn invalid_rays_and_unbounded_clearance_requests_fail_closed() {
    for direction in [
        Vec2::ZERO,
        Vec2::new(f32::NAN, 0.0),
        Vec2::new(0.0, f32::INFINITY),
    ] {
        assert!(level::cast_ray(Vec2::new(2.5, 7.5), direction, 32.0).is_none());
    }
    for maximum in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(level::cast_ray(Vec2::new(2.5, 7.5), Vec2::X, maximum).is_none());
    }
    assert!(!level::can_stand(Vec2::new(f32::MAX, 1.0), 0.2));
    assert!(!level::can_stand(Vec2::new(2.5, 7.5), 1_000.0));
    assert!(!level::can_stand(Vec2::new(2.5, 5.5), 0.2));
    assert!(!level::can_stand(Vec2::new(1.1, 7.5), 0.2));
    assert!(level::can_stand(Vec2::new(1.21, 7.5), 0.2));
}

//! A small original map and bounded grid geometry for Iron Maze.

use sim_logic::prelude::Vec2;

pub const WIDTH: usize = 16;
pub const HEIGHT: usize = 16;
pub const EXIT: Vec2 = Vec2::new(13.5, 14.5);

pub const MAP: [&[u8; WIDTH]; HEIGHT] = [
    b"1111111111111111",
    b"1......1.......1",
    b"1..2...1..3....1",
    b"1..2......3....1",
    b"1......1.......1",
    b"111.1111.111.111",
    b"1......1.......1",
    b"1..............1",
    b"1......1.......1",
    b"1.111..1..111..1",
    b"1......1.......1",
    b"1..2.......3...1",
    b"1..2...1...3...1",
    b"1......1.......1",
    b"1..............1",
    b"1111111111111111",
];

/// Returns zero for floor, wall material 1..3 otherwise; outside is solid.
pub fn cell(x: i32, y: i32) -> u8 {
    if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 {
        return 1;
    }
    match MAP[y as usize][x as usize] {
        b'1' => 1,
        b'2' => 2,
        b'3' => 3,
        _ => 0,
    }
}

/// A wall intersection. Distance is the parameter along the supplied ray,
/// not its Euclidean length unless the direction was normalized.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RayHit {
    pub distance: f32,
    pub tile: u8,
    /// Zero for an x boundary, one for a y boundary.
    pub side: u8,
    pub texture_u: f32,
}

/// Finds the nearest wall using at most WIDTH+HEIGHT+4 grid crossings.
/// Invalid rays return None; a ray starting inside a wall hits at distance zero.
pub fn cast_ray(origin: Vec2, direction: Vec2, max_distance: f32) -> Option<RayHit> {
    if !origin.is_finite()
        || !direction.is_finite()
        || direction == Vec2::ZERO
        || !max_distance.is_finite()
        || max_distance <= 0.0
    {
        return None;
    }
    let mut x = origin.x().floor() as i32;
    let mut y = origin.y().floor() as i32;
    let hit = |distance: f32, tile: u8, side: u8| {
        let point = origin + direction * distance;
        RayHit {
            distance,
            tile,
            side,
            texture_u: if side == 0 { point.y() } else { point.x() }.rem_euclid(1.0),
        }
    };
    if cell(x, y) != 0 {
        return Some(hit(0.0, cell(x, y), 0));
    }
    let (step_x, delta_x, mut side_x) = axis_crossing(origin.x(), direction.x());
    let (step_y, delta_y, mut side_y) = axis_crossing(origin.y(), direction.y());
    for _ in 0..WIDTH + HEIGHT + 4 {
        let distance = side_x.min(side_y);
        if !distance.is_finite() || distance > max_distance {
            return None;
        }
        // At an exact corner, either adjacent solid cell blocks the ray.
        // This prevents shooting diagonally through two touching walls.
        if (side_x - side_y).abs() <= 0.000_001 {
            let horizontal = cell(x + step_x, y);
            let vertical = cell(x, y + step_y);
            if horizontal != 0 {
                return Some(hit(distance, horizontal, 0));
            }
            if vertical != 0 {
                return Some(hit(distance, vertical, 1));
            }
            x += step_x;
            y += step_y;
            side_x += delta_x;
            side_y += delta_y;
            if cell(x, y) != 0 {
                return Some(hit(distance, cell(x, y), 0));
            }
        } else if side_x < side_y {
            x += step_x;
            side_x += delta_x;
            if cell(x, y) != 0 {
                return Some(hit(distance, cell(x, y), 0));
            }
        } else {
            y += step_y;
            side_y += delta_y;
            if cell(x, y) != 0 {
                return Some(hit(distance, cell(x, y), 1));
            }
        }
    }
    None
}

fn axis_crossing(position: f32, direction: f32) -> (i32, f32, f32) {
    if direction > 0.0 {
        let delta = direction.recip();
        (1, delta, (position.floor() + 1.0 - position) * delta)
    } else if direction < 0.0 {
        let delta = -direction.recip();
        (-1, delta, (position - position.floor()) * delta)
    } else {
        (0, f32::INFINITY, f32::INFINITY)
    }
}

/// Circle-versus-map clearance. Only small finite bodies inside the map are
/// accepted, keeping the cell scan bounded independently of caller values.
pub fn can_stand(position: Vec2, radius: f32) -> bool {
    if !position.is_finite()
        || !radius.is_finite()
        || !(0.0..=0.49).contains(&radius)
        || position.x() < radius
        || position.y() < radius
        || position.x() > WIDTH as f32 - radius
        || position.y() > HEIGHT as f32 - radius
    {
        return false;
    }
    for y in (position.y() - radius).floor() as i32..=(position.y() + radius).floor() as i32 {
        for x in (position.x() - radius).floor() as i32..=(position.x() + radius).floor() as i32 {
            if cell(x, y) == 0 {
                continue;
            }
            let nearest = Vec2::new(
                position.x().clamp(x as f32, x as f32 + 1.0),
                position.y().clamp(y as f32, y as f32 + 1.0),
            );
            if (position - nearest).length_squared() < radius * radius || radius == 0.0 {
                return false;
            }
        }
    }
    true
}

/// Slides a small body along walls. Callers supply a movement of at most one
/// tenth of a cell per axis, after bounded substepping in the simulation.
pub fn slide(position: Vec2, delta: Vec2, radius: f32) -> Vec2 {
    let horizontal = Vec2::new(position.x() + delta.x(), position.y());
    let next = if can_stand(horizontal, radius) {
        horizontal
    } else {
        position
    };
    let vertical = Vec2::new(next.x(), next.y() + delta.y());
    if can_stand(vertical, radius) {
        vertical
    } else {
        next
    }
}

/// Walls block sight even when a target is inside the same rendered column.
pub fn visible(from: Vec2, to: Vec2) -> bool {
    let delta = to - from;
    let distance = delta.length();
    distance.is_finite()
        && (distance < 0.0001 || cast_ray(from, delta / distance, distance).is_none())
}

use super::{Region, terrain::WORLD_LIMIT};

/// Camera-space movement values in [-1, 1]. A jump is a queued press, not held input.
#[derive(Clone, Copy, Debug, Default)]
pub struct Movement {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
}

/// Feet-centered grounded controller; yaw zero looks along -Z, pitch positive up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Player {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    vertical_velocity: f32,
}

impl Player {
    pub const RADIUS: f32 = 0.28;
    pub const HEIGHT: f32 = 1.75;
    pub const EYE_HEIGHT: f32 = 1.58;
    pub const REACH: f32 = 6.0;
    /// Maximum feet altitude in world units; creative flight respects this ceiling.
    pub const MAX_ALTITUDE: f32 = 64.0;

    pub fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            yaw: 0.0,
            pitch: -0.18,
            grounded: false,
            vertical_velocity: 0.0,
        }
    }

    pub fn eye(&self) -> [f32; 3] {
        [
            self.position[0],
            self.position[1] + Self::EYE_HEIGHT,
            self.position[2],
        ]
    }

    pub fn forward(&self) -> [f32; 3] {
        let horizontal = self.pitch.cos();
        [
            self.yaw.sin() * horizontal,
            self.pitch.sin(),
            -self.yaw.cos() * horizontal,
        ]
    }

    pub fn turn(&mut self, yaw_delta: f32, pitch_delta: f32) {
        if yaw_delta.is_finite() && pitch_delta.is_finite() {
            let tau = std::f32::consts::TAU;
            self.yaw = (self.yaw.rem_euclid(tau) + yaw_delta.rem_euclid(tau)).rem_euclid(tau);
            self.pitch = (self.pitch + pitch_delta.clamp(-2.9, 2.9)).clamp(-1.45, 1.45);
        }
    }

    pub fn stop(&mut self) {
        self.vertical_velocity = 0.0;
        self.grounded = false;
    }

    /// Ignore invalid input and steps outside [0, 0.1] seconds without mutation.
    /// Normalized movement avoids diagonal speedup; small bounded substeps stop tunneling.
    pub fn step(&mut self, region: &Region, movement: Movement, seconds: f32) {
        if !seconds.is_finite()
            || !(0.0..=0.1).contains(&seconds)
            || !movement.forward.is_finite()
            || !movement.strafe.is_finite()
        {
            return;
        }
        let mut forward = movement.forward.clamp(-1.0, 1.0);
        let mut strafe = movement.strafe.clamp(-1.0, 1.0);
        let length = forward.hypot(strafe).max(1.0);
        forward /= length;
        strafe /= length;
        if movement.jump && self.grounded {
            self.vertical_velocity = 8.0;
        }
        self.vertical_velocity = (self.vertical_velocity - 24.0 * seconds).max(-30.0);
        let motion = [
            (self.yaw.sin() * forward + self.yaw.cos() * strafe) * 4.5 * seconds,
            self.vertical_velocity * seconds,
            (-self.yaw.cos() * forward + self.yaw.sin() * strafe) * 4.5 * seconds,
        ];
        self.move_bounded(region, motion);
    }

    /// Creative flight uses ordinary collisions, normalized three-axis motion
    /// and no gravity. Invalid inputs leave the player unchanged.
    pub fn step_flying(
        &mut self,
        region: &Region,
        movement: Movement,
        vertical: f32,
        seconds: f32,
    ) -> bool {
        if !seconds.is_finite()
            || !(0.0..=0.1).contains(&seconds)
            || ![movement.forward, movement.strafe, vertical, self.yaw]
                .into_iter()
                .all(f32::is_finite)
            || !self.position.into_iter().all(f32::is_finite)
        {
            return false;
        }
        let forward = movement.forward.clamp(-1.0, 1.0);
        let strafe = movement.strafe.clamp(-1.0, 1.0);
        let vertical = vertical.clamp(-1.0, 1.0);
        let length = forward.hypot(strafe).hypot(vertical).max(1.0);
        let speed = 7.0 * seconds / length;
        self.vertical_velocity = 0.0;
        self.move_bounded(
            region,
            [
                (self.yaw.sin() * forward + self.yaw.cos() * strafe) * speed,
                vertical * speed,
                (-self.yaw.cos() * forward + self.yaw.sin() * strafe) * speed,
            ],
        );
        true
    }

    fn move_bounded(&mut self, region: &Region, motion: [f32; 3]) {
        let steps = (motion.into_iter().map(f32::abs).fold(0.0, f32::max) / 0.12)
            .ceil()
            .max(1.0) as usize;
        let delta = motion.map(|value| value / steps as f32);
        self.grounded = false;
        for _ in 0..steps {
            for axis in [0, 2, 1] {
                let old = self.position[axis];
                self.position[axis] += delta[axis];
                if self.position[1] > Self::MAX_ALTITUDE || self.collides(region) {
                    // Stop at the surface rather than retaining a whole substep
                    // of visible hovering space. Ten probes bound the work and
                    // leave less than 0.00012 blocks of separation.
                    let mut free = 0.0;
                    let mut blocked = 1.0;
                    for _ in 0..10 {
                        let fraction = (free + blocked) * 0.5;
                        self.position[axis] = old + delta[axis] * fraction;
                        if self.position[1] > Self::MAX_ALTITUDE || self.collides(region) {
                            blocked = fraction;
                        } else {
                            free = fraction;
                        }
                    }
                    self.position[axis] = old + delta[axis] * free;
                    if axis == 1 {
                        if delta[axis] < 0.0 {
                            self.grounded = true;
                        }
                        self.vertical_velocity = 0.0;
                    }
                }
            }
        }
    }

    pub fn overlaps(&self, cell: [i32; 3]) -> bool {
        let min = [
            self.position[0] - Self::RADIUS,
            self.position[1],
            self.position[2] - Self::RADIUS,
        ];
        let max = [
            self.position[0] + Self::RADIUS,
            self.position[1] + Self::HEIGHT,
            self.position[2] + Self::RADIUS,
        ];
        (0..3).all(|axis| min[axis] < cell[axis] as f32 + 1.0 && max[axis] > cell[axis] as f32)
    }

    pub fn collides(&self, region: &Region) -> bool {
        let min = [
            (self.position[0] - Self::RADIUS).floor() as i32,
            self.position[1].floor() as i32,
            (self.position[2] - Self::RADIUS).floor() as i32,
        ];
        let max = [
            (self.position[0] + Self::RADIUS - 0.00001).floor() as i32,
            (self.position[1] + Self::HEIGHT - 0.00001).floor() as i32,
            (self.position[2] + Self::RADIUS - 0.00001).floor() as i32,
        ];
        for y in min[1]..=max[1] {
            for z in min[2]..=max[2] {
                for x in min[0]..=max[0] {
                    if region.collision_cell([x, y, z]) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub(super) fn valid(&self, region: &Region) -> bool {
        self.position.into_iter().all(f32::is_finite)
            && self.yaw.is_finite()
            && self.pitch.is_finite()
            && (-1.45..=1.45).contains(&self.pitch)
            && (-WORLD_LIMIT as f32 + Self::RADIUS..=WORLD_LIMIT as f32 - Self::RADIUS)
                .contains(&self.position[0])
            && (-WORLD_LIMIT as f32 + Self::RADIUS..=WORLD_LIMIT as f32 - Self::RADIUS)
                .contains(&self.position[2])
            && (0.0..=Self::MAX_ALTITUDE).contains(&self.position[1])
            && !self.collides(region)
    }
}

/// First solid cell and an adjacent placement cell; origin-inside has no face.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub cell: [i32; 3],
    pub adjacent: Option<[i32; 3]>,
    pub distance: f32,
}

/// Bounded voxel traversal. Zero/non-finite directions or reach outside [0, 6]
/// return None. Tied crossings advance together, skipping zero-volume contacts.
pub fn raycast(region: &Region, origin: [f32; 3], direction: [f32; 3], reach: f32) -> Option<Hit> {
    if !origin
        .into_iter()
        .all(|value| value.is_finite() && value.abs() <= WORLD_LIMIT as f32 + 8.0)
        || !direction.into_iter().all(f32::is_finite)
        || !reach.is_finite()
        || !(0.0..=Player::REACH).contains(&reach)
    {
        return None;
    }
    let length = direction
        .into_iter()
        .map(|value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    if length <= f64::EPSILON {
        return None;
    }
    let direction = direction.map(|value| (f64::from(value) / length) as f32);
    let mut cell = origin.map(|value| value.floor() as i32);
    let mut next = [f32::INFINITY; 3];
    let mut stride = [f32::INFINITY; 3];
    let mut signs = [0; 3];
    for axis in 0..3 {
        if direction[axis] > 0.0 {
            signs[axis] = 1;
            next[axis] = (cell[axis] as f32 + 1.0 - origin[axis]) / direction[axis];
            stride[axis] = 1.0 / direction[axis];
        } else if direction[axis] < 0.0 {
            signs[axis] = -1;
            next[axis] = (origin[axis] - cell[axis] as f32) / -direction[axis];
            stride[axis] = 1.0 / -direction[axis];
        }
    }
    let mut distance = 0.0;
    let mut adjacent = None;
    for _ in 0..128 {
        if region.get(cell).solid() {
            return Some(Hit {
                cell,
                adjacent,
                distance,
            });
        }
        distance = next.into_iter().fold(f32::INFINITY, f32::min);
        if distance > reach || !distance.is_finite() {
            return None;
        }
        let face = (0..3).find(|&axis| next[axis] == distance)?;
        for axis in 0..3 {
            if next[axis] == distance {
                cell[axis] += signs[axis];
                next[axis] += stride[axis];
            }
        }
        let mut neighbor = cell;
        neighbor[face] -= signs[face];
        adjacent = Some(neighbor);
    }
    None
}

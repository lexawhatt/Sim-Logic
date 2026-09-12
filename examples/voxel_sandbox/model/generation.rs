//! Version-2 deterministic terrain sampled independently of loaded render chunks.

use super::{Block, RegionId};

pub(super) fn sample(kind: RegionId, seed: u64, [x, y, z]: [i32; 3]) -> Block {
    if y == 0 {
        return Block::Stone;
    }
    let surface_height = height(kind, seed, x, z);
    if y < surface_height {
        if y == surface_height - 1 {
            return if kind == RegionId::Meadow {
                Block::Grass
            } else {
                Block::Sand
            };
        }
        if y == surface_height - 2 && kind == RegionId::Meadow {
            return Block::Dirt;
        }
        let ore = hash(seed ^ y as u64, x, z) % 97;
        return match ore {
            0 => Block::CoalOre,
            1 => Block::IronOre,
            2 => Block::CopperOre,
            _ => Block::Stone,
        };
    }
    // Stable sparse landmarks in 16x16 districts, including negative coordinates.
    // No chunk residency or generation order participates in the rules.
    let district_x = x.div_euclid(16);
    let district_z = z.div_euclid(16);
    let center_x = district_x * 16 + 4;
    let center_z = district_z * 16 + 5;
    let base = height(kind, seed, center_x, center_z);
    if kind == RegionId::Meadow {
        if x == center_x && z == center_z && y < base + 3 {
            return Block::Wood;
        }
        if (x - center_x).abs() <= 1 && (z - center_z).abs() <= 1 && y == base + 3 {
            return Block::Leaves;
        }
        if x == center_x && z == center_z && y == base + 4 {
            return Block::Leaves;
        }
    } else if x == center_x && z == center_z && y < base + 3 {
        return Block::Sandstone;
    }
    Block::Air
}

fn height(kind: RegionId, seed: u64, x: i32, z: i32) -> i32 {
    match kind {
        RegionId::Meadow => 3 + (hash(seed, x.div_euclid(4), z.div_euclid(4)) % 2) as i32,
        RegionId::Canyon => 3 + (hash(seed ^ 0xcafe, x.div_euclid(3), z.div_euclid(3)) % 4) as i32,
    }
}

fn hash(seed: u64, x: i32, z: i32) -> u64 {
    let value = seed
        ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (z as u64).wrapping_mul(0x517cc1b727220a95);
    let value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value ^ (value >> 27)
}

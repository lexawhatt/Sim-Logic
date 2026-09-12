use super::{Block, Player, RegionId};

pub const WIDTH: i32 = 24;
pub const HEIGHT: i32 = 16;
pub const CHUNK_SIZE: i32 = 8;
pub const CHUNK_COUNT: usize = 18;
pub(super) const CELL_COUNT: usize = (WIDTH * HEIGHT * WIDTH) as usize;

/// Fixed-size canonical block storage and chunk revisions. No renderer state.
#[derive(Clone, Debug)]
pub struct Region {
    pub(super) blocks: Vec<Block>,
    revisions: [u64; CHUNK_COUNT],
}

impl Region {
    pub(super) fn generated(kind: RegionId, seed: u64) -> Self {
        let mut region = Self {
            blocks: vec![Block::Air; CELL_COUNT],
            revisions: [1; CHUNK_COUNT],
        };
        for z in 0..WIDTH {
            for x in 0..WIDTH {
                let height = match kind {
                    RegionId::Meadow => 3 + i32::from(noise(seed, x / 4, z / 4) % 2),
                    RegionId::Canyon => 3 + i32::from(noise(seed ^ 0xcafe, x / 3, z / 3) % 4),
                };
                for y in 0..height {
                    let block = if y == height - 1 {
                        match kind {
                            RegionId::Meadow => Block::Grass,
                            RegionId::Canyon => Block::Sand,
                        }
                    } else {
                        Block::Stone
                    };
                    region.blocks[index([x, y, z])] = block;
                }
            }
        }
        // Keep the central spawn platform clear; scenery is deliberately bounded.
        for (x, z) in [(4, 5), (18, 6), (6, 18), (19, 19)] {
            let base = region.surface_height(x, z);
            if kind == RegionId::Meadow {
                for y in base..base + 3 {
                    region.blocks[index([x, y, z])] = Block::Wood;
                }
                for dx in -1..=1 {
                    for dz in -1..=1 {
                        region.blocks[index([x + dx, base + 3, z + dz])] = Block::Leaves;
                    }
                }
                region.blocks[index([x, base + 4, z])] = Block::Leaves;
            } else {
                for y in base..base + 3 {
                    region.blocks[index([x, y, z])] = Block::Stone;
                }
            }
        }
        region
    }

    pub const fn contains([x, y, z]: [i32; 3]) -> bool {
        x >= 0 && x < WIDTH && y >= 0 && y < HEIGHT && z >= 0 && z < WIDTH
    }

    /// Outside terrain is air for rendering and ray picking.
    pub fn get(&self, cell: [i32; 3]) -> Block {
        if Self::contains(cell) {
            self.blocks[index(cell)]
        } else {
            Block::Air
        }
    }

    /// Returns a monotonically changing revision for a known chunk.
    pub fn chunk_revision(&self, chunk: usize) -> Option<u64> {
        self.revisions.get(chunk).copied()
    }

    #[cfg(test)]
    pub fn solid_count(&self) -> usize {
        self.blocks.iter().filter(|block| block.solid()).count()
    }

    pub fn surface_height(&self, x: i32, z: i32) -> i32 {
        (0..HEIGHT)
            .rev()
            .find(|&y| self.get([x, y, z]).solid())
            .map_or(0, |y| y + 1)
    }

    pub fn spawn(&self) -> Player {
        Player::at([12.5, self.surface_height(12, 12) as f32 + 0.02, 12.5])
    }

    /// Bounds are solid to the controller but not to meshing or ray picking.
    pub(super) fn collision_cell(&self, cell: [i32; 3]) -> bool {
        cell[0] < 0
            || cell[0] >= WIDTH
            || cell[2] < 0
            || cell[2] >= WIDTH
            || cell[1] < 0
            || self.get(cell).solid()
    }

    pub(super) fn set(&mut self, cell: [i32; 3], block: Block) {
        if !Self::contains(cell) || self.get(cell) == block {
            return;
        }
        self.blocks[index(cell)] = block;
        let own = chunk_index(cell);
        self.bump(own);
        // Boundary face exposure belongs to the neighboring chunk as well.
        for axis in 0..3 {
            for direction in [-1, 1] {
                let mut neighbor = cell;
                neighbor[axis] += direction;
                if Self::contains(neighbor) {
                    let chunk = chunk_index(neighbor);
                    if chunk != own {
                        self.bump(chunk);
                    }
                }
            }
        }
    }

    fn bump(&mut self, chunk: usize) {
        // Wrap is practically unreachable; never use zero, reserved for "not built".
        self.revisions[chunk] = self.revisions[chunk].wrapping_add(1).max(1);
    }

    pub(super) fn from_blocks(blocks: Vec<Block>) -> Self {
        Self {
            blocks,
            revisions: [1; CHUNK_COUNT],
        }
    }
}

pub(super) fn index([x, y, z]: [i32; 3]) -> usize {
    (x + WIDTH * (z + WIDTH * y)) as usize
}

pub(super) fn chunk_index([x, y, z]: [i32; 3]) -> usize {
    (x / CHUNK_SIZE + 3 * (z / CHUNK_SIZE + 3 * (y / CHUNK_SIZE))) as usize
}

pub(super) fn chunk_origin(chunk: usize) -> [i32; 3] {
    [
        (chunk % 3) as i32 * CHUNK_SIZE,
        (chunk / 9) as i32 * CHUNK_SIZE,
        (chunk / 3 % 3) as i32 * CHUNK_SIZE,
    ]
}

fn noise(seed: u64, x: i32, z: i32) -> u8 {
    let value = seed
        ^ (x as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (z as u64).wrapping_mul(0x517cc1b727220a95);
    let value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    (value ^ (value >> 27)) as u8
}

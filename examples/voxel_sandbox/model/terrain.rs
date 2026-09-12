use std::collections::BTreeMap;

use super::{Block, EditError, Player, RegionId, generation};

/// Legacy initial 3x3 area width; horizontal streaming extends in both directions.
#[cfg(test)]
pub const WIDTH: i32 = 24;
pub const HEIGHT: i32 = 16;
pub const CHUNK_SIZE: i32 = 8;
/// The bounded initial bootstrap, retaining its historical chunk identities.
pub const CHUNK_COUNT: usize = 18;
pub const MAX_RESIDENT_CHUNKS: usize = 50;
pub const WORLD_LIMIT: i32 = 4096;
pub const MAX_EDITS: usize = 16_384;
pub const GENERATIONS_PER_FRAME: usize = 2;
#[cfg(test)]
pub(super) const CELL_COUNT: usize = (WIDTH * HEIGHT * WIDTH) as usize;
const CHUNK_CELLS: usize = 512;
const GRID: usize = (WORLD_LIMIT * 2 / CHUNK_SIZE) as usize;

#[derive(Clone, Debug)]
struct Chunk {
    blocks: Vec<Block>,
    revision: u64,
}

/// Canonical procedural rules plus bounded sparse edits; loaded chunks are a cache.
#[derive(Clone, Debug)]
pub struct Region {
    pub(super) kind: RegionId,
    pub(super) seed: u64,
    pub(super) edits: BTreeMap<[i32; 3], Block>,
    chunks: BTreeMap<usize, Chunk>,
    next_revision: u64,
    generated: u64,
    desired: [usize; MAX_RESIDENT_CHUNKS],
    desired_count: usize,
    #[cfg(test)]
    fixture: Option<Vec<Block>>,
}

impl Region {
    pub(super) fn generated(kind: RegionId, seed: u64) -> Self {
        let mut region = Self {
            kind,
            seed,
            edits: BTreeMap::new(),
            chunks: BTreeMap::new(),
            next_revision: 1,
            generated: 0,
            desired: [usize::MAX; MAX_RESIDENT_CHUNKS],
            desired_count: 0,
            #[cfg(test)]
            fixture: None,
        };
        // Fixed bounded bootstrap before the runner starts; later frame work is
        // capped separately. Other coordinates are never eagerly allocated.
        for id in 0..CHUNK_COUNT {
            region
                .generate_chunk(id)
                .expect("bounded fresh region bootstrap");
        }
        region
    }

    pub const fn contains([x, y, z]: [i32; 3]) -> bool {
        x >= -WORLD_LIMIT
            && x < WORLD_LIMIT
            && z >= -WORLD_LIMIT
            && z < WORLD_LIMIT
            && y >= 0
            && y < HEIGHT
    }

    /// Procedural collision/picking remain defined before a chunk is rendered.
    pub fn get(&self, cell: [i32; 3]) -> Block {
        if !Self::contains(cell) {
            return Block::Air;
        }
        if let Some(chunk) = self.chunks.get(&chunk_index(cell)) {
            return chunk.blocks[local_index(cell)];
        }
        self.canonical(cell)
    }

    fn canonical(&self, cell: [i32; 3]) -> Block {
        self.edits
            .get(&cell)
            .copied()
            .unwrap_or_else(|| self.base(cell))
    }

    pub(super) fn base(&self, cell: [i32; 3]) -> Block {
        if !Self::contains(cell) {
            return Block::Air;
        }
        #[cfg(test)]
        if let Some(blocks) = &self.fixture {
            if cell[0] >= 0 && cell[0] < WIDTH && cell[2] >= 0 && cell[2] < WIDTH {
                return blocks[index(cell)];
            }
            return Block::Air;
        }
        generation::sample(self.kind, self.seed, cell)
    }

    pub fn chunk_revision(&self, chunk: usize) -> Option<u64> {
        self.chunks.get(&chunk).map(|chunk| chunk.revision)
    }
    pub fn resident_chunks(&self) -> impl Iterator<Item = usize> + '_ {
        self.chunks.keys().copied()
    }
    pub fn resident_count(&self) -> usize {
        self.chunks.len()
    }
    pub fn pending_chunks(&self) -> usize {
        self.desired[..self.desired_count]
            .iter()
            .filter(|id| !self.chunks.contains_key(id))
            .count()
    }
    pub fn generated_chunks(&self) -> u64 {
        self.generated
    }
    pub fn edit_count(&self) -> usize {
        self.edits.len()
    }

    /// Desired nearest-first mesh order; no allocation in a warmed frame.
    pub fn desired_chunks(&self) -> &[usize] {
        &self.desired[..self.desired_count]
    }

    /// Keep five by five signed columns and both height layers; generate at most two.
    /// Eviction discards derived blocks only, never the sparse edit map.
    pub fn stream_around(&mut self, position: [f32; 3]) -> Result<(), EditError> {
        if !position.into_iter().all(f32::is_finite) {
            return Err(EditError::OutOfBounds);
        }
        if position[0].abs() >= WORLD_LIMIT as f32 || position[2].abs() >= WORLD_LIMIT as f32 {
            return Err(EditError::OutOfBounds);
        }
        // div_euclid is required before zero: -0.5 belongs to chunk -1.
        let center = [
            (position[0].floor() as i32).div_euclid(CHUNK_SIZE),
            (position[2].floor() as i32).div_euclid(CHUNK_SIZE),
        ];
        let mut count = 0;
        for ring in 0_i32..=2 {
            for dz in -ring..=ring {
                for dx in -ring..=ring {
                    if dx.abs().max(dz.abs()) != ring {
                        continue;
                    }
                    for y in 0..2 {
                        let coord = [center[0] + dx, y, center[1] + dz];
                        let origin = coord.map(|axis| axis * CHUNK_SIZE);
                        if !Self::contains(origin) {
                            continue;
                        }
                        self.desired[count] = chunk_index(origin);
                        count += 1;
                    }
                }
            }
        }
        self.desired_count = count;
        self.chunks
            .retain(|id, _| self.desired[..count].contains(id));
        let mut generated = 0;
        for index in 0..count {
            let id = self.desired[index];
            if !self.chunks.contains_key(&id) && generated < GENERATIONS_PER_FRAME {
                self.generate_chunk(id)?;
                generated += 1;
            }
        }
        Ok(())
    }

    fn generate_chunk(&mut self, id: usize) -> Result<(), EditError> {
        let Some(origin) = chunk_origin(id) else {
            return Err(EditError::OutOfBounds);
        };
        let revision = self.issue_revision()?;
        let mut blocks = Vec::with_capacity(CHUNK_CELLS);
        for y in origin[1]..origin[1] + CHUNK_SIZE {
            for z in origin[2]..origin[2] + CHUNK_SIZE {
                for x in origin[0]..origin[0] + CHUNK_SIZE {
                    blocks.push(self.canonical([x, y, z]));
                }
            }
        }
        self.chunks.insert(id, Chunk { blocks, revision });
        self.generated = self.generated.saturating_add(1);
        Ok(())
    }

    fn issue_revision(&mut self) -> Result<u64, EditError> {
        let revision = self.next_revision;
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or(EditError::RevisionExhausted)?;
        Ok(revision)
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
    pub(super) fn collision_cell(&self, cell: [i32; 3]) -> bool {
        cell[0] < -WORLD_LIMIT
            || cell[0] >= WORLD_LIMIT
            || cell[2] < -WORLD_LIMIT
            || cell[2] >= WORLD_LIMIT
            || cell[1] < 0
            || self.get(cell).solid()
    }

    pub(super) fn try_set(&mut self, cell: [i32; 3], block: Block) -> Result<(), EditError> {
        if !Self::contains(cell) {
            return Err(EditError::OutOfBounds);
        }
        if self.get(cell) == block {
            return Ok(());
        }
        if cell[1] == 0 && block != Block::Stone {
            return Err(EditError::Bedrock);
        }
        let baseline = self.base(cell);
        if block != baseline && !self.edits.contains_key(&cell) && self.edits.len() >= MAX_EDITS {
            return Err(EditError::EditLimit);
        }
        self.next_revision
            .checked_add(7)
            .ok_or(EditError::RevisionExhausted)?;
        if block == baseline {
            self.edits.remove(&cell);
        } else {
            self.edits.insert(cell, block);
        }
        let own = chunk_index(cell);
        if let Some(chunk) = self.chunks.get_mut(&own) {
            chunk.blocks[local_index(cell)] = block;
        }
        self.bump(own)?;
        for axis in 0..3 {
            for direction in [-1, 1] {
                let mut neighbor = cell;
                neighbor[axis] += direction;
                if Self::contains(neighbor) {
                    let id = chunk_index(neighbor);
                    if id != own {
                        self.bump(id)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn bump(&mut self, id: usize) -> Result<(), EditError> {
        if self.chunks.contains_key(&id) {
            let revision = self.issue_revision()?;
            if let Some(chunk) = self.chunks.get_mut(&id) {
                chunk.revision = revision;
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn set(&mut self, cell: [i32; 3], block: Block) {
        self.try_set(cell, block).expect("bounded fixture edit");
    }

    #[cfg(test)]
    pub(super) fn from_blocks(blocks: Vec<Block>) -> Self {
        assert_eq!(blocks.len(), CELL_COUNT);
        let mut region = Self::generated(RegionId::Meadow, 0);
        region.fixture = Some(blocks);
        region.chunks.clear();
        for id in 0..CHUNK_COUNT {
            region
                .generate_chunk(id)
                .expect("bounded fixture bootstrap");
        }
        region
    }

    #[cfg(test)]
    pub fn solid_count(&self) -> usize {
        self.chunks
            .values()
            .map(|chunk| chunk.blocks.iter().filter(|block| block.solid()).count())
            .sum()
    }

    #[cfg(test)]
    pub(super) fn bootstrap_blocks(&self) -> Vec<Block> {
        (0..HEIGHT)
            .flat_map(|y| {
                (0..WIDTH).flat_map(move |z| (0..WIDTH).map(move |x| self.get([x, y, z])))
            })
            .collect()
    }

    pub(super) fn refresh_cache(&mut self) -> Result<(), EditError> {
        let ids: Vec<_> = self.resident_chunks().collect();
        self.chunks.clear();
        for id in ids {
            self.generate_chunk(id)?;
        }
        Ok(())
    }
}

/// Stable bounded coordinate encoding, preserving original IDs zero through 17.
pub(super) fn chunk_index([x, y, z]: [i32; 3]) -> usize {
    let [x, y, z] = [
        x.div_euclid(CHUNK_SIZE),
        y.div_euclid(CHUNK_SIZE),
        z.div_euclid(CHUNK_SIZE),
    ];
    if (0..3).contains(&x) && (0..3).contains(&z) {
        (x + 3 * (z + 3 * y)) as usize
    } else {
        CHUNK_COUNT
            + (x + WORLD_LIMIT / CHUNK_SIZE) as usize
            + GRID * ((z + WORLD_LIMIT / CHUNK_SIZE) as usize + GRID * y as usize)
    }
}

pub fn chunk_origin(id: usize) -> Option<[i32; 3]> {
    let coord = if id < CHUNK_COUNT {
        [(id % 3) as i32, (id / 9) as i32, (id / 3 % 3) as i32]
    } else {
        let value = id.checked_sub(CHUNK_COUNT)?;
        if value >= GRID * GRID * 2 {
            return None;
        }
        [
            (value % GRID) as i32 - WORLD_LIMIT / CHUNK_SIZE,
            (value / (GRID * GRID)) as i32,
            (value / GRID % GRID) as i32 - WORLD_LIMIT / CHUNK_SIZE,
        ]
    };
    let origin = coord.map(|axis| axis * CHUNK_SIZE);
    (Region::contains(origin) && chunk_index(origin) == id).then_some(origin)
}

fn local_index([x, y, z]: [i32; 3]) -> usize {
    (x.rem_euclid(CHUNK_SIZE)
        + CHUNK_SIZE * (z.rem_euclid(CHUNK_SIZE) + CHUNK_SIZE * y.rem_euclid(CHUNK_SIZE)))
        as usize
}

#[cfg(test)]
pub(super) fn index([x, y, z]: [i32; 3]) -> usize {
    (x + WIDTH * (z + WIDTH * y)) as usize
}

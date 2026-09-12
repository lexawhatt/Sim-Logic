//! Bounded game rules and terrain, independent of ECS and renderer lifetime.

mod block;
mod generation;
mod inventory;
mod mesh;
mod player;
mod storage;
mod terrain;

pub use block::Block;
pub use inventory::Inventory;
pub use player::{Hit, Movement, Player, raycast};
#[cfg(test)]
pub use terrain::{CHUNK_COUNT, WIDTH, WORLD_LIMIT, chunk_origin};
pub use terrain::{CHUNK_SIZE, HEIGHT, MAX_RESIDENT_CHUNKS, Region};

/// The two persistent terrain regions, not two simultaneously active ECS worlds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegionId {
    Meadow,
    Canyon,
}

impl RegionId {
    pub const ALL: [Self; 2] = [Self::Meadow, Self::Canyon];

    pub const fn index(self) -> usize {
        match self {
            Self::Meadow => 0,
            Self::Canyon => 1,
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Meadow => Self::Canyon,
            Self::Canyon => Self::Meadow,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Meadow => "Meadow",
            Self::Canyon => "Canyon",
        }
    }
}

/// One accepted atomic terrain/inventory edit. Revisions identify changed chunks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Edit {
    pub cell: [i32; 3],
    pub before: Block,
    pub after: Block,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditError {
    NoTarget,
    OutOfBounds,
    Bedrock,
    Occupied,
    PlayerOverlap,
    InventoryFull,
    InventoryEmpty,
    EditLimit,
    RevisionExhausted,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::NoTarget => "No block within reach",
            Self::OutOfBounds => "Outside the build area",
            Self::Bedrock => "The bottom layer cannot be removed",
            Self::Occupied => "That block is occupied",
            Self::PlayerOverlap => "Cannot build inside the player",
            Self::InventoryFull => "That inventory slot is full",
            Self::InventoryEmpty => "That inventory slot is empty",
            Self::EditLimit => "This region has reached its persistent edit limit",
            Self::RevisionExhausted => "Terrain revision identities are exhausted",
        })
    }
}

impl std::error::Error for EditError {}

/// Canonical application-owned state. No entity or GPU handles are persisted.
#[derive(Clone, Debug)]
pub struct SaveGame {
    pub active: RegionId,
    pub player: Player,
    pub inventory: Inventory,
    pub creative: bool,
    seed: u64,
    regions: [Region; 2],
    parked_players: [Player; 2],
}

impl SaveGame {
    pub fn new(seed: u64) -> Self {
        let regions = [
            Region::generated(RegionId::Meadow, seed),
            Region::generated(RegionId::Canyon, seed),
        ];
        let parked_players = [regions[0].spawn(), regions[1].spawn()];
        Self {
            active: RegionId::Meadow,
            player: parked_players[0],
            inventory: Inventory::default(),
            creative: true,
            seed,
            regions,
            parked_players,
        }
    }

    pub const fn seed(&self) -> u64 {
        self.seed
    }

    pub fn stream_active(&mut self) -> Result<(), EditError> {
        self.regions[self.active.index()].stream_around(self.player.position)
    }

    pub fn region(&self, id: RegionId) -> &Region {
        &self.regions[id.index()]
    }

    pub fn active_region(&self) -> &Region {
        self.region(self.active)
    }

    /// Advance only the active player's bounded deterministic movement.
    pub fn step(&mut self, movement: Movement, seconds: f32) {
        self.player
            .step(&self.regions[self.active.index()], movement, seconds);
    }

    /// Fly only in creative mode. Rejected inputs or survival mode change nothing.
    pub fn fly_step(&mut self, movement: Movement, vertical: f32, seconds: f32) -> bool {
        self.creative
            && self.player.step_flying(
                &self.regions[self.active.index()],
                movement,
                vertical,
                seconds,
            )
    }

    /// Commit only after the destination ECS world has been installed.
    /// Both terrain and each region's last player position survive the visit.
    pub fn travel(&mut self, target: RegionId) {
        if target == self.active {
            return;
        }
        self.parked_players[self.active.index()] = self.player;
        self.active = target;
        self.player = self.parked_players[target.index()];
        self.player.stop();
    }

    pub fn target(&self) -> Option<Hit> {
        raycast(
            self.active_region(),
            self.player.eye(),
            self.player.forward(),
            Player::REACH,
        )
    }

    /// Rejects before changing either inventory or terrain.
    pub fn break_target(&mut self) -> Result<Edit, EditError> {
        let hit = self.target().ok_or(EditError::NoTarget)?;
        if hit.cell[1] == 0 {
            return Err(EditError::Bedrock);
        }
        let before = self.active_region().get(hit.cell);
        let count = &mut self.inventory.counts[before as usize - 1];
        if !self.creative && *count == Inventory::CAPACITY {
            return Err(EditError::InventoryFull);
        }
        self.regions[self.active.index()].try_set(hit.cell, Block::Air)?;
        if !self.creative {
            *count += 1;
        }
        Ok(Edit {
            cell: hit.cell,
            before,
            after: Block::Air,
        })
    }

    /// Place only on a reachable face, outside the player's body.
    pub fn place_target(&mut self) -> Result<Edit, EditError> {
        let cell = self
            .target()
            .ok_or(EditError::NoTarget)?
            .adjacent
            .ok_or(EditError::Occupied)?;
        if !Region::contains(cell) {
            return Err(EditError::OutOfBounds);
        }
        if self.active_region().get(cell).solid() {
            return Err(EditError::Occupied);
        }
        if self.player.overlaps(cell) {
            return Err(EditError::PlayerOverlap);
        }
        let after = self.inventory.selected();
        let count = &mut self.inventory.counts[after as usize - 1];
        if !self.creative && *count == 0 {
            return Err(EditError::InventoryEmpty);
        }
        self.regions[self.active.index()].try_set(cell, after)?;
        if !self.creative {
            *count -= 1;
        }
        Ok(Edit {
            cell,
            before: Block::Air,
            after,
        })
    }
}

#[cfg(test)]
mod streaming_tests;
#[cfg(test)]
mod tests;

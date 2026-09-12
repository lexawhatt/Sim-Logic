//! Bounded game rules and terrain, independent of ECS and renderer lifetime.

mod mesh;
mod player;
mod storage;
mod terrain;

pub use player::{Hit, Movement, Player, raycast};
pub use terrain::{CHUNK_COUNT, CHUNK_SIZE, HEIGHT, Region, WIDTH};

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

/// Opaque block materials. Air has no mesh or inventory slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Block {
    Air,
    Grass,
    Stone,
    Wood,
    Sand,
    Leaves,
}

impl Block {
    pub const SOLID: [Self; 5] = [
        Self::Grass,
        Self::Stone,
        Self::Wood,
        Self::Sand,
        Self::Leaves,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Air => "Air",
            Self::Grass => "Grass",
            Self::Stone => "Stone",
            Self::Wood => "Wood",
            Self::Sand => "Sand",
            Self::Leaves => "Leaves",
        }
    }

    /// Straight-linear opaque tint; shade 0 is bottom, 1 sides, and 2 top.
    pub fn color(self, shade: u8) -> sim_engine::Color {
        let rgb = match self {
            Self::Air => [0.0, 0.0, 0.0],
            Self::Grass => [0.28, 0.64, 0.18],
            Self::Stone => [0.48, 0.53, 0.60],
            Self::Wood => [0.50, 0.27, 0.11],
            Self::Sand => [0.82, 0.60, 0.29],
            Self::Leaves => [0.14, 0.43, 0.16],
        };
        let light = match shade {
            0 => 0.48,
            1 => 0.73,
            _ => 1.0,
        };
        sim_engine::Color::rgb(rgb[0] * light, rgb[1] * light, rgb[2] * light)
    }

    pub const fn solid(self) -> bool {
        !matches!(self, Self::Air)
    }

    pub(super) fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Air),
            1 => Some(Self::Grass),
            2 => Some(Self::Stone),
            3 => Some(Self::Wood),
            4 => Some(Self::Sand),
            5 => Some(Self::Leaves),
            _ => None,
        }
    }
}

/// Five finite slots shared between regions. Building consumes one block.
#[derive(Clone, Debug, PartialEq)]
pub struct Inventory {
    counts: [u16; 5],
    selected: Block,
}

impl Default for Inventory {
    fn default() -> Self {
        Self {
            counts: [32; 5],
            selected: Block::Grass,
        }
    }
}

impl Inventory {
    pub const CAPACITY: u16 = 999;

    pub fn count(&self, block: Block) -> u16 {
        if block.solid() {
            self.counts[block as usize - 1]
        } else {
            0
        }
    }

    pub fn selected(&self) -> Block {
        self.selected
    }

    /// Returns false for Air, leaving selection unchanged.
    pub fn select(&mut self, block: Block) -> bool {
        if !block.solid() {
            return false;
        }
        self.selected = block;
        true
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
            seed,
            regions,
            parked_players,
        }
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
        if *count == Inventory::CAPACITY {
            return Err(EditError::InventoryFull);
        }
        *count += 1;
        self.regions[self.active.index()].set(hit.cell, Block::Air);
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
        let after = self.inventory.selected;
        let count = &mut self.inventory.counts[after as usize - 1];
        if *count == 0 {
            return Err(EditError::InventoryEmpty);
        }
        *count -= 1;
        self.regions[self.active.index()].set(cell, after);
        Ok(Edit {
            cell,
            before: Block::Air,
            after,
        })
    }
}

#[cfg(test)]
mod tests;

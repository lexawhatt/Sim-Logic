//! Small versioned host save format; no serializer or filesystem in the library.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use super::{Block, Inventory, Player, Region, RegionId, SaveGame, terrain::CELL_COUNT};

const MAGIC: &[u8; 8] = b"SVXLS001";
pub(super) const SAVE_BYTES: usize = 8 + 8 + 2 + 10 + 40 + 2 * CELL_COUNT + 8;

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    Invalid(&'static str),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Save file: {error}"),
            Self::Invalid(reason) => write!(formatter, "Invalid save: {reason}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Invalid(_) => None,
        }
    }
}

impl From<std::io::Error> for SaveError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl SaveGame {
    /// Explicit path only. Validate first, write and sync a newly-created sibling
    /// temporary file, then rename over the destination. Failures before rename
    /// preserve the old destination. The containing directory is not fsynced.
    /// A conflicting temporary file is never deleted or overwritten.
    pub fn save(&self, path: &Path) -> Result<(), SaveError> {
        let bytes = self.encode()?;
        let name = path
            .file_name()
            .ok_or(SaveError::Invalid("path has no file name"))?;
        let mut temporary_name = name.to_os_string();
        temporary_name.push(format!(".{}.tmp", std::process::id()));
        let temporary = path.with_file_name(temporary_name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let mut cleanup = TemporaryFile {
            path: temporary,
            owned: true,
        };
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&cleanup.path, path)?;
        cleanup.owned = false;
        Ok(())
    }

    /// Read at most the exact versioned size plus one byte. Invalid or oversized
    /// input never changes a running game; the caller decides when to install it.
    pub fn load(path: &Path) -> Result<Self, SaveError> {
        let file = File::open(path)?;
        let mut bytes = Vec::with_capacity(SAVE_BYTES + 1);
        file.take((SAVE_BYTES + 1) as u64).read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, SaveError> {
        let mut players = self.parked_players;
        players[self.active.index()] = self.player;
        validate(&self.regions, &players, &self.inventory)?;
        let mut bytes = Vec::with_capacity(SAVE_BYTES);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.seed.to_le_bytes());
        bytes.push(self.active.index() as u8);
        bytes.push(self.inventory.selected as u8);
        for count in self.inventory.counts {
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        for player in players {
            for value in player
                .position
                .into_iter()
                .chain([player.yaw, player.pitch])
            {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        for region in &self.regions {
            bytes.extend(region.blocks.iter().map(|block| *block as u8));
        }
        bytes.extend_from_slice(&checksum(&bytes).to_le_bytes());
        Ok(bytes)
    }

    pub(super) fn decode(bytes: &[u8]) -> Result<Self, SaveError> {
        if bytes.len() != SAVE_BYTES {
            return Err(SaveError::Invalid("unexpected byte count"));
        }
        if &bytes[..8] != MAGIC {
            return Err(SaveError::Invalid("unknown format version"));
        }
        let (body, stored_checksum) = bytes.split_at(SAVE_BYTES - 8);
        if stored_checksum != checksum(body).to_le_bytes() {
            return Err(SaveError::Invalid("checksum mismatch"));
        }
        let mut cursor = Cursor {
            bytes: body,
            offset: 8,
        };
        let seed = u64::from_le_bytes(cursor.take()?);
        let active = match cursor.take::<1>()?[0] {
            0 => RegionId::Meadow,
            1 => RegionId::Canyon,
            _ => return Err(SaveError::Invalid("unknown region")),
        };
        let selected = Block::from_byte(cursor.take::<1>()?[0])
            .filter(|block| block.solid())
            .ok_or(SaveError::Invalid("unknown selected block"))?;
        let mut counts = [0; 5];
        for count in &mut counts {
            *count = u16::from_le_bytes(cursor.take()?);
        }
        let inventory = Inventory { counts, selected };
        let mut parked_players = [Player::at([0.0; 3]); 2];
        for player in &mut parked_players {
            for value in &mut player.position {
                *value = f32::from_le_bytes(cursor.take()?);
            }
            player.yaw = f32::from_le_bytes(cursor.take()?);
            player.pitch = f32::from_le_bytes(cursor.take()?);
        }
        let mut decoded = Vec::with_capacity(2);
        for _ in 0..2 {
            let mut blocks = Vec::with_capacity(CELL_COUNT);
            for _ in 0..CELL_COUNT {
                blocks.push(
                    Block::from_byte(cursor.take::<1>()?[0])
                        .ok_or(SaveError::Invalid("unknown terrain block"))?,
                );
            }
            decoded.push(Region::from_blocks(blocks));
        }
        let regions: [Region; 2] = decoded
            .try_into()
            .map_err(|_| SaveError::Invalid("region count"))?;
        validate(&regions, &parked_players, &inventory)?;
        Ok(Self {
            active,
            player: parked_players[active.index()],
            inventory,
            seed,
            regions,
            parked_players,
        })
    }
}

fn validate(
    regions: &[Region; 2],
    players: &[Player; 2],
    inventory: &Inventory,
) -> Result<(), SaveError> {
    if inventory
        .counts
        .into_iter()
        .any(|count| count > Inventory::CAPACITY)
        || !inventory.selected.solid()
    {
        return Err(SaveError::Invalid("inventory exceeds its limits"));
    }
    for (region, player) in regions.iter().zip(players) {
        if region.blocks.len() != CELL_COUNT {
            return Err(SaveError::Invalid("terrain dimensions"));
        }
        for z in 0..super::WIDTH {
            for x in 0..super::WIDTH {
                if region.get([x, 0, z]) != Block::Stone {
                    return Err(SaveError::Invalid("missing bedrock"));
                }
            }
        }
        if !player.valid(region) {
            return Err(SaveError::Invalid(
                "player outside the safe area or inside terrain",
            ));
        }
    }
    Ok(())
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl Cursor<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], SaveError> {
        let slice = self
            .bytes
            .get(self.offset..self.offset + N)
            .ok_or(SaveError::Invalid("truncated save"))?;
        self.offset += N;
        slice
            .try_into()
            .map_err(|_| SaveError::Invalid("truncated field"))
    }
}

pub(super) fn checksum(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

struct TemporaryFile {
    path: PathBuf,
    owned: bool,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

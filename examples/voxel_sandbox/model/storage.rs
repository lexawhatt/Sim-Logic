//! Small versioned host save format; no serializer or filesystem in the library.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use super::{Block, Inventory, Player, Region, RegionId, SaveGame, terrain::MAX_EDITS};

const MAGIC: &[u8; 8] = b"SVXLS002";
pub(super) const FIXED_BYTES: usize = 8 + 8 + 4 + 9 + 64 + 40 + 8 + 8;
pub(super) const MAX_SAVE_BYTES: usize = FIXED_BYTES + 2 * MAX_EDITS * 13;

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
        // A supplied old save path is not implicit permission to destroy data
        // this version cannot load. Use the separate v2 filename instead.
        match File::open(path) {
            Ok(mut existing) => {
                let mut prefix = [0; 8];
                match existing.read_exact(&mut prefix) {
                    Ok(()) if &prefix == b"SVXLS001" => {
                        return Err(SaveError::Invalid(
                            "legacy save preserved; choose a new v2 filename",
                        ));
                    }
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => {}
                    Err(error) => return Err(error.into()),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
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

    /// Read at most the maximum versioned size plus one byte. Invalid or oversized
    /// input never changes a running game; the caller decides when to install it.
    pub fn load(path: &Path) -> Result<Self, SaveError> {
        let file = File::open(path)?;
        let mut bytes = Vec::with_capacity(MAX_SAVE_BYTES + 1);
        file.take((MAX_SAVE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::decode(&bytes)
    }

    pub(crate) fn encode(&self) -> Result<Vec<u8>, SaveError> {
        let mut players = self.parked_players;
        players[self.active.index()] = self.player;
        validate(&self.regions, &players, &self.inventory)?;
        let size = FIXED_BYTES
            + self
                .regions
                .iter()
                .map(|region| region.edits.len() * 13)
                .sum::<usize>();
        let mut bytes = Vec::with_capacity(size);
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&self.seed.to_le_bytes());
        bytes.extend_from_slice(&[
            self.active.index() as u8,
            u8::from(self.creative),
            self.inventory.selected_slot as u8,
            Block::SOLID.len() as u8,
        ]);
        bytes.extend(self.inventory.hotbar.map(|block| block as u8));
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
        for (id, region) in RegionId::ALL.into_iter().zip(&self.regions) {
            if region.seed != self.seed || region.kind != id {
                return Err(SaveError::Invalid("region generation descriptor mismatch"));
            }
            bytes.extend_from_slice(&(region.edits.len() as u32).to_le_bytes());
            for (cell, block) in &region.edits {
                for coordinate in cell {
                    bytes.extend_from_slice(&coordinate.to_le_bytes());
                }
                bytes.push(*block as u8);
            }
        }
        bytes.extend_from_slice(&checksum(&bytes).to_le_bytes());
        Ok(bytes)
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, SaveError> {
        if bytes.get(..8) == Some(b"SVXLS001") {
            return Err(SaveError::Invalid(
                "legacy finite-world save is unsupported; keep the original file and open it with the previous example",
            ));
        }
        if bytes.len() < FIXED_BYTES || bytes.len() > MAX_SAVE_BYTES {
            return Err(SaveError::Invalid("unexpected byte count"));
        }
        if &bytes[..8] != MAGIC {
            return Err(SaveError::Invalid("unknown format version"));
        }
        let (body, stored_checksum) = bytes.split_at(bytes.len() - 8);
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
        let creative = match cursor.take::<1>()?[0] {
            0 => false,
            1 => true,
            _ => return Err(SaveError::Invalid("unknown creative flag")),
        };
        let selected_slot = cursor.take::<1>()?[0] as usize;
        if cursor.take::<1>()?[0] as usize != Block::SOLID.len() {
            return Err(SaveError::Invalid("unsupported block palette"));
        }
        let mut hotbar = [Block::Grass; 9];
        for block in &mut hotbar {
            *block = Block::from_byte(cursor.take::<1>()?[0])
                .filter(|block| block.solid())
                .ok_or(SaveError::Invalid("unknown hotbar block"))?;
        }
        let mut counts = [0; 32];
        for count in &mut counts {
            *count = u16::from_le_bytes(cursor.take()?);
        }
        let inventory = Inventory {
            counts,
            hotbar,
            selected_slot,
        };
        let mut parked_players = [Player::at([0.0; 3]); 2];
        for player in &mut parked_players {
            for value in &mut player.position {
                *value = f32::from_le_bytes(cursor.take()?);
            }
            player.yaw = f32::from_le_bytes(cursor.take()?);
            player.pitch = f32::from_le_bytes(cursor.take()?);
        }
        let mut regions = [
            Region::generated(RegionId::Meadow, seed),
            Region::generated(RegionId::Canyon, seed),
        ];
        for region in &mut regions {
            let count = u32::from_le_bytes(cursor.take()?) as usize;
            if count > MAX_EDITS {
                return Err(SaveError::Invalid("region edit limit exceeded"));
            }
            let mut previous = None;
            for _ in 0..count {
                let cell = [
                    i32::from_le_bytes(cursor.take()?),
                    i32::from_le_bytes(cursor.take()?),
                    i32::from_le_bytes(cursor.take()?),
                ];
                let block = Block::from_byte(cursor.take::<1>()?[0])
                    .ok_or(SaveError::Invalid("unknown edited block"))?;
                if !Region::contains(cell) || cell[1] == 0 {
                    return Err(SaveError::Invalid(
                        "edited cell outside bounds or on bedrock",
                    ));
                }
                if previous.is_some_and(|old| old >= cell) || block == region.base(cell) {
                    return Err(SaveError::Invalid(
                        "noncanonical or duplicate terrain edits",
                    ));
                }
                region.edits.insert(cell, block);
                previous = Some(cell);
            }
            region
                .refresh_cache()
                .map_err(|_| SaveError::Invalid("terrain revision limit"))?;
        }
        if cursor.offset != body.len() {
            return Err(SaveError::Invalid("trailing save fields"));
        }
        validate(&regions, &parked_players, &inventory)?;
        Ok(Self {
            active,
            player: parked_players[active.index()],
            inventory,
            creative,
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
        || inventory.selected_slot >= 9
        || inventory.hotbar.iter().any(|block| !block.solid())
    {
        return Err(SaveError::Invalid("inventory exceeds its limits"));
    }
    for (region, player) in regions.iter().zip(players) {
        if region.edits.len() > MAX_EDITS
            || region.edits.iter().any(|(cell, block)| {
                !Region::contains(*cell) || cell[1] == 0 || *block == region.base(*cell)
            })
        {
            return Err(SaveError::Invalid("invalid sparse terrain edits"));
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

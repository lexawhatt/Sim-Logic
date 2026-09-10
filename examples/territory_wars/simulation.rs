//! Bounded, deterministic territory-game rules, independent of rendering and ECS.
//!
//! A tick is one tenth of a second. Each faction may have one expedition at a
//! time; dispatched troops no longer defend home territory. Expeditions capture
//! at most six bordering cells per tick and return survivors when blocked.
//! These are original demo rules, not Territorial.io's exact balance formulas.

#[path = "simulation/ai.rs"]
mod ai;
/// Pure combat calculations and the bounded expedition resolver.
#[path = "simulation/combat.rs"]
pub mod combat;
/// Pure troop calculations, income settlement, and explicitly gated host cheats.
#[path = "simulation/economy.rs"]
pub mod economy;
#[path = "simulation/terrain.rs"]
mod terrain;

use std::{error::Error, fmt};

/// Grid width, in cells.
pub const WIDTH: usize = 96;
/// Grid height, in cells.
pub const HEIGHT: usize = 64;
/// One human faction and six computer opponents.
pub const FACTIONS: usize = 7;
/// Impassable water; armies cannot sail in this demo.
pub const WATER: u8 = 255;
/// Unclaimed land; capturing a cell costs a fixed number of troops.
pub const NEUTRAL: u8 = 254;

const CELLS: usize = WIDTH * HEIGHT;
const INITIAL_LAND: usize = 13;
const INITIAL_TROOPS: u32 = 620;
const NEUTRAL_COST: u32 = 12;
const CAPACITY_PER_CELL: u32 = 80;
const CAPTURES_PER_TICK: usize = 6;
const NAMES: [&str; FACTIONS] = ["YOU", "EMBER", "AZURE", "MOSS", "VIOLET", "GOLD", "CORAL"];

/// The world waits for a starting cell, runs, or keeps a terminal result visible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Click unclaimed land to establish the player's capital.
    Choosing,
    /// Income, expeditions, and computer decisions are active.
    Running,
    /// The player controls 70% of the island or has eliminated every rival.
    Won,
    /// The player has lost every owned cell.
    Lost,
}

/// Read-only faction information used by the HUD and map labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Faction {
    /// Short label; its index is also its map ownership and palette index.
    pub name: &'static str,
    /// Home troops available for defense or a new expedition.
    pub troops: u32,
    /// Exact number of currently owned land cells.
    pub land: usize,
    /// Initial spawn cell; it has no special combat or victory rules.
    pub capital: usize,
}

/// Last computer decision, retained until its next staggered decision point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// No decision has run yet, or a previous expedition is still active.
    Waiting,
    /// Dispatching against unclaimed land.
    Expanding,
    /// Dispatching against a neighboring faction.
    Attacking,
    /// Saving troops or unable to reach another owner.
    Holding,
    /// This faction owns no remaining land.
    Eliminated,
}

/// Fixed-size inspection data. Opening the debug panel does not change play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FactionDiagnostics {
    /// Newly credited troops at the last one-second income boundary.
    pub last_income: u32,
    /// Troops consumed by this faction's expedition during the latest tick.
    pub last_spent: u32,
    /// Cells captured during the latest tick, not a forecast.
    pub last_captured: usize,
    /// Troops committed by the last successfully accepted order.
    pub last_dispatched: u32,
    /// Latest staggered AI decision; the human entry remains `Waiting`.
    pub last_decision: Decision,
    /// Target for that decision; `None` means no order was accepted.
    pub decision_target: Option<u8>,
}

impl Default for FactionDiagnostics {
    fn default() -> Self {
        Self {
            last_income: 0,
            last_spent: 0,
            last_captured: 0,
            last_dispatched: 0,
            last_decision: Decision::Waiting,
            decision_target: None,
        }
    }
}

/// A rejected order has no effect, including no change to the random stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderError {
    /// The match has not started or already ended.
    NotRunning,
    /// Attacker index is outside the faction table.
    InvalidFaction,
    /// Water, self, or an unknown faction is not an attack target.
    InvalidTarget,
    /// Dispatch percentages must be between 1 and 100 inclusive.
    InvalidPercent,
    /// The attacker has lost all territory.
    Eliminated,
    /// No target cell shares a four-connected border with the attacker.
    NoBorder,
    /// The requested fraction cannot pay even the neutral-cell cost.
    InsufficientTroops,
    /// This faction's previous expedition is still moving.
    CampaignActive,
}

impl fmt::Display for OrderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotRunning => "choose a starting point or restart the match",
            Self::InvalidFaction => "unknown attacking faction",
            Self::InvalidTarget => "select neutral land or a rival faction",
            Self::InvalidPercent => "dispatch must be between 1 and 100 percent",
            Self::Eliminated => "the attacking faction has no territory",
            Self::NoBorder => "the selected territory does not touch yours",
            Self::InsufficientTroops => "wait for more troops or send a larger fraction",
            Self::CampaignActive => "your expedition is still moving",
        })
    }
}

impl Error for OrderError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Campaign {
    target: u8,
    remaining: u32,
}

/// A single island match. Tick and order processing reuse fixed-capacity storage.
pub struct Game {
    owners: Vec<u8>,
    factions: [Faction; FACTIONS],
    campaigns: [Option<Campaign>; FACTIONS],
    diagnostics: [FactionDiagnostics; FACTIONS],
    frontier: Vec<usize>,
    phase: Phase,
    ticks: u64,
    land_count: usize,
    seed: u64,
    random: Random,
    bots_enabled: bool,
}

impl Game {
    /// Generates a connected island and allocates all simulation scratch storage.
    pub fn new(seed: u64) -> Self {
        let owners = terrain::island(seed);
        let land_count = owners.iter().filter(|&&owner| owner == NEUTRAL).count();
        Self {
            owners,
            factions: std::array::from_fn(|index| Faction {
                name: NAMES[index],
                troops: 0,
                land: 0,
                capital: 0,
            }),
            campaigns: [None; FACTIONS],
            diagnostics: [FactionDiagnostics::default(); FACTIONS],
            frontier: Vec::with_capacity(CELLS),
            phase: Phase::Choosing,
            ticks: 0,
            land_count,
            seed,
            random: Random::new(seed),
            bots_enabled: true,
        }
    }

    /// Starts on a neutral cell and spreads the six opponents away from it.
    /// Invalid cells or repeated starts are rejected without changing the match.
    pub fn start(&mut self, cell: usize) -> bool {
        if self.phase != Phase::Choosing || self.owners.get(cell) != Some(&NEUTRAL) {
            return false;
        }
        self.establish(0, cell);
        for faction in 1..FACTIONS {
            let mut selected = None;
            let mut largest_distance = 0;
            for candidate in 0..CELLS {
                if self.owners[candidate] != NEUTRAL {
                    continue;
                }
                let distance = self.factions[..faction]
                    .iter()
                    .map(|other| distance_squared(candidate, other.capital))
                    .min()
                    .unwrap_or(0);
                if distance > largest_distance {
                    largest_distance = distance;
                    selected = Some(candidate);
                }
            }
            if let Some(capital) = selected {
                self.establish(faction, capital);
            }
        }
        self.phase = Phase::Running;
        true
    }

    /// Advances exactly 100 ms. Finished matches and the spawn screen are inert.
    pub fn tick(&mut self) {
        if self.phase != Phase::Running {
            return;
        }
        self.ticks = self.ticks.saturating_add(1);
        for diagnostic in &mut self.diagnostics {
            diagnostic.last_spent = 0;
            diagnostic.last_captured = 0;
        }
        if self.ticks.is_multiple_of(10) {
            self.income();
        }
        // Rotate resolution priority so index zero does not always move first.
        let first = (self.ticks % FACTIONS as u64) as usize;
        for offset in 0..FACTIONS {
            self.advance_campaign((first + offset) % FACTIONS);
        }
        self.check_outcome();
        if self.phase == Phase::Running && self.bots_enabled {
            // Stagger decisions and leave a short opening for the human player.
            for faction in 1..FACTIONS {
                if self.ticks >= 100 && self.ticks % 13 == (faction * 2) as u64 {
                    self.computer_order(faction);
                }
            }
        }
    }

    /// Dispatches a fraction of home troops against a bordering owner. An army
    /// targets the faction, not a particular clicked cell. At most one expedition
    /// per attacker may be active; failures are atomic.
    pub fn order(&mut self, attacker: usize, target: u8, percent: u8) -> Result<(), OrderError> {
        if self.phase != Phase::Running {
            return Err(OrderError::NotRunning);
        }
        let Some(faction) = self.factions.get(attacker) else {
            return Err(OrderError::InvalidFaction);
        };
        if (target as usize >= FACTIONS && target != NEUTRAL) || target as usize == attacker {
            return Err(OrderError::InvalidTarget);
        }
        if !(1..=100).contains(&percent) {
            return Err(OrderError::InvalidPercent);
        }
        if faction.land == 0 {
            return Err(OrderError::Eliminated);
        }
        if self.campaigns[attacker].is_some() {
            return Err(OrderError::CampaignActive);
        }
        if !self.borders(attacker, target) {
            return Err(OrderError::NoBorder);
        }
        let remaining = economy::dispatch(faction.troops, percent).unwrap_or(0);
        if remaining < NEUTRAL_COST {
            return Err(OrderError::InsufficientTroops);
        }
        self.factions[attacker].troops -= remaining;
        self.campaigns[attacker] = Some(Campaign { target, remaining });
        self.diagnostics[attacker].last_dispatched = remaining;
        Ok(())
    }

    /// Sends the player's requested fraction into adjacent neutral land.
    pub fn expand(&mut self, percent: u8) -> Result<(), OrderError> {
        self.order(0, NEUTRAL, percent)
    }

    /// Row-major owner indices, with [`NEUTRAL`] and [`WATER`] sentinels.
    pub fn owners(&self) -> &[u8] {
        &self.owners
    }

    /// Live home-army and territory totals, excluding deployed troops.
    pub fn factions(&self) -> &[Faction; FACTIONS] {
        &self.factions
    }

    /// Current spawn, playing, or terminal mode.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Number of elapsed 100 ms ticks; terminal matches stop accumulating time.
    pub fn elapsed_ticks(&self) -> u64 {
        self.ticks
    }

    /// Total traversable cells, invariant throughout the match.
    pub fn land_count(&self) -> usize {
        self.land_count
    }

    /// Seed used to construct this map and computer decision stream.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Whether any target cell shares an edge with this faction's territory.
    pub fn borders(&self, attacker: usize, target: u8) -> bool {
        attacker < FACTIONS
            && (target == NEUTRAL || (target as usize) < FACTIONS)
            && target as usize != attacker
            && self
                .owners
                .iter()
                .enumerate()
                .any(|(cell, &owner)| owner == target && self.touches(cell, attacker as u8))
    }

    /// Number of nonempty expeditions, bounded by [`FACTIONS`].
    pub fn active_campaigns(&self) -> usize {
        self.campaigns.iter().flatten().count()
    }

    /// Troops currently away from home; unknown or idle factions return zero.
    pub fn campaign_remaining(&self, attacker: usize) -> u32 {
        self.campaigns
            .get(attacker)
            .copied()
            .flatten()
            .map_or(0, |campaign| campaign.remaining)
    }

    /// Last economy, combat, and AI decisions, without allocating a report.
    pub fn diagnostics(&self) -> &[FactionDiagnostics; FACTIONS] {
        &self.diagnostics
    }

    fn establish(&mut self, faction: usize, cell: usize) {
        self.frontier.clear();
        self.frontier.push(cell);
        self.owners[cell] = faction as u8;
        let mut cursor = 0;
        while cursor < self.frontier.len() && self.frontier.len() < INITIAL_LAND {
            let origin = self.frontier[cursor];
            cursor += 1;
            for adjacent in neighbors(origin).into_iter().flatten() {
                if self.owners[adjacent] == NEUTRAL && self.frontier.len() < INITIAL_LAND {
                    self.owners[adjacent] = faction as u8;
                    self.frontier.push(adjacent);
                }
            }
        }
        self.factions[faction].capital = cell;
        self.factions[faction].land = self.frontier.len();
        self.factions[faction].troops = INITIAL_TROOPS;
    }

    fn touches(&self, cell: usize, owner: u8) -> bool {
        neighbors(cell)
            .into_iter()
            .flatten()
            .any(|adjacent| self.owners[adjacent] == owner)
    }

    fn check_outcome(&mut self) {
        if self.factions[0].land == 0 {
            self.phase = Phase::Lost;
        } else if self.factions[0].land * 10 >= self.land_count * 7
            || self.factions[1..].iter().all(|faction| faction.land == 0)
        {
            self.phase = Phase::Won;
        }
    }
}

fn neighbors(cell: usize) -> [Option<usize>; 4] {
    [
        (!cell.is_multiple_of(WIDTH)).then(|| cell - 1),
        (cell % WIDTH + 1 < WIDTH).then_some(cell + 1),
        (cell >= WIDTH).then(|| cell - WIDTH),
        (cell + WIDTH < CELLS).then_some(cell + WIDTH),
    ]
}

fn distance_squared(left: usize, right: usize) -> usize {
    let dx = (left % WIDTH).abs_diff(right % WIDTH);
    let dy = (left / WIDTH).abs_diff(right / WIDTH);
    dx * dx + dy * dy
}

struct Random(u64);

impl Random {
    fn new(seed: u64) -> Self {
        Self(seed ^ 0x6a09_e667_f3bc_c909)
    }

    fn index(&mut self, length: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((value ^ (value >> 31)) % length as u64) as usize
    }
}

#[cfg(test)]
#[path = "simulation/tests.rs"]
mod tests;

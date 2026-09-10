//! Inspectable, integer troop formulas. Income is settled once every ten ticks.

use super::{CAPACITY_PER_CELL, Game, Phase};

/// One second of potential and actually credited income.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IncomeBreakdown {
    /// Two troops per owned cell per second before the capacity limit.
    pub territory: u32,
    /// Four percent of home reserves, rounded down, before the capacity limit.
    pub interest: u32,
    /// Actual newly added troops after accounting for home and deployed armies.
    pub credited: u32,
    /// Total supported troops, home plus deployed; 80 per owned cell.
    pub capacity: u32,
}

/// Calculates the next one-second payout without mutating a game. Armies above
/// capacity receive no income but are not silently deleted. Inputs outside the
/// example's bounded map remain safe through saturating arithmetic.
pub fn income(troops: u32, land: usize, deployed: u32) -> IncomeBreakdown {
    let land = u32::try_from(land).unwrap_or(u32::MAX);
    let capacity = land.saturating_mul(CAPACITY_PER_CELL);
    let territory = land.saturating_mul(2);
    let interest = if land == 0 { 0 } else { troops / 25 };
    let space = capacity.saturating_sub(troops).saturating_sub(deployed);
    IncomeBreakdown {
        territory,
        interest,
        credited: territory.saturating_add(interest).min(space),
        capacity,
    }
}

/// Floors the requested fraction of home troops. Invalid percentages return
/// `None`; affordability and ownership are validated by `Game::order`.
pub fn dispatch(troops: u32, percent: u8) -> Option<u32> {
    (1..=100)
        .contains(&percent)
        .then(|| (u64::from(troops) * u64::from(percent) / 100) as u32)
}

impl Game {
    pub(super) fn income(&mut self) {
        for faction in 0..self.factions.len() {
            let deployed = self.campaign_remaining(faction);
            let state = &mut self.factions[faction];
            let credited = income(state.troops, state.land, deployed).credited;
            state.troops += credited;
            self.diagnostics[faction].last_income = credited;
        }
    }

    /// Debug-only host operation: grants at most the remaining supported troop
    /// capacity. Returns the actual grant. Unknown, eliminated, and non-running
    /// factions receive zero. The host must gate this and mark assisted matches.
    pub fn grant_troops(&mut self, faction: usize, amount: u32) -> u32 {
        if self.phase != Phase::Running {
            return 0;
        }
        let deployed = self.campaign_remaining(faction);
        let Some(state) = self.factions.get_mut(faction) else {
            return 0;
        };
        let capacity = income(state.troops, state.land, deployed).capacity;
        let amount = amount.min(
            capacity
                .saturating_sub(state.troops)
                .saturating_sub(deployed),
        );
        state.troops += amount;
        amount
    }
}

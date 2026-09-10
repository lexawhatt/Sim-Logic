//! Progressive frontier capture and pure cost previews. Actual enemy captures
//! vary as income, other campaigns, and garrison density change during travel.

use super::{CAPTURES_PER_TICK, Decision, Game, NEUTRAL, NEUTRAL_COST};

/// Cost to take one cell from the given home army. Zero land denotes neutral
/// territory. Defense is evenly distributed, rounded up, with a 25% advantage.
pub fn capture_cost(defender_troops: u32, defender_land: usize) -> u32 {
    let garrison = garrison(defender_troops, defender_land);
    NEUTRAL_COST
        .saturating_add(garrison)
        .saturating_add(garrison / 4)
}

/// Neutral cells this many troops can afford at 12 troops per cell. This is an
/// upper estimate: an expedition may run out of adjacent reachable neutral land.
pub fn neutral_capture_estimate(dispatched: u32) -> usize {
    (dispatched / NEUTRAL_COST) as usize
}

fn garrison(troops: u32, land: usize) -> u32 {
    if land == 0 {
        0
    } else {
        troops.div_ceil(u32::try_from(land).unwrap_or(u32::MAX))
    }
}

impl Game {
    pub(super) fn advance_campaign(&mut self, attacker: usize) {
        let Some(mut campaign) = self.campaigns[attacker].take() else {
            return;
        };
        if self.factions[attacker].land == 0 {
            return;
        }
        self.frontier.clear();
        for (cell, &owner) in self.owners.iter().enumerate() {
            if owner == campaign.target && self.touches(cell, attacker as u8) {
                self.frontier.push(cell);
            }
        }
        if self.frontier.is_empty() {
            self.factions[attacker].troops += campaign.remaining;
            return;
        }
        let captures = self.frontier.len().min(CAPTURES_PER_TICK);
        for _ in 0..captures {
            let selected = self.random.index(self.frontier.len());
            let cell = self.frontier.swap_remove(selected);
            let defender = (campaign.target != NEUTRAL).then_some(campaign.target as usize);
            let (garrison, cost) = defender.map_or((0, NEUTRAL_COST), |faction| {
                let state = &self.factions[faction];
                (
                    garrison(state.troops, state.land),
                    capture_cost(state.troops, state.land),
                )
            });
            if campaign.remaining < cost {
                // An unsuccessful final assault causes bounded attrition but
                // cannot take land. Neutral land never receives stored damage.
                if let Some(defender) = defender {
                    self.factions[defender].troops = self.factions[defender]
                        .troops
                        .saturating_sub(campaign.remaining.saturating_sub(NEUTRAL_COST));
                }
                self.diagnostics[attacker].last_spent += campaign.remaining;
                campaign.remaining = 0;
                break;
            }
            campaign.remaining -= cost;
            self.diagnostics[attacker].last_spent += cost;
            self.diagnostics[attacker].last_captured += 1;
            self.owners[cell] = attacker as u8;
            self.factions[attacker].land += 1;
            if let Some(defender) = defender {
                let state = &mut self.factions[defender];
                state.land -= 1;
                state.troops = state.troops.saturating_sub(garrison);
                if state.land == 0 {
                    state.troops = 0;
                    // Eliminated factions do not continue conquering from an
                    // army that no longer has any home territory.
                    self.campaigns[defender] = None;
                    self.diagnostics[defender].last_decision = Decision::Eliminated;
                    self.diagnostics[defender].decision_target = None;
                }
            }
            if campaign.remaining < NEUTRAL_COST {
                break;
            }
        }
        if campaign.remaining >= NEUTRAL_COST {
            self.campaigns[attacker] = Some(campaign);
        } else {
            self.factions[attacker].troops += campaign.remaining;
        }
    }
}

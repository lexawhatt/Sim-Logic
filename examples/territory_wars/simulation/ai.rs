//! Small deterministic opponents. They prefer cheap expansion, save a defensive
//! reserve, and occasionally exploit a neighbor's weakly defended frontier.

use super::{Decision, FACTIONS, Game, NEUTRAL, Phase, WATER};

impl Game {
    /// Enables or disables new bot orders. Existing armies and income continue;
    /// this is an AI switch, not a simulation pause. Host cheat gating is required.
    pub fn set_bots_enabled(&mut self, enabled: bool) {
        self.bots_enabled = enabled;
    }

    /// Whether computer factions may issue new orders at their next decision.
    pub fn bots_enabled(&self) -> bool {
        self.bots_enabled
    }

    /// Debug-only victory: gives all land to the player and retires every army.
    /// Spawn screens and already terminal matches are unchanged. The host must
    /// gate this and label the match assisted; no ordinary rule calls it.
    pub fn force_player_victory(&mut self) {
        if self.phase != Phase::Running {
            return;
        }
        for owner in &mut self.owners {
            if *owner != WATER {
                *owner = 0;
            }
        }
        self.factions[0].land = self.land_count;
        for faction in 1..FACTIONS {
            self.factions[faction].land = 0;
            self.factions[faction].troops = 0;
            self.diagnostics[faction].last_decision = Decision::Eliminated;
            self.diagnostics[faction].decision_target = None;
        }
        self.campaigns.fill(None);
        self.phase = Phase::Won;
    }

    pub(super) fn computer_order(&mut self, faction: usize) {
        self.diagnostics[faction].decision_target = None;
        if self.factions[faction].land == 0 {
            self.diagnostics[faction].last_decision = Decision::Eliminated;
            return;
        }
        if self.campaigns[faction].is_some() {
            self.diagnostics[faction].last_decision = Decision::Waiting;
            return;
        }
        self.diagnostics[faction].last_decision = Decision::Holding;
        if self.factions[faction].troops < 90 {
            return;
        }
        let neutral = self.borders(faction, NEUTRAL);
        let mut target = None;
        let mut weakest = u32::MAX;
        for opponent in 0..FACTIONS {
            if opponent == faction || !self.borders(faction, opponent as u8) {
                continue;
            }
            let defense = &self.factions[opponent];
            let strength = defense.troops / defense.land.max(1) as u32;
            if strength < weakest {
                weakest = strength;
                target = Some(opponent as u8);
            }
        }
        let target = if neutral && (weakest > 9 || self.random.index(5) != 0) {
            NEUTRAL
        } else if let Some(target) = target {
            target
        } else {
            return;
        };
        let percent = if target == NEUTRAL { 38 } else { 52 };
        if self.order(faction, target, percent).is_ok() {
            let diagnostic = &mut self.diagnostics[faction];
            diagnostic.last_decision = if target == NEUTRAL {
                Decision::Expanding
            } else {
                Decision::Attacking
            };
            diagnostic.decision_target = Some(target);
        }
    }
}

//! HUD snapshots and throttled diagnostics. No formatted hidden debug rows.

use std::time::Duration;

use sim_logic::prelude::*;

use super::super::{
    app::Session,
    materials::Settings,
    model::{Block, CHUNK_SIZE, MAX_RESIDENT_CHUNKS, RegionId},
    scene::{Local, Phase},
};
use super::{Button, DEBUG_LINES, HOTBAR_SLOTS, Label, Menu, Panel};

#[derive(PartialEq, Eq)]
pub struct Stamp {
    region: RegionId,
    phase: Phase,
    menu: Menu,
    debug: bool,
    creative: bool,
    hotbar: [Block; HOTBAR_SLOTS],
    counts: [u16; HOTBAR_SLOTS],
    selected_slot: usize,
}

impl Stamp {
    pub fn new(local: &Local, session: &Session) -> Self {
        let inventory = &session.game.inventory;
        let hotbar = inventory.hotbar();
        Self {
            region: local.region,
            phase: local.phase,
            menu: local.menu,
            debug: local.debug,
            creative: session.game.creative,
            hotbar,
            counts: hotbar.map(|block| inventory.count(block)),
            selected_slot: inventory.selected_slot(),
        }
    }
}

#[derive(Default, Resource)]
pub struct HudCache {
    stamp: Option<Stamp>,
    notice: String,
    notice_revision: u64,
    notice_elapsed: Duration,
    debug_elapsed: Duration,
    debug_frames: u64,
    debug_lines: [String; DEBUG_LINES],
}

impl HudCache {
    /// Returns whether a fresh debug snapshot is due. FrameTime is the runner's
    /// unscaled interval, not measured CPU execution or GPU time.
    pub fn tick(&mut self, debug: bool, delta: Duration, notice: &str, revision: u64) -> bool {
        self.notice_elapsed = if self.notice == notice && self.notice_revision == revision {
            self.notice_elapsed.saturating_add(delta)
        } else {
            Duration::ZERO
        };
        if !debug {
            self.debug_elapsed = Duration::ZERO;
            self.debug_frames = 0;
            return false;
        }
        self.debug_elapsed = self.debug_elapsed.saturating_add(delta);
        self.debug_frames = self.debug_frames.saturating_add(1);
        self.stamp.as_ref().is_none_or(|stamp| !stamp.debug)
            || self.debug_elapsed >= Duration::from_millis(250)
    }

    pub fn changed(&self, stamp: &Stamp, notice: &str, revision: u64) -> bool {
        self.stamp.as_ref() != Some(stamp)
            || self.notice != notice
            || self.notice_revision != revision
    }

    pub fn publish(&mut self, stamp: Stamp, notice: &str, revision: u64) {
        self.stamp = Some(stamp);
        self.notice_revision = revision;
        if self.notice != notice {
            self.notice.clear();
            self.notice.push_str(notice);
        }
    }

    pub fn visible(&self, label: Label, local: &Local, session: &Session) -> bool {
        super::label_visible(label, local.menu, local.debug)
            && match label {
                Label::SlotCount(_) => !session.game.creative,
                Label::Notice => {
                    local.phase != Phase::Ready
                        || (!session.notice.is_empty()
                            && self.notice_elapsed < Duration::from_secs(4))
                }
                _ => true,
            }
    }

    pub fn caption(&self, label: Label, stamp: &Stamp, notice: &str) -> String {
        match label {
            Label::Title => format!("TWIN FIELDS / {}", stamp.region.name()),
            Label::Help => "E inventory / F3 debug / Esc menu".into(),
            Label::Notice if stamp.phase != Phase::Ready => "Preparing terrain...".into(),
            Label::Notice => notice.chars().take(110).collect(),
            Label::Selected => stamp.hotbar[stamp.selected_slot].name().into(),
            Label::MenuTitle => if stamp.menu == Menu::Creative {
                "Block inventory"
            } else {
                "Game paused"
            }
            .into(),
            Label::MenuSubtitle => {
                if stamp.menu == Menu::Creative {
                    format!(
                        "{} blocks / {} / slot {} selected",
                        Block::SOLID.len(),
                        if stamp.creative {
                            "unlimited supply"
                        } else {
                            "finite supplies"
                        },
                        stamp.selected_slot + 1
                    )
                } else {
                    "Double Space flies / Space up / Shift down".into()
                }
            }
            Label::CreativeHint => format!(
                "Click a block for slot {} / 1-9 select slot / E, F5 or Esc close",
                stamp.selected_slot + 1
            ),
            Label::SlotNumber(index) => (index + 1).to_string(),
            Label::SlotCount(index) => stamp.counts[index].to_string(),
            Label::Debug(index) => self.debug_lines[index].clone(),
            Label::Button(button) => match button {
                Button::Pause => "Back to game".into(),
                Button::Creative => "Block inventory [E / F5]".into(),
                Button::Travel => format!("Travel to {} [N]", stamp.region.other().name()),
                Button::Save => "Save world [F7]".into(),
                Button::Load => "Load world [F9]".into(),
                Button::Exit => "Quit game".into(),
                Button::Close => "Close".into(),
                Button::CreativeBlock(index) => Block::SOLID[index].name().into(),
                Button::Slot(_) => String::new(),
            },
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "real, independent diagnostic sources"
    )]
    pub fn sample_debug(
        &mut self,
        local: &Local,
        session: &Session,
        settings: &Settings,
        capture: &PointerCapture,
        mesh_count: usize,
        vertices: usize,
        triangles: usize,
    ) {
        let player = &session.game.player;
        let [x, y, z] = player.position;
        let chunk = player
            .position
            .map(|value| (value.floor() as i32).div_euclid(CHUNK_SIZE));
        let forward = player.forward();
        let facing = if forward[0].abs() > forward[2].abs() {
            if forward[0] > 0.0 { "+X" } else { "-X" }
        } else if forward[2] > 0.0 {
            "+Z"
        } else {
            "-Z"
        };
        let region = session.game.active_region();
        let interval = if self.debug_frames == 0 || self.debug_elapsed.is_zero() {
            "Host frame interval: unavailable".into()
        } else {
            format!(
                "Host frame interval: {:.2} ms average (not CPU or GPU execution time)",
                self.debug_elapsed.as_secs_f64() * 1000.0 / self.debug_frames as f64
            )
        };
        self.debug_lines = [
            format!(
                "TWIN FIELDS / {} / {:?} / {} / Engine 0.4.0",
                local.region.name(),
                local.phase,
                if session.game.creative {
                    "creative"
                } else {
                    "finite inventory"
                }
            ),
            format!(
                "XYZ: {x:.3} / {y:.3} / {z:.3}  / grounded: {} / flying: {}",
                player.grounded, local.flying
            ),
            format!(
                "Chunk: {} / {} / {}  / facing {facing} / yaw {:.1} / pitch {:.1}",
                chunk[0],
                chunk[1],
                chunk[2],
                player.yaw.to_degrees(),
                player.pitch.to_degrees()
            ),
            format!(
                "Seed: {} / held: {} / active hotbar slot: {}",
                session.game.seed(),
                session.game.inventory.selected().name(),
                session.game.inventory.selected_slot() + 1
            ),
            format!(
                "Chunks: {} / {} resident / {} pending / {} generated",
                region.resident_count(),
                MAX_RESIDENT_CHUNKS,
                region.pending_chunks(),
                region.generated_chunks()
            ),
            format!(
                "Persistent edits: {} / accepted edits this session: {} / chunk rebuilds: {}",
                region.edit_count(),
                session.edits,
                local.rebuilt_chunks
            ),
            format!(
                "Visible mesh sources: {mesh_count} objects / {vertices} vertices / {triangles} triangles (before GPU clipping)"
            ),
            interval,
            format!(
                "Native surfaces / {} [L] / fog {} [F] / mips {} [M] / {} [V]",
                if settings.lighting {
                    "Lambert"
                } else {
                    "Unlit"
                },
                settings.fog,
                settings.mipmaps,
                if settings.orthographic {
                    "orthographic"
                } else {
                    "perspective"
                }
            ),
            format!(
                "Mouse: {:?} (requested {}) / double Space: flight / GPU timings: desktop run report",
                capture.status(),
                capture.requested()
            ),
            session.game.target().map_or_else(
                || "Target: none within six blocks".into(),
                |hit| {
                    format!(
                        "Target: {} / cell {} / {} / {} / distance {:.2} blocks",
                        region.get(hit.cell).name(),
                        hit.cell[0],
                        hit.cell[1],
                        hit.cell[2],
                        hit.distance
                    )
                },
            ),
        ];
        self.debug_elapsed = Duration::ZERO;
        self.debug_frames = 0;
    }
}

pub fn panel_color(panel: Panel, local: &Local, session: &Session, hover: Option<Button>) -> Color {
    match panel {
        Panel::Hotbar => Color::rgba(0.025, 0.030, 0.035, 0.94),
        Panel::Shade => Color::rgba(0.0, 0.0, 0.0, 0.50),
        Panel::PauseBody | Panel::CreativeBody => Color::rgb8(28, 33, 35),
        Panel::PauseAccent | Panel::CreativeAccent | Panel::Selection(_) => {
            Color::rgb8(237, 185, 81)
        }
        Panel::DebugBacking => Color::rgba(0.015, 0.020, 0.025, 0.86),
        Panel::CrossHorizontal | Panel::CrossVertical => Color::rgb8(241, 240, 220),
        Panel::Icon(button, face) => {
            let block = match button {
                Button::Slot(index) => session.game.inventory.hotbar()[index],
                Button::CreativeBlock(index) => Block::SOLID[index],
                _ => Block::Stone,
            };
            block.color(match face {
                0 => 1,
                1 => 2,
                _ => 0,
            })
        }
        Panel::Button(button) => {
            if local.pointer.captured() == Some(button) {
                Color::rgb8(143, 103, 44)
            } else if hover == Some(button) {
                Color::rgb8(76, 92, 87)
            } else if matches!(button, Button::Slot(index) if index == session.game.inventory.selected_slot())
            {
                Color::rgb8(94, 78, 48)
            } else {
                Color::rgb8(49, 57, 56)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_notice_occurrence_restarts_its_visible_duration() {
        let mut cache = HudCache {
            notice: "Saved both worlds.".into(),
            notice_revision: 1,
            ..HudCache::default()
        };
        cache.tick(false, Duration::from_secs(5), "Saved both worlds.", 1);
        assert!(cache.notice_elapsed >= Duration::from_secs(4));
        cache.tick(false, Duration::from_millis(16), "Saved both worlds.", 2);
        assert_eq!(cache.notice_elapsed, Duration::ZERO);
        assert!(cache.debug_lines.iter().all(String::is_empty));
    }
}

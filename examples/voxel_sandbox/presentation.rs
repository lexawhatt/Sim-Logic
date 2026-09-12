//! Camera and HUD are derived from canonical game state, never used as storage.

use super::{
    app::Session,
    model::{Block, RegionId},
    scene::{Local, Phase, SelectionEdge},
    view::{self, Button, Label, Layout, Panel},
};
use sim_engine::{Rotation3d, Transform3d};
use sim_logic::prelude::*;

#[derive(PartialEq, Eq)]
struct Stamp {
    region: RegionId,
    phase: Phase,
    paused: bool,
    counts: [u16; 5],
    selected: Block,
    target: Option<([i32; 3], Block)>,
    edits: u64,
    rebuilt: u64,
}

#[derive(Default, Resource)]
pub struct HudCache {
    stamp: Option<Stamp>,
    notice: String,
}

fn caption(label: Label, stamp: &Stamp, notice: &str) -> String {
    match label {
        Label::Title => format!("TWIN FIELDS / {}", stamp.region.name()),
        Label::Status => format!(
            "Two persistent regions / {} edits / {} chunk rebuilds / N travel / P pause / Esc exit",
            stamp.edits, stamp.rebuilt
        ),
        Label::Help => {
            "WASD move / Arrows or middle-drag look / Space jump / LMB break / RMB build".into()
        }
        Label::Notice if stamp.phase != Phase::Ready => "Loading saved terrain...".into(),
        Label::Notice if stamp.paused => format!(
            "PAUSED / P to resume / {}",
            notice.chars().take(90).collect::<String>()
        ),
        Label::Notice => notice.chars().take(120).collect(),
        Label::Target => stamp
            .target
            .map(|(cell, block)| {
                format!("{} [{}, {}, {}]", block.name(), cell[0], cell[1], cell[2])
            })
            .unwrap_or_default(),
        Label::Button(button) => match button {
            Button::Travel => "Travel [N]".into(),
            Button::Pause => if stamp.paused {
                "Resume [P]"
            } else {
                "Pause [P]"
            }
            .into(),
            Button::Save => "F5 Save".into(),
            Button::Load => "F9 Load".into(),
            Button::Slot(index) => format!(
                "{} {}  {}",
                index + 1,
                Block::SOLID[index].name(),
                stamp.counts[index]
            ),
        },
    }
}

fn panel_color(
    panel: Panel,
    local: &Local,
    session: &Session,
    pointer: Option<PointerSample>,
) -> Color {
    match panel {
        Panel::Header | Panel::Footer => Color::rgba(0.035, 0.055, 0.078, 0.94),
        Panel::CrossHorizontal | Panel::CrossVertical => Color::WHITE,
        Panel::Button(button) => {
            if local.pointer.captured() == Some(button) {
                return Color::rgb8(145, 103, 41);
            }
            if let Button::Slot(index) = button
                && session.game.inventory.selected() == Block::SOLID[index]
            {
                return Color::rgb8(46, 108, 81);
            }
            if pointer.and_then(view::hit) == Some(button) {
                Color::rgb8(57, 78, 102)
            } else {
                Color::rgb8(34, 47, 63)
            }
        }
    }
}

#[allow(clippy::too_many_arguments, reason = "disjoint presentation-only data")]
pub fn present(
    viewport: FrameViewport,
    input: FrameInput<super::app::Action>,
    local: Res<Local>,
    session: AppRes<Session>,
    fonts: AppRes<view::Fonts>,
    mut view: ResMut<View3d>,
    mut cache: ResMut<HudCache>,
    mut panels: Query<(&Panel, &mut ScreenRectangleVisual)>,
    mut labels: Query<(&Label, &mut ScreenTextVisual)>,
    mut outline: Query<(&SelectionEdge, &mut CuboidVisual3d)>,
) -> LogicResult {
    let layout = Layout::new(viewport.logical());
    let ready = local.phase == Phase::Ready;
    let eye = session.game.player.eye();
    let forward = session.game.player.forward();
    view.set_pose(
        Vec3::new(eye[0], eye[1], eye[2])?,
        Vec3::new(
            eye[0] + forward[0],
            eye[1] + forward[1],
            eye[2] + forward[2],
        )?,
    )?;
    view.set_enabled(ready);
    let target = ready.then(|| session.game.target()).flatten();
    let stamp = Stamp {
        region: local.region,
        phase: local.phase,
        paused: local.paused,
        counts: Block::SOLID.map(|block| session.game.inventory.count(block)),
        selected: session.game.inventory.selected(),
        target: target.map(|hit| (hit.cell, session.game.active_region().get(hit.cell))),
        edits: session.edits,
        rebuilt: local.rebuilt_chunks,
    };
    let changed = cache.stamp.as_ref() != Some(&stamp) || cache.notice != session.notice;
    for (label, mut visual) in &mut labels {
        let (position, _) = layout.label(*label);
        visual.set_position(position)?;
        visual.set_font(fonts.at(layout).clone())?;
        if changed {
            visual.set_text(&caption(*label, &stamp, &session.notice))?;
        }
    }
    if changed {
        cache.stamp = Some(stamp);
        cache.notice.clone_from(&session.notice);
    }
    for (panel, mut visual) in &mut panels {
        let [x, y, width, height] = layout.panel(*panel);
        visual.set_geometry(
            LogicalScreenPosition::new(x, y),
            LogicalScreenVector::new(width, height),
        )?;
        visual.set_color(panel_color(*panel, &local, &session, input.pointer()))?;
    }
    for (edge, mut visual) in &mut outline {
        visual.set_visible(target.is_some() && !local.paused);
        if let Some(hit) = target {
            // Twelve thin opaque cuboids form a true outline, not a solid
            // replacement cube hiding the selected block's material.
            let axis = edge.0 / 4;
            let bits = edge.0 % 4;
            let mut center = hit.cell.map(|value| value as f32);
            let mut size = [0.014; 3];
            size[axis] = 1.014;
            center[axis] += 0.5;
            center[(axis + 1) % 3] += (bits & 1) as f32;
            center[(axis + 2) % 3] += ((bits >> 1) & 1) as f32;
            visual.set_transform(Transform3d::new(
                Vec3::new(center[0], center[1], center[2])?,
                Rotation3d::IDENTITY,
                Vec3::new(size[0], size[1], size[2])?,
            )?)?;
        }
    }
    Ok(())
}

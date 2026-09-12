//! Camera and HUD are derived from canonical state, never used as storage.

use super::{
    app::Session,
    materials::{self, Settings},
    scene::{Local, Phase, SelectionEdge},
    view::{
        self, Label, Layout, Menu, Panel,
        content::{Stamp, panel_color},
    },
};
use sim_engine::{Rotation3d, Transform3d};
use sim_logic::prelude::*;

pub use super::view::content::HudCache;

#[allow(clippy::too_many_arguments, reason = "disjoint presentation-only data")]
pub fn present(
    viewport: FrameViewport,
    time: FrameTime,
    input: FrameInput<super::app::Action>,
    local: Res<Local>,
    session: AppRes<Session>,
    fonts: AppRes<view::Fonts>,
    settings: Res<Settings>,
    capture: AppRes<PointerCapture>,
    mut view: ResMut<View3d>,
    mut cache: ResMut<HudCache>,
    mut panels: Query<(LogicEntityRef, &Panel, Option<&mut ScreenRectangleVisual>)>,
    mut labels: Query<(LogicEntityRef, &Label, Option<&mut ScreenTextVisual>)>,
    mut outline: Query<(&SelectionEdge, &mut CuboidVisual3d)>,
    meshes: Query<&MeshVisual3d>,
    mut commands: Commands,
) -> LogicResult {
    let layout = Layout::new(viewport.logical());
    let ready = local.phase == Phase::Ready;
    camera(&mut view, &local, &session, &settings)?;
    let target = ready.then(|| session.game.target()).flatten();
    let stamp = Stamp::new(&local, &session);
    let debug_due = cache.tick(
        local.debug,
        time.delta(),
        &session.notice,
        session.notice_revision,
    );
    let changed = cache.changed(&stamp, &session.notice, session.notice_revision);
    if debug_due {
        let (mut objects, mut vertices, mut triangles) = (0_usize, 0_usize, 0_usize);
        if ready {
            for mesh in &meshes {
                if mesh.visible() {
                    objects = objects.saturating_add(1);
                    vertices = vertices.saturating_add(mesh.asset().mesh().vertices().len());
                    triangles = triangles.saturating_add(mesh.asset().mesh().triangle_count());
                }
            }
        }
        cache.sample_debug(
            &local, &session, &settings, &capture, objects, vertices, triangles,
        );
    }

    // Lazily reuse one parsed face/plan per changed font batch. Hidden labels
    // never format or shape. Sessions borrow registrations only for this call.
    let mut preparations: [Option<TextPreparationSession<'_>>; 3] = [None, None, None];
    for (entity, label, visual) in &mut labels {
        if !cache.visible(*label, &local, &session) {
            if visual.is_some() {
                commands.remove::<ScreenTextVisual>(entity.handle())?;
            }
            continue;
        }
        let font_index = fonts.index(*label, layout);
        let font = &fonts.0[font_index];
        let caption_changed = if matches!(label, Label::Debug(_)) {
            debug_due
        } else {
            changed
        };
        let needs_text =
            caption_changed || visual.as_ref().is_none_or(|value| value.font() != font);
        if needs_text {
            let preparation = &mut preparations[font_index];
            if preparation.is_none() {
                *preparation = Some(font.shaping_session()?);
            }
            let shaping = preparation.as_mut().ok_or("missing HUD shaping session")?;
            let caption = cache.caption(*label, &stamp, &session.notice);
            if let Some(mut visual) = visual {
                if visual.font() != font {
                    *visual = view::text(shaping, *label, &caption, layout, local.menu)?;
                } else {
                    let (position, alignment) = layout.label(*label, local.menu);
                    visual.set_position(position)?;
                    visual.set_alignment(alignment)?;
                    visual.set_text_with_session(shaping, &caption)?;
                }
            } else {
                // Query actual component presence, not cached requested state:
                // a discarded command batch must retry an opening next frame.
                commands.insert(
                    entity.handle(),
                    view::text(shaping, *label, &caption, layout, local.menu)?,
                )?;
            }
        } else if let Some(mut visual) = visual {
            let (position, alignment) = layout.label(*label, local.menu);
            visual.set_position(position)?;
            visual.set_alignment(alignment)?;
        }
    }
    cache.publish(stamp, &session.notice, session.notice_revision);

    let hover = if capture.requested() && local.menu == Menu::None {
        None
    } else {
        input
            .pointer()
            .and_then(|pointer| view::hit(pointer, local.menu))
    };
    for (entity, panel, visual) in &mut panels {
        if !view::panel_visible(
            *panel,
            local.menu,
            local.debug,
            session.game.inventory.selected_slot(),
        ) {
            if visual.is_some() {
                commands.remove::<ScreenRectangleVisual>(entity.handle())?;
            }
            continue;
        }
        let color = panel_color(*panel, &local, &session, hover);
        if let Some(mut visual) = visual {
            let [x, y, width, height] = layout.panel(*panel);
            visual.set_geometry(
                LogicalScreenPosition::new(x, y),
                LogicalScreenVector::new(width, height),
            )?;
            visual.set_color(color)?;
        } else {
            commands.insert(entity.handle(), view::rectangle(*panel, layout, color)?)?;
        }
    }
    for (edge, mut visual) in &mut outline {
        visual.set_visible(target.is_some() && local.menu == Menu::None && !local.paused);
        if let Some(hit) = target {
            // Twelve thin opaque cuboids outline the selected block without
            // covering its textured, masked or transparent surface.
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

fn camera(view: &mut View3d, local: &Local, session: &Session, settings: &Settings) -> LogicResult {
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
    view.set_enabled(local.phase == Phase::Ready);
    if settings.orthographic != view.orthographic_span().is_some() {
        if settings.orthographic {
            view.set_orthographic(
                WorldLength::new(12.0)?,
                WorldLength::new(0.1)?,
                WorldLength::new(1000.0)?,
            )?;
        } else {
            view.set_perspective(
                std::f32::consts::FRAC_PI_3,
                WorldLength::new(0.1)?,
                WorldLength::new(1000.0)?,
            )?;
        }
    }
    let (lighting, fog) = materials::environment(local.region)?;
    view.set_lighting(lighting);
    view.set_fog(settings.fog.then_some(fog));
    Ok(())
}

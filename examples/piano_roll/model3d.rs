//! A small original upright piano made from genuine depth-tested cuboids.

use super::{
    app::{Session, View},
    control::Control,
    layout::black_key,
    music::{MAX_PITCH, MIN_PITCH},
    view::BACKGROUND,
};
use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, Component)]
pub struct Part {
    pub rest: CuboidVisual3d,
    pub pitch: Option<u8>,
}

pub fn camera() -> LogicResult<View3d> {
    let mut view = View3d::new(Vec3::new(8.5, 7.0, 13.0)?, Vec3::new(0.0, 1.6, 0.0)?)?;
    view.set_perspective(0.75, WorldLength::new(0.1)?, WorldLength::new(1000.0)?)?;
    view.set_background(BACKGROUND)?;
    Ok(view)
}

pub fn parts() -> LogicResult<Vec<(Part, CuboidVisual3d)>> {
    let mut parts = Vec::with_capacity(56);
    let mut add = |center: [f32; 3], size: [f32; 3], color: Color, pitch| -> LogicResult {
        let [x, y, z] = center;
        let [w, h, d] = size;
        let visual = CuboidVisual3d::new(Vec3::new(x, y, z)?, Vec3::new(w, h, d)?, color)?;
        parts.push((
            Part {
                rest: visual,
                pitch,
            },
            visual,
        ));
        Ok(())
    };
    let wood = Color::rgb8(72, 48, 45);
    let dark = Color::rgb8(35, 28, 33);
    let brass = Color::rgb8(202, 150, 81);
    add(
        [0.0, 0.04, 0.0],
        [12.0, 0.08, 7.0],
        Color::rgb8(27, 31, 40),
        None,
    )?;
    add([0.0, 2.65, -0.50], [9.25, 3.25, 1.65], wood, None)?;
    add([0.0, 4.34, -0.50], [9.7, 0.15, 1.95], dark, None)?;
    add([0.0, 2.28, 0.58], [9.7, 0.36, 2.75], dark, None)?;
    add(
        [0.0, 3.28, 0.36],
        [8.7, 1.40, 0.14],
        Color::rgb8(102, 65, 52),
        None,
    )?;
    add([0.0, 2.80, 0.47], [8.45, 0.06, 0.05], brass, None)?;
    add([0.0, 2.53, 0.51], [8.45, 0.08, 0.16], brass, None)?;
    for x in [-4.48, 4.48] {
        add([x, 1.15, 1.38], [0.40, 2.30, 0.47], wood, None)?;
        add([x, 2.61, 0.83], [0.40, 0.65, 2.18], wood, None)?;
    }
    for x in [-0.56, 0.0, 0.56] {
        add([x, 0.35, 0.98], [0.27, 0.12, 0.84], brass, None)?;
    }
    add([0.0, 1.10, 2.98], [4.9, 0.30, 1.35], dark, None)?;
    for x in [-2.02, 2.02] {
        add([x, 0.54, 2.98], [0.24, 1.08, 0.92], wood, None)?;
    }
    let mut white = 0;
    for pitch in MIN_PITCH..=MAX_PITCH {
        let is_black = black_key(pitch);
        let x = -4.2
            + if is_black {
                white as f32 * 0.40
            } else {
                (white as f32 + 0.5) * 0.40
            };
        if !is_black {
            white += 1;
        }
        add(
            [
                x,
                if is_black { 2.62 } else { 2.48 },
                if is_black { 0.91 } else { 1.16 },
            ],
            [
                if is_black { 0.24 } else { 0.38 },
                if is_black { 0.20 } else { 0.16 },
                if is_black { 0.88 } else { 1.38 },
            ],
            if is_black {
                Color::rgb8(20, 24, 31)
            } else {
                Color::rgb8(232, 223, 204)
            },
            Some(pitch),
        )?;
    }
    Ok(parts)
}

pub fn update(
    session: AppRes<Session>,
    control: AppRes<Control>,
    viewport: FrameViewport,
    mut view: ResMut<View3d>,
    mut parts: Query<(&Part, &mut CuboidVisual3d)>,
) -> LogicResult {
    let enabled = session.view == View::ThreeD;
    view.set_enabled(enabled);
    if !enabled {
        return Ok(());
    }
    // Engine 0.2 requires solid triangles to remain wholly inside the frustum.
    // Dolly out for portrait windows; the fixed 2D UI letterboxes the same way.
    let aspect = viewport.logical().width() / viewport.logical().height();
    let fit = (1.6 / aspect).max(1.0);
    view.set_pose(
        Vec3::new(8.5 * fit, 1.6 + 5.4 * fit, 13.0 * fit)?,
        Vec3::new(0.0, 1.6, 0.0)?,
    )?;
    view.set_perspective(
        0.75,
        WorldLength::new(0.1)?,
        WorldLength::new(1000.0 * fit)?,
    )?;
    let active = control.snapshot().active_keys;
    for (part, mut visual) in &mut parts {
        let Some(pitch) = part.pitch else {
            continue;
        };
        let pressed = active & (1 << (pitch - MIN_PITCH)) != 0;
        let rest = part.rest.transform();
        let position = rest.translation();
        let transform = Transform3d::new(
            Vec3::new(
                position.x(),
                position.y() - if pressed { 0.07 } else { 0.0 },
                position.z(),
            )?,
            rest.rotation(),
            rest.scale(),
        )?;
        if visual.transform() != transform {
            visual.set_transform(transform)?;
        }
        let color = if pressed {
            Color::rgb8(105, 232, 186)
        } else {
            part.rest.color()
        };
        if visual.color() != color {
            visual.set_color(color)?;
        }
    }
    Ok(())
}

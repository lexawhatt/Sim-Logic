//! Map-only coordinates. Neither the fixed HUD nor the simulation moves.

use sim_logic::prelude::*;

use super::layout::{self, Area, Layout, MAP};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapView {
    zoom: f64,
    // Original design-map coordinate at the viewport center.
    center: (f64, f64),
}

impl Default for MapView {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            center: (
                f64::from(MAP.x + MAP.width * 0.5),
                f64::from(MAP.y + MAP.height * 0.5),
            ),
        }
    }
}

impl MapView {
    pub fn zoom(self) -> f32 {
        self.zoom as f32
    }

    pub fn project(self, x: f32, y: f32) -> (f32, f32) {
        (
            (f64::from(MAP.x + MAP.width * 0.5) + (f64::from(x) - self.center.0) * self.zoom)
                as f32,
            (f64::from(MAP.y + MAP.height * 0.5) + (f64::from(y) - self.center.1) * self.zoom)
                as f32,
        )
    }

    fn unproject(self, x: f32, y: f32) -> (f64, f64) {
        (
            self.center.0 + (f64::from(x) - f64::from(MAP.x + MAP.width * 0.5)) / self.zoom,
            self.center.1 + (f64::from(y) - f64::from(MAP.y + MAP.height * 0.5)) / self.zoom,
        )
    }

    pub fn cell_at(self, x: f32, y: f32) -> Option<usize> {
        if !MAP.contains(x, y) {
            return None;
        }
        let (x, y) = self.unproject(x, y);
        // Keep the half-open cell test in f64: rounding a right-edge point to
        // f32 must not turn a valid hit into WIDTH or HEIGHT.
        let column = ((x - f64::from(MAP.x)) / f64::from(layout::CELL)).floor();
        let row = ((y - f64::from(MAP.y)) / f64::from(layout::CELL)).floor();
        use super::simulation::{HEIGHT, WIDTH};
        (column >= 0.0 && column < WIDTH as f64 && row >= 0.0 && row < HEIGHT as f64)
            .then_some(row as usize * WIDTH + column as usize)
    }

    pub fn scroll(&mut self, x: f32, y: f32, delta: ScrollDelta) {
        if !MAP.contains(x, y) {
            return;
        }
        let anchor = self.unproject(x, y);
        // A line is a wheel notch; pixel deltas are already logical pixels.
        // Clamp the exponent before powf, including absurd but valid devices.
        let steps = match delta.unit() {
            ScrollUnit::Lines => delta.y(),
            ScrollUnit::Pixels => delta.y() / 48.0,
        };
        self.zoom = (self.zoom * 1.2_f64.powf(steps.clamp(-32.0, 32.0))).clamp(1.0, 8.0);
        let after = self.unproject(x, y);
        self.center.0 += anchor.0 - after.0;
        self.center.1 += anchor.1 - after.1;
        self.clamp_center();
    }

    fn pan(&mut self, dx: f64, dy: f64) {
        self.center.0 -= dx / self.zoom;
        self.center.1 -= dy / self.zoom;
        self.clamp_center();
    }

    fn clamp_center(&mut self) {
        let half_w = f64::from(MAP.width) / (2.0 * self.zoom);
        let half_h = f64::from(MAP.height) / (2.0 * self.zoom);
        self.center.0 = self.center.0.clamp(
            f64::from(MAP.x) + half_w,
            f64::from(MAP.x + MAP.width) - half_w,
        );
        self.center.1 = self.center.1.clamp(
            f64::from(MAP.y) + half_h,
            f64::from(MAP.y + MAP.height) - half_h,
        );
    }

    /// Transform a map-space rectangle and clip it before it enters the pool.
    pub fn rectangle(self, area: Area) -> Option<Area> {
        let (x, y) = self.project(area.x, area.y);
        clip(Area::new(
            x,
            y,
            area.width * self.zoom(),
            area.height * self.zoom(),
        ))
    }
}

pub fn clip(area: Area) -> Option<Area> {
    let right = (area.x + area.width).min(MAP.x + MAP.width);
    let bottom = (area.y + area.height).min(MAP.y + MAP.height);
    let x = area.x.max(MAP.x);
    let y = area.y.max(MAP.y);
    (right > x && bottom > y).then_some(Area::new(x, y, right - x, bottom - y))
}

#[derive(Default)]
pub struct MapDrag {
    last: Option<PointerSample>,
}

impl MapDrag {
    pub fn begin(&mut self, pointer: Option<PointerSample>) {
        self.last = pointer.filter(|p| {
            let (x, y) = Layout::new(p.viewport()).unproject(p.position());
            MAP.contains(x, y)
        });
    }

    pub fn cancel(&mut self) {
        self.last = None;
    }

    pub fn advance(&mut self, pointer: Option<PointerSample>, view: &mut MapView) {
        let (Some(last), Some(next)) = (self.last, pointer) else {
            self.cancel();
            return;
        };
        if last.viewport() != next.viewport() {
            self.cancel();
            return;
        }
        let layout = Layout::new(last.viewport());
        let (x0, y0) = layout.unproject(last.position());
        let (x1, y1) = layout.unproject(next.position());
        if ![x0, y0, x1, y1].into_iter().all(f32::is_finite) {
            self.cancel();
            return;
        }
        // Continue outside the map; only the starting point captures a drag.
        view.pan(f64::from(x1) - f64::from(x0), f64::from(y1) - f64::from(y0));
        self.last = Some(next);
    }
}

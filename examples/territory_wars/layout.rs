//! One letterboxed coordinate system for both drawing and event-time picking.

use sim_logic::prelude::*;

use super::simulation::{HEIGHT, WIDTH};

pub const DESIGN_WIDTH: f32 = 1440.0;
pub const DESIGN_HEIGHT: f32 = 900.0;
pub const MAP_X: f32 = 24.0;
pub const MAP_Y: f32 = 112.0;
pub const CELL: f32 = 10.0;

#[derive(Debug, Clone, Copy)]
pub struct Area {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Area {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }
}

pub const MAP: Area = Area::new(MAP_X, MAP_Y, WIDTH as f32 * CELL, HEIGHT as f32 * CELL);
pub const SIDEBAR: Area = Area::new(1008.0, 112.0, 408.0, 640.0);
pub const NEW_MAP: Area = Area::new(1080.0, 24.0, 164.0, 44.0);
pub const PAUSE: Area = Area::new(1260.0, 24.0, 156.0, 44.0);
pub const EXPAND: Area = Area::new(692.0, 798.0, 220.0, 54.0);
pub const DEBUG: Area = Area::new(936.0, 798.0, 208.0, 54.0);
pub const SLIDER: Area = Area::new(264.0, 802.0, 384.0, 50.0);

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    scale: f32,
    offset_x: f32,
    offset_y: f32,
}

impl Layout {
    pub fn new(viewport: LogicalViewport) -> Self {
        let scale = (viewport.width() / DESIGN_WIDTH).min(viewport.height() / DESIGN_HEIGHT);
        Self {
            scale,
            offset_x: (viewport.width() - DESIGN_WIDTH * scale) * 0.5,
            offset_y: (viewport.height() - DESIGN_HEIGHT * scale) * 0.5,
        }
    }

    pub fn position(self, x: f32, y: f32) -> LogicalScreenPosition {
        LogicalScreenPosition::new(
            self.offset_x + x * self.scale,
            self.offset_y + y * self.scale,
        )
    }

    pub fn size(self, width: f32, height: f32) -> LogicalScreenVector {
        LogicalScreenVector::new(width * self.scale, height * self.scale)
    }

    pub fn unproject(self, position: LogicalScreenPosition) -> (f32, f32) {
        let point = position.to_vec2();
        (
            (point.x() - self.offset_x) / self.scale,
            (point.y() - self.offset_y) / self.scale,
        )
    }
}

pub fn cell_at(x: f32, y: f32) -> Option<usize> {
    MAP.contains(x, y).then(|| {
        let column = ((x - MAP_X) / CELL) as usize;
        let row = ((y - MAP_Y) / CELL) as usize;
        row * WIDTH + column
    })
}

pub fn cell_center(cell: usize) -> (f32, f32) {
    (
        MAP_X + (cell % WIDTH) as f32 * CELL + CELL * 0.5,
        MAP_Y + (cell / WIDTH) as f32 * CELL + CELL * 0.5,
    )
}

pub fn attack_percent(x: f32) -> u8 {
    let fraction = ((x - SLIDER.x) / SLIDER.width).clamp(0.0, 1.0);
    ((fraction * 19.0).round() as u8 + 1) * 5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letterbox_round_trip_and_half_open_map() -> LogicResult {
        for (width, height) in [(1440.0, 900.0), (700.0, 1000.0), (1.0, 1.0)] {
            let layout = Layout::new(LogicalViewport::new(width, height)?);
            let (x, y) = layout.unproject(layout.position(500.0, 300.0));
            assert!((x - 500.0).abs() < 0.01 && (y - 300.0).abs() < 0.01);
        }
        assert_eq!(cell_at(MAP_X, MAP_Y), Some(0));
        assert_eq!(cell_at(MAP_X + MAP.width, MAP_Y), None);
        assert_eq!(cell_at(MAP_X, MAP_Y + MAP.height), None);
        assert_eq!(cell_at(f32::NAN, 200.0), None);
        assert_eq!(attack_percent(-100.0), 5);
        assert_eq!(attack_percent(9000.0), 100);
        Ok(())
    }
}

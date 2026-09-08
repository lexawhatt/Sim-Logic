//! One shared logical layout for drawing and event-time hit testing.

use super::music::{MAX_PITCH, MIN_PITCH, STEP_COUNT};
use sim_logic::prelude::*;

pub const WIDTH: f32 = 1440.0;
pub const HEIGHT: f32 = 900.0;
pub const GRID: Area = Area::new(108.0, 230.0, 1272.0, 396.0);
pub const KEYBOARD: Area = Area::new(108.0, 714.0, 1272.0, 116.0);

#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub fn contains(self, point: Vec2) -> bool {
        point.x() >= self.x
            && point.x() < self.x + self.width
            && point.y() >= self.y
            && point.y() < self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Play,
    Stop,
    Demo,
    Clear,
    TempoDown,
    TempoUp,
    LengthDown,
    LengthUp,
    VolumeDown,
    VolumeUp,
    View2d,
    View3d,
}
pub const BUTTONS: [Button; 12] = [
    Button::Play,
    Button::Stop,
    Button::Demo,
    Button::Clear,
    Button::TempoDown,
    Button::TempoUp,
    Button::LengthDown,
    Button::LengthUp,
    Button::VolumeDown,
    Button::VolumeUp,
    Button::View2d,
    Button::View3d,
];
impl Button {
    pub const fn area(self) -> Area {
        match self {
            Self::Play => Area::new(40.0, 96.0, 124.0, 48.0),
            Self::Stop => Area::new(176.0, 96.0, 112.0, 48.0),
            Self::Demo => Area::new(328.0, 96.0, 104.0, 48.0),
            Self::Clear => Area::new(444.0, 96.0, 104.0, 48.0),
            Self::TempoDown => Area::new(672.0, 96.0, 38.0, 48.0),
            Self::TempoUp => Area::new(790.0, 96.0, 38.0, 48.0),
            Self::LengthDown => Area::new(964.0, 96.0, 38.0, 48.0),
            Self::LengthUp => Area::new(1062.0, 96.0, 38.0, 48.0),
            Self::VolumeDown => Area::new(156.0, 162.0, 32.0, 30.0),
            Self::VolumeUp => Area::new(264.0, 162.0, 32.0, 30.0),
            Self::View2d => Area::new(1188.0, 96.0, 90.0, 48.0),
            Self::View3d => Area::new(1290.0, 96.0, 90.0, 48.0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub scale: f32,
    pub origin: Vec2,
}
impl Layout {
    pub fn new(viewport: LogicalViewport) -> Self {
        let scale = (viewport.width() / WIDTH).min(viewport.height() / HEIGHT);
        Self {
            scale,
            origin: Vec2::new(
                (viewport.width() - WIDTH * scale) * 0.5,
                (viewport.height() - HEIGHT * scale) * 0.5,
            ),
        }
    }
    pub fn unproject(self, point: LogicalScreenPosition) -> Vec2 {
        (point.to_vec2() - self.origin) / self.scale
    }
    pub fn position(self, x: f32, y: f32) -> LogicalScreenPosition {
        LogicalScreenPosition::new(
            self.origin.x() + x * self.scale,
            self.origin.y() + y * self.scale,
        )
    }
    pub fn size(self, width: f32, height: f32) -> LogicalScreenVector {
        LogicalScreenVector::new(width * self.scale, height * self.scale)
    }
}

pub fn grid_cell(point: Vec2) -> Option<(u8, u8)> {
    GRID.contains(point).then(|| {
        let step = ((point.x() - GRID.x) / (GRID.width / f32::from(STEP_COUNT))) as u8;
        let row = ((point.y() - GRID.y) / (GRID.height / 36.0)) as u8;
        (MAX_PITCH - row.min(35), step.min(STEP_COUNT - 1))
    })
}

pub const fn black_key(pitch: u8) -> bool {
    matches!(pitch % 12, 1 | 3 | 6 | 8 | 10)
}

pub fn key_area(pitch: u8) -> Area {
    let white_before = (MIN_PITCH..pitch)
        .filter(|pitch| !black_key(*pitch))
        .count() as f32;
    let width = KEYBOARD.width / 21.0;
    if black_key(pitch) {
        Area::new(
            KEYBOARD.x + white_before * width - width * 0.30,
            KEYBOARD.y,
            width * 0.60,
            KEYBOARD.height * 0.62,
        )
    } else {
        Area::new(
            KEYBOARD.x + white_before * width,
            KEYBOARD.y,
            width - 2.0,
            KEYBOARD.height,
        )
    }
}

pub fn keyboard_pitch(point: Vec2) -> Option<u8> {
    // Black keys overlap whites and must win hit testing, just as they draw last.
    (MIN_PITCH..=MAX_PITCH)
        .filter(|pitch| black_key(*pitch))
        .chain((MIN_PITCH..=MAX_PITCH).filter(|pitch| !black_key(*pitch)))
        .find(|pitch| key_area(*pitch).contains(point))
}

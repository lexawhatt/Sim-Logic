//! Responsive screen-space UI. Hit tests use each occurrence's own viewport.

use sim_engine::{Layer, SceneBudget};
use sim_logic::prelude::*;

pub const FONT: &[u8] = include_bytes!("../../tests/assets/text/DejaVuSans.ttf");
pub const MAX_PANELS: usize = 13;
pub const MAX_LABELS: usize = 14;

/// Three bounded font registrations avoid resizing glyphs with a GPU workaround.
pub struct Fonts(pub [TextFont; 3]);

impl Fonts {
    pub fn at(&self, layout: Layout) -> &TextFont {
        &self.0[if layout.scale >= 0.93 {
            0
        } else if layout.scale >= 0.67 {
            1
        } else {
            2
        }]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Button {
    Travel,
    Pause,
    Save,
    Load,
    Slot(usize),
}

pub const BUTTONS: [Button; 9] = [
    Button::Travel,
    Button::Pause,
    Button::Save,
    Button::Load,
    Button::Slot(0),
    Button::Slot(1),
    Button::Slot(2),
    Button::Slot(3),
    Button::Slot(4),
];

#[derive(Debug, Clone, Copy, Component)]
pub enum Panel {
    Header,
    Footer,
    CrossHorizontal,
    CrossVertical,
    Button(Button),
}

#[derive(Debug, Clone, Copy, Component)]
pub enum Label {
    Title,
    Status,
    Help,
    Notice,
    Target,
    Button(Button),
}

#[derive(Clone, Copy)]
pub struct Layout {
    width: f32,
    height: f32,
    scale: f32,
}

impl Layout {
    pub fn new(viewport: LogicalViewport) -> Self {
        let scale = (viewport.width() / 1100.0)
            .min(viewport.height() / 720.0)
            .min(1.5);
        Self {
            width: viewport.width() / scale,
            height: viewport.height() / scale,
            scale,
        }
    }

    pub fn button(self, button: Button) -> [f32; 4] {
        match button {
            Button::Travel => [self.width - 426.0, 16.0, 122.0, 40.0],
            Button::Pause => [self.width - 294.0, 16.0, 94.0, 40.0],
            Button::Save => [self.width - 190.0, 16.0, 78.0, 40.0],
            Button::Load => [self.width - 102.0, 16.0, 78.0, 40.0],
            Button::Slot(index) => [
                self.width * 0.5 - 300.0 + index as f32 * 122.0,
                self.height - 76.0,
                112.0,
                48.0,
            ],
        }
    }

    pub fn panel(self, panel: Panel) -> [f32; 4] {
        let rect = match panel {
            Panel::Header => [0.0, 0.0, self.width, 106.0],
            Panel::Footer => [0.0, self.height - 100.0, self.width, 100.0],
            Panel::CrossHorizontal => [self.width * 0.5 - 8.0, self.height * 0.5 - 1.0, 16.0, 2.0],
            Panel::CrossVertical => [self.width * 0.5 - 1.0, self.height * 0.5 - 8.0, 2.0, 16.0],
            Panel::Button(button) => self.button(button),
        };
        rect.map(|value| value * self.scale)
    }

    pub fn label(self, label: Label) -> (LogicalScreenPosition, TextAlignment) {
        let (x, y, alignment) = match label {
            Label::Title => (24.0, 40.0, TextAlignment::Left),
            Label::Status => (24.0, 68.0, TextAlignment::Left),
            Label::Help => (24.0, 91.0, TextAlignment::Left),
            Label::Notice => (self.width * 0.5, self.height - 115.0, TextAlignment::Center),
            Label::Target => (
                self.width * 0.5,
                self.height * 0.5 + 34.0,
                TextAlignment::Center,
            ),
            Label::Button(button) => {
                let [x, y, w, _] = self.button(button);
                (x + w * 0.5, y + 29.0, TextAlignment::Center)
            }
        };
        (
            LogicalScreenPosition::new(x * self.scale, y * self.scale),
            alignment,
        )
    }
}

pub fn hit(pointer: PointerSample) -> Option<Button> {
    let point = pointer.position().to_vec2();
    let viewport = pointer.viewport();
    if point.x() < 0.0
        || point.y() < 0.0
        || point.x() >= viewport.width()
        || point.y() >= viewport.height()
    {
        return None;
    }
    let layout = Layout::new(viewport);
    BUTTONS.into_iter().find(|button| {
        let [x, y, w, h] = layout.panel(Panel::Button(*button));
        point.x() >= x && point.y() >= y && point.x() < x + w && point.y() < y + h
    })
}

pub fn game_area(pointer: PointerSample) -> bool {
    let layout = Layout::new(pointer.viewport());
    let p = pointer.position().to_vec2();
    p.x() >= 0.0
        && p.x() < pointer.viewport().width()
        && p.y() >= 106.0 * layout.scale
        && p.y() < pointer.viewport().height() - 100.0 * layout.scale
}

pub fn rectangle(panel: Panel, layout: Layout, color: Color) -> LogicResult<ScreenRectangleVisual> {
    let [x, y, w, h] = layout.panel(panel);
    let mut value = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, y),
        LogicalScreenVector::new(w, h),
        color,
    )?;
    value.set_layer(Layer::new(if matches!(panel, Panel::Button(_)) {
        2
    } else {
        1
    }));
    Ok(value)
}

pub fn text(
    font: &TextFont,
    label: Label,
    content: &str,
    layout: Layout,
) -> LogicResult<ScreenTextVisual> {
    let (position, alignment) = layout.label(label);
    let mut value = ScreenTextVisual::new(font.clone(), content, position)?;
    value.set_alignment(alignment)?;
    value.set_layer(Layer::new(3));
    value.set_tint(Color::rgb8(226, 236, 244))?;
    Ok(value)
}

pub fn limits() -> RenderLimits {
    // Engine budgets an AA filled rectangle at twelve tessellated vertices.
    let scene = SceneBudget::new(
        MAX_PANELS,
        0,
        MAX_PANELS * 12,
        64 * 1024,
        64 * 1024,
        64 * 1024,
        MAX_PANELS,
    );
    RenderLimits::new(
        0,
        scene,
        FrameLimits::new(64, 128, 16_384, 2 * 1024 * 1024, 128 * 1024 * 1024, 128),
    )
    .with_max_world_rectangles(0)
    .with_max_world_lines(0)
    .with_max_screen_rectangles(MAX_PANELS)
    .with_screen_scene_budget(scene)
    .with_max_screen_texts(MAX_LABELS)
    .with_max_screen_text_bytes(4096)
    .with_max_screen_text_glyphs(2048)
}

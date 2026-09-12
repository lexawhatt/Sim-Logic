//! Original block-game UI made from bounded code-native geometry and text.

use sim_engine::{Layer, SceneBudget};
use sim_logic::prelude::*;

use super::model::Block;

#[path = "view/content.rs"]
pub mod content;
#[path = "view/layout.rs"]
mod layout;
pub use layout::Layout;

pub const FONT: &[u8] = include_bytes!("../../tests/assets/text/DejaVuSans.ttf");
pub const HOTBAR_SLOTS: usize = 9;
pub const DEBUG_LINES: usize = 11;
pub const MAX_PANELS: usize = 256;
pub const MAX_LABELS: usize = 96;

/// Three bounded registrations avoid resizing glyphs with a GPU workaround.
pub struct Fonts(pub [TextFont; 3]);

impl Fonts {
    pub fn index(&self, label: Label, layout: Layout) -> usize {
        let large = matches!(label, Label::MenuTitle | Label::Selected)
            || matches!(label, Label::Button(button) if !matches!(button, Button::CreativeBlock(_)));
        if large && layout.scale() >= 0.93 {
            0
        } else if (large && layout.scale() >= 0.67) || layout.scale() >= 0.93 {
            1
        } else {
            2
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Menu {
    #[default]
    None,
    Pause,
    Creative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum Button {
    Travel,
    Pause,
    Save,
    Load,
    Exit,
    Creative,
    Close,
    Slot(usize),
    CreativeBlock(usize),
}

pub const MENU_BUTTONS: [Button; 6] = [
    Button::Pause,
    Button::Creative,
    Button::Travel,
    Button::Save,
    Button::Load,
    Button::Exit,
];

#[derive(Debug, Clone, Copy, Component)]
pub enum Panel {
    Hotbar,
    Shade,
    PauseBody,
    CreativeBody,
    PauseAccent,
    CreativeAccent,
    DebugBacking,
    CrossHorizontal,
    CrossVertical,
    Selection(usize),
    Button(Button),
    Icon(Button, u8),
}

#[derive(Debug, Clone, Copy, Component)]
pub enum Label {
    Title,
    Help,
    Notice,
    Selected,
    MenuTitle,
    MenuSubtitle,
    CreativeHint,
    SlotNumber(usize),
    SlotCount(usize),
    Debug(usize),
    Button(Button),
}

pub fn button_visible(button: Button, menu: Menu) -> bool {
    match button {
        Button::Slot(index) => index < HOTBAR_SLOTS && menu != Menu::Pause,
        Button::CreativeBlock(index) => index < Block::SOLID.len() && menu == Menu::Creative,
        Button::Close => menu == Menu::Creative,
        _ => menu == Menu::Pause,
    }
}

pub fn panel_visible(panel: Panel, menu: Menu, debug: bool, selected: usize) -> bool {
    match panel {
        Panel::Hotbar => menu != Menu::Pause,
        Panel::Shade => menu != Menu::None,
        Panel::PauseBody | Panel::PauseAccent => menu == Menu::Pause,
        Panel::CreativeBody | Panel::CreativeAccent => menu == Menu::Creative,
        Panel::DebugBacking => debug,
        Panel::CrossHorizontal | Panel::CrossVertical => menu == Menu::None,
        Panel::Selection(index) => index == selected && menu != Menu::Pause,
        Panel::Button(button) | Panel::Icon(button, _) => button_visible(button, menu),
    }
}

pub fn label_visible(label: Label, menu: Menu, debug: bool) -> bool {
    match label {
        Label::Title => menu == Menu::None && !debug,
        Label::Help | Label::Selected => menu == Menu::None,
        Label::Notice => true,
        Label::MenuTitle | Label::MenuSubtitle => menu != Menu::None,
        Label::CreativeHint => menu == Menu::Creative,
        Label::Debug(_) => debug,
        Label::SlotNumber(_) | Label::SlotCount(_) => menu != Menu::Pause,
        Label::Button(button) => button_visible(button, menu),
    }
}

pub fn hit(pointer: PointerSample, menu: Menu) -> Option<Button> {
    if !inside(pointer) {
        return None;
    }
    let point = pointer.position().to_vec2();
    let layout = Layout::new(pointer.viewport());
    (0..HOTBAR_SLOTS)
        .map(Button::Slot)
        .chain(MENU_BUTTONS)
        .chain([Button::Close])
        .chain((0..Block::SOLID.len()).map(Button::CreativeBlock))
        .filter(|button| button_visible(*button, menu))
        .find(|button| {
            let [x, y, w, h] = layout.panel(Panel::Button(*button));
            point.x() >= x && point.y() >= y && point.x() < x + w && point.y() < y + h
        })
}

fn inside(pointer: PointerSample) -> bool {
    let point = pointer.position().to_vec2();
    point.x() >= 0.0
        && point.y() >= 0.0
        && point.x() < pointer.viewport().width()
        && point.y() < pointer.viewport().height()
}

pub fn game_area(pointer: PointerSample, menu: Menu) -> bool {
    menu == Menu::None && inside(pointer) && hit(pointer, menu).is_none()
}

pub fn rectangle(panel: Panel, layout: Layout, color: Color) -> LogicResult<ScreenRectangleVisual> {
    let [x, y, w, h] = layout.panel(panel);
    let mut value = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, y),
        LogicalScreenVector::new(w, h),
        color,
    )?;
    value.set_layer(Layer::new(match panel {
        Panel::Shade => 9,
        Panel::PauseBody | Panel::CreativeBody => 21,
        Panel::PauseAccent | Panel::CreativeAccent => 22,
        Panel::DebugBacking => 30,
        Panel::Button(Button::Slot(_)) => 11,
        Panel::Button(_) => 22,
        Panel::Icon(Button::Slot(_), _) => 12,
        Panel::Icon(_, _) => 23,
        Panel::Selection(_) => 13,
        _ => 10,
    }));
    Ok(value)
}

pub fn text(
    session: &mut TextPreparationSession<'_>,
    label: Label,
    content: &str,
    layout: Layout,
    menu: Menu,
) -> LogicResult<ScreenTextVisual> {
    let (position, alignment) = layout.label(label, menu);
    let mut value = ScreenTextVisual::new_with_session(session, content, position)?;
    value.set_alignment(alignment)?;
    value.set_layer(Layer::new(match label {
        Label::Debug(_) => 31,
        Label::MenuTitle | Label::MenuSubtitle | Label::CreativeHint | Label::Button(_) => 24,
        Label::Notice => 25,
        _ => 14,
    }));
    value.set_tint(Color::rgb8(237, 240, 230))?;
    Ok(value)
}

/// Fixed descriptor sets are created once. Hidden visuals are removed at the
/// presentation barrier, so an unopened creative menu has no glyph instances.
pub fn panels(layout: Layout) -> LogicResult<Vec<(Panel, ScreenRectangleVisual)>> {
    let mut kinds = vec![
        Panel::Hotbar,
        Panel::Shade,
        Panel::PauseBody,
        Panel::CreativeBody,
        Panel::PauseAccent,
        Panel::CreativeAccent,
        Panel::DebugBacking,
        Panel::CrossHorizontal,
        Panel::CrossVertical,
    ];
    kinds.extend((0..HOTBAR_SLOTS).map(Panel::Selection));
    let buttons = (0..HOTBAR_SLOTS)
        .map(Button::Slot)
        .chain(MENU_BUTTONS)
        .chain([Button::Close])
        .chain((0..Block::SOLID.len()).map(Button::CreativeBlock));
    for button in buttons {
        kinds.push(Panel::Button(button));
        if matches!(button, Button::Slot(_) | Button::CreativeBlock(_)) {
            kinds.extend((0..3).map(|face| Panel::Icon(button, face)));
        }
    }
    kinds
        .into_iter()
        .map(|kind| Ok((kind, rectangle(kind, layout, Color::TRANSPARENT)?)))
        .collect()
}

pub fn labels(font: &TextFont, layout: Layout) -> LogicResult<Vec<(Label, ScreenTextVisual)>> {
    let mut kinds = vec![
        Label::Title,
        Label::Help,
        Label::Notice,
        Label::Selected,
        Label::MenuTitle,
        Label::MenuSubtitle,
        Label::CreativeHint,
    ];
    kinds.extend((0..HOTBAR_SLOTS).map(Label::SlotNumber));
    kinds.extend((0..HOTBAR_SLOTS).map(Label::SlotCount));
    kinds.extend((0..DEBUG_LINES).map(Label::Debug));
    kinds.extend(
        MENU_BUTTONS
            .into_iter()
            .chain([Button::Close])
            .map(Label::Button),
    );
    kinds.extend((0..Block::SOLID.len()).map(|index| Label::Button(Button::CreativeBlock(index))));
    let mut session = font.shaping_session()?;
    kinds
        .into_iter()
        .map(|kind| Ok((kind, text(&mut session, kind, "", layout, Menu::None)?)))
        .collect()
}

pub fn limits() -> RenderLimits {
    // Engine budgets an AA filled rectangle at twelve tessellated vertices.
    let scene = SceneBudget::new(
        MAX_PANELS,
        0,
        MAX_PANELS * 12,
        256 * 1024,
        256 * 1024,
        256 * 1024,
        MAX_PANELS,
    );
    RenderLimits::new(
        0,
        scene,
        FrameLimits::new(1024, 1024, 65_536, 8 * 1024 * 1024, 128 * 1024 * 1024, 1024),
    )
    .with_max_world_rectangles(0)
    .with_max_world_lines(0)
    .with_max_screen_rectangles(MAX_PANELS)
    .with_screen_scene_budget(scene)
    .with_max_screen_texts(MAX_LABELS)
    .with_max_screen_text_bytes(8192)
    .with_max_screen_text_glyphs(4096)
}

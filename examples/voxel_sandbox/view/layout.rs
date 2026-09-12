//! One coordinate source for drawing and menu-aware half-open hit testing.

use super::{Button, Label, MENU_BUTTONS, Menu, Panel};
use sim_logic::prelude::*;

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

    pub fn scale(self) -> f32 {
        self.scale
    }

    fn pause(self) -> [f32; 4] {
        [
            self.width * 0.5 - 200.0,
            self.height * 0.5 - 224.0,
            400.0,
            448.0,
        ]
    }

    fn creative(self) -> [f32; 4] {
        [
            self.width * 0.5 - 372.0,
            (self.height - 670.0) * 0.5,
            744.0,
            568.0,
        ]
    }

    /// Returns design coordinates. Public hit testing uses `panel`'s scaled box.
    pub fn button(self, button: Button) -> [f32; 4] {
        match button {
            Button::Slot(index) => [
                self.width * 0.5 - 261.0 + index as f32 * 58.0,
                self.height - 76.0,
                52.0,
                52.0,
            ],
            Button::CreativeBlock(index) => {
                let [x, y, _, _] = self.creative();
                [
                    x + 28.0 + (index % 8) as f32 * 86.0,
                    y + 90.0 + (index / 8) as f32 * 104.0,
                    80.0,
                    96.0,
                ]
            }
            Button::Close => {
                let [x, y, w, _] = self.creative();
                [x + w - 89.0, y + 22.0, 64.0, 32.0]
            }
            _ => {
                let [x, y, _, _] = self.pause();
                let row = MENU_BUTTONS
                    .iter()
                    .position(|item| *item == button)
                    .unwrap_or(0);
                [x + 32.0, y + 102.0 + row as f32 * 52.0, 336.0, 42.0]
            }
        }
    }

    pub fn panel(self, panel: Panel) -> [f32; 4] {
        let rect = match panel {
            Panel::Hotbar => [self.width * 0.5 - 269.0, self.height - 84.0, 538.0, 68.0],
            Panel::Shade => [0.0, 0.0, self.width, self.height],
            Panel::PauseBody => self.pause(),
            Panel::CreativeBody => self.creative(),
            Panel::PauseAccent => {
                let [x, y, w, _] = self.pause();
                [x, y, w, 4.0]
            }
            Panel::CreativeAccent => {
                let [x, y, w, _] = self.creative();
                [x, y, w, 4.0]
            }
            Panel::DebugBacking => [12.0, 12.0, 710.0, 258.0],
            Panel::CrossHorizontal => [self.width * 0.5 - 6.0, self.height * 0.5 - 1.0, 12.0, 2.0],
            Panel::CrossVertical => [self.width * 0.5 - 1.0, self.height * 0.5 - 6.0, 2.0, 12.0],
            Panel::Selection(index) => {
                let [x, y, w, h] = self.button(Button::Slot(index));
                [x, y + h - 3.0, w, 3.0]
            }
            Panel::Button(button) => self.button(button),
            Panel::Icon(button, face) => {
                let [x, y, w, _] = self.button(button);
                let size = if matches!(button, Button::CreativeBlock(_)) {
                    36.0
                } else {
                    24.0
                };
                let x = x + (w - size - 5.0) * 0.5;
                let y = y + if matches!(button, Button::CreativeBlock(_)) {
                    20.0
                } else {
                    12.0
                };
                match face {
                    0 => [x, y + 5.0, size, size],
                    1 => [x, y, size, 5.0],
                    _ => [x + size, y, 5.0, size + 5.0],
                }
            }
        };
        rect.map(|value| value * self.scale)
    }

    pub fn label(self, label: Label, menu: Menu) -> (LogicalScreenPosition, TextAlignment) {
        let (x, y, alignment) = match label {
            Label::Title => (20.0, 30.0, TextAlignment::Left),
            Label::Help => (self.width - 20.0, 30.0, TextAlignment::Right),
            Label::Notice => (
                self.width * 0.5,
                self.height - if menu == Menu::None { 126.0 } else { 104.0 },
                TextAlignment::Center,
            ),
            Label::Selected => (self.width * 0.5, self.height - 98.0, TextAlignment::Center),
            Label::MenuTitle | Label::MenuSubtitle => {
                let [x, y, w, _] = if menu == Menu::Creative {
                    self.creative()
                } else {
                    self.pause()
                };
                let subtitle = matches!(label, Label::MenuSubtitle);
                if menu == Menu::Creative {
                    (
                        x + 28.0,
                        y + if subtitle { 67.0 } else { 42.0 },
                        TextAlignment::Left,
                    )
                } else {
                    (
                        x + w * 0.5,
                        y + if subtitle { 70.0 } else { 44.0 },
                        TextAlignment::Center,
                    )
                }
            }
            Label::CreativeHint => {
                let [x, y, w, h] = self.creative();
                (x + w * 0.5, y + h - 23.0, TextAlignment::Center)
            }
            Label::SlotNumber(index) => {
                let [x, y, _, _] = self.button(Button::Slot(index));
                (x + 4.0, y + 13.0, TextAlignment::Left)
            }
            Label::SlotCount(index) => {
                let [x, y, w, h] = self.button(Button::Slot(index));
                (x + w - 4.0, y + h - 7.0, TextAlignment::Right)
            }
            Label::Debug(index) => (24.0, 33.0 + index as f32 * 22.0, TextAlignment::Left),
            Label::Button(button) => {
                let [x, y, w, h] = self.button(button);
                let baseline = if matches!(button, Button::CreativeBlock(_)) {
                    h - 10.0
                } else {
                    h * 0.5 + 6.0
                };
                (x + w * 0.5, y + baseline, TextAlignment::Center)
            }
        };
        (
            LogicalScreenPosition::new(x * self.scale, y * self.scale),
            alignment,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::super::HOTBAR_SLOTS;
    use super::*;

    #[test]
    fn hotbar_and_palette_fit_supported_viewports() {
        for (width, height) in [
            (1100.0, 720.0),
            (960.0, 540.0),
            (1280.0, 720.0),
            (720.0, 1100.0),
        ] {
            let layout = Layout::new(LogicalViewport::new(width, height).unwrap());
            for button in (0..HOTBAR_SLOTS)
                .map(Button::Slot)
                .chain((0..32).map(Button::CreativeBlock))
                .chain(MENU_BUTTONS)
                .chain([Button::Close])
            {
                let [x, y, w, h] = layout.panel(Panel::Button(button));
                assert!(w > 0.0 && h > 0.0 && x >= 0.0 && y >= 0.0);
                assert!(x + w <= width && y + h <= height);
            }
        }
    }

    #[test]
    fn menu_buttons_are_not_interactive_when_hidden() {
        assert!(!super::super::button_visible(Button::Travel, Menu::None));
        assert!(!super::super::button_visible(
            Button::CreativeBlock(0),
            Menu::None
        ));
        assert!(!super::super::button_visible(Button::Slot(0), Menu::Pause));
        assert!(super::super::button_visible(
            Button::CreativeBlock(31),
            Menu::Creative
        ));
        assert!(!super::super::button_visible(
            Button::CreativeBlock(32),
            Menu::Creative
        ));
    }

    #[test]
    fn all_palette_and_hotbar_centers_hit_only_their_own_button() {
        for (width, height) in [(1100.0, 720.0), (960.0, 540.0), (720.0, 1100.0)] {
            let viewport = LogicalViewport::new(width, height).unwrap();
            let layout = Layout::new(viewport);
            for button in (0..32)
                .map(Button::CreativeBlock)
                .chain((0..HOTBAR_SLOTS).map(Button::Slot))
                .chain([Button::Close])
            {
                let [x, y, w, h] = layout.panel(Panel::Button(button));
                let pointer = PointerSample::new(
                    LogicalScreenPosition::new(x + w * 0.5, y + h * 0.5),
                    viewport,
                )
                .unwrap();
                assert_eq!(super::super::hit(pointer, Menu::Creative), Some(button));
                assert!(!super::super::game_area(pointer, Menu::Creative));
                if matches!(button, Button::CreativeBlock(_) | Button::Close) {
                    assert_eq!(super::super::hit(pointer, Menu::None), None);
                }
            }
        }
    }

    #[test]
    fn outside_viewport_and_half_open_button_edges_do_not_capture() {
        let viewport = LogicalViewport::new(1100.0, 720.0).unwrap();
        let layout = Layout::new(viewport);
        for [x, y] in [
            [-1.0, 700.0],
            [1100.0, 700.0],
            [550.0, -1.0],
            [550.0, 720.0],
        ] {
            let pointer = PointerSample::new(LogicalScreenPosition::new(x, y), viewport).unwrap();
            assert_eq!(super::super::hit(pointer, Menu::Creative), None);
            assert!(!super::super::game_area(pointer, Menu::None));
        }
        let [x, y, w, h] = layout.panel(Panel::Button(Button::Slot(0)));
        for [x, y] in [[x + w, y + h * 0.5], [x + w * 0.5, y + h]] {
            let pointer = PointerSample::new(LogicalScreenPosition::new(x, y), viewport).unwrap();
            assert_eq!(super::super::hit(pointer, Menu::None), None);
        }
    }

    #[test]
    fn descriptor_geometry_fits_the_explicit_panel_budget() {
        let layout = Layout::new(LogicalViewport::new(1100.0, 720.0).unwrap());
        let panels = super::super::panels(layout).unwrap();
        assert_eq!(panels.len(), 189);
        assert!(panels.len() <= super::super::MAX_PANELS);
    }
}

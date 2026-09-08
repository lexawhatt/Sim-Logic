//! Reused screen-visual pools for the score, controls and playable keyboard.

use super::{
    app::{Session, View},
    control::Control,
    labels::{Label, LabelAssets},
    layout::{self, Area, Button, GRID, Layout},
    music::{MAX_PITCH, MIN_PITCH},
};
use sim_logic::prelude::*;

pub const MAX_PANELS: usize = 384;
pub const MAX_LABELS: usize = 96;
// Preconverted sRGB palette: Color::rgb8 is not a const constructor.
pub const BACKGROUND: Color = Color::rgb(0.005181517, 0.006512091, 0.010329823);
const TEXT: Color = Color::rgb(0.760_524_5, 0.814_846_6, 0.896_269_4);
const MUTED: Color = Color::rgb(0.187_820_78, 0.250_158_28, 0.366_252_6);
const ACCENT: Color = Color::rgb(0.158_960_83, 0.863_157_2, 0.520_995_56);

#[derive(Component)]
pub struct Panel(pub usize);
#[derive(Component)]
pub struct ImageSlot(pub usize);

#[derive(Resource)]
pub struct Canvas {
    panels: Vec<ScreenRectangleVisual>,
    images: Vec<ScreenImageVisual>,
    blank_panel: ScreenRectangleVisual,
    blank_image: ScreenImageVisual,
    panel_count: usize,
    image_count: usize,
}

impl Canvas {
    pub fn new(blank_panel: ScreenRectangleVisual, blank_image: ScreenImageVisual) -> Self {
        Self {
            panels: vec![blank_panel; MAX_PANELS],
            images: vec![blank_image; MAX_LABELS],
            blank_panel,
            blank_image,
            panel_count: 0,
            image_count: 0,
        }
    }
    fn reset(&mut self) {
        self.panels.fill(self.blank_panel);
        self.images.fill(self.blank_image);
        self.panel_count = 0;
        self.image_count = 0;
    }
    fn rectangle(&mut self, layout: Layout, area: Area, color: Color) -> LogicResult {
        let Some(slot) = self.panels.get_mut(self.panel_count) else {
            return Err("piano panel pool exceeded".into());
        };
        *slot = ScreenRectangleVisual::new(
            layout.position(area.x, area.y),
            layout.size(area.width, area.height),
            color,
        )?;
        // Pool creation order is not managed-identity sort order. Give every
        // painter operation its own explicit order so backgrounds stay behind.
        slot.set_draw_order_depth(self.panel_count as f32)?;
        self.panel_count += 1;
        Ok(())
    }
    fn text(
        &mut self,
        layout: Layout,
        image: ImageAssetId,
        x: f32,
        y: f32,
        scale: f32,
        color: Color,
    ) -> LogicResult {
        let Some(slot) = self.images.get_mut(self.image_count) else {
            return Err("piano label pool exceeded".into());
        };
        let mut visual = ScreenImageVisual::new(
            image,
            layout.position(x, y),
            layout.size(image.width() as f32 * scale, image.height() as f32 * scale),
        )?;
        visual.set_tint(color)?;
        visual.set_draw_order_depth((MAX_PANELS + self.image_count) as f32)?;
        *slot = visual;
        self.image_count += 1;
        Ok(())
    }
    fn number(
        &mut self,
        layout: Layout,
        assets: &LabelAssets,
        value: u16,
        digits: u8,
        x: f32,
        y: f32,
    ) -> LogicResult {
        let mut divisor = if digits == 3 { 100 } else { 10 };
        for index in 0..digits {
            self.text(
                layout,
                assets.digits[usize::from((value / divisor) % 10)],
                x + f32::from(index) * 17.0,
                y,
                2.5,
                TEXT,
            )?;
            divisor = (divisor / 10).max(1);
        }
        Ok(())
    }
}

fn button_label(button: Button, playing: bool) -> Label {
    match button {
        Button::Play => {
            if playing {
                Label::Pause
            } else {
                Label::Play
            }
        }
        Button::Stop => Label::Stop,
        Button::Demo => Label::Demo,
        Button::Clear => Label::Clear,
        Button::TempoDown | Button::LengthDown | Button::VolumeDown => Label::Minus,
        Button::TempoUp | Button::LengthUp | Button::VolumeUp => Label::Plus,
        Button::View2d => Label::View2d,
        Button::View3d => Label::View3d,
    }
}

pub fn draw(
    session: AppRes<Session>,
    control: AppRes<Control>,
    labels: AppRes<LabelAssets>,
    viewport: FrameViewport,
    mut canvas: ResMut<Canvas>,
    mut panels: Query<(&Panel, &mut ScreenRectangleVisual)>,
    mut images: Query<(&ImageSlot, &mut ScreenImageVisual)>,
) -> LogicResult {
    let layout = Layout::new(viewport.logical());
    let snapshot = control.snapshot();
    canvas.reset();
    canvas.rectangle(layout, Area::new(20.0, 16.0, 1400.0, 182.0), BACKGROUND)?;
    canvas.text(layout, labels.id(Label::Title), 40.0, 30.0, 5.0, TEXT)?;
    canvas.text(layout, labels.id(Label::Subtitle), 42.0, 72.0, 1.5, MUTED)?;
    let status = if control.is_faulted() {
        Label::AudioError
    } else if session.notice > 0.0 {
        Label::Busy
    } else if control.is_offline() {
        Label::Silent
    } else {
        Label::Audio
    };
    let status_id = labels.id(status);
    canvas.text(
        layout,
        status_id,
        1380.0 - status_id.width() as f32 * 2.0,
        44.0,
        2.0,
        if status == Label::Audio {
            ACCENT
        } else {
            Color::rgb8(246, 160, 117)
        },
    )?;
    for button in layout::BUTTONS {
        let area = button.area();
        let selected = matches!(button, Button::View2d) && session.view == View::TwoD
            || matches!(button, Button::View3d) && session.view == View::ThreeD
            || button == Button::Play && session.playing;
        canvas.rectangle(
            layout,
            area,
            if selected {
                ACCENT
            } else {
                Color::rgb8(38, 47, 62)
            },
        )?;
        let image = labels.id(button_label(button, session.playing));
        let scale = if matches!(button, Button::VolumeDown | Button::VolumeUp) {
            2.0
        } else {
            2.5
        };
        canvas.text(
            layout,
            image,
            area.x + (area.width - image.width() as f32 * scale) * 0.5,
            area.y + (area.height - 7.0 * scale) * 0.5,
            scale,
            if selected { BACKGROUND } else { TEXT },
        )?;
    }
    canvas.text(layout, labels.id(Label::Tempo), 586.0, 112.0, 2.0, MUTED)?;
    canvas.number(layout, &labels, session.settings.tempo, 3, 724.0, 111.0)?;
    canvas.text(layout, labels.id(Label::Length), 868.0, 112.0, 2.0, MUTED)?;
    canvas.number(layout, &labels, u16::from(session.length), 2, 1016.0, 111.0)?;
    canvas.text(layout, labels.id(Label::Volume), 40.0, 170.0, 2.0, MUTED)?;
    canvas.number(
        layout,
        &labels,
        (session.settings.volume * 100.0).round() as u16,
        3,
        200.0,
        168.0,
    )?;
    canvas.text(
        layout,
        labels.id(Label::PointerHelp),
        378.0,
        170.0,
        1.5,
        MUTED,
    )?;

    if session.view == View::TwoD {
        let row_height = GRID.height / 36.0;
        let cell_width = GRID.width / 32.0;
        for pitch in MIN_PITCH..=MAX_PITCH {
            let y = GRID.y + f32::from(MAX_PITCH - pitch) * row_height;
            canvas.rectangle(
                layout,
                Area::new(GRID.x, y, GRID.width, row_height - 1.0),
                if layout::black_key(pitch) {
                    Color::rgb8(21, 26, 36)
                } else {
                    Color::rgb8(30, 36, 48)
                },
            )?;
            canvas.text(
                layout,
                labels.key_names[usize::from(pitch - MIN_PITCH)],
                66.0,
                y + 2.0,
                1.0,
                if pitch % 12 == 0 { ACCENT } else { MUTED },
            )?;
        }
        for step in 0..=32 {
            canvas.rectangle(
                layout,
                Area::new(GRID.x + step as f32 * cell_width, GRID.y, 1.0, GRID.height),
                if step % 4 == 0 {
                    Color::rgb8(77, 94, 119)
                } else {
                    Color::rgb8(40, 49, 65)
                },
            )?;
            if step < 32 && step % 4 == 0 {
                canvas.text(
                    layout,
                    labels.digits[step / 4 + 1],
                    GRID.x + step as f32 * cell_width + 6.0,
                    210.0,
                    1.5,
                    MUTED,
                )?;
            }
        }
        for (index, entry) in session.settings.sequence.notes().iter().enumerate() {
            let Some(note) = *entry else {
                continue;
            };
            let area = Area::new(
                GRID.x + f32::from(note.start()) * cell_width + 2.0,
                GRID.y + f32::from(MAX_PITCH - note.pitch()) * row_height + 1.0,
                f32::from(note.duration()) * cell_width - 4.0,
                row_height - 2.0,
            );
            canvas.rectangle(
                layout,
                area,
                if session.selected == Some(index) {
                    ACCENT
                } else {
                    Color::rgb8(244, 148, 108)
                },
            )?;
            canvas.rectangle(
                layout,
                Area::new(
                    area.x + area.width - 3.0,
                    area.y + 2.0,
                    1.0,
                    area.height - 4.0,
                ),
                BACKGROUND,
            )?;
        }
        canvas.rectangle(
            layout,
            Area::new(
                GRID.x + snapshot.step.clamp(0.0, 32.0) * cell_width,
                GRID.y - 4.0,
                2.0,
                GRID.height + 8.0,
            ),
            ACCENT,
        )?;
        canvas.rectangle(
            layout,
            Area::new(GRID.x, 650.0, GRID.width, 3.0),
            Color::rgb8(41, 50, 65),
        )?;
        canvas.rectangle(
            layout,
            Area::new(
                GRID.x,
                650.0,
                (GRID.width * snapshot.step / 32.0).max(1.0),
                3.0,
            ),
            ACCENT,
        )?;
    }
    canvas.rectangle(
        layout,
        Area::new(90.0, 690.0, 1310.0, 155.0),
        Color::rgb8(28, 35, 47),
    )?;
    for black in [false, true] {
        for pitch in MIN_PITCH..=MAX_PITCH {
            if layout::black_key(pitch) != black {
                continue;
            }
            let area = layout::key_area(pitch);
            let active = snapshot.active_keys & (1 << (pitch - MIN_PITCH)) != 0;
            canvas.rectangle(
                layout,
                area,
                if active {
                    ACCENT
                } else if black {
                    Color::rgb8(15, 20, 29)
                } else {
                    Color::rgb8(226, 225, 216)
                },
            )?;
            if !black {
                canvas.text(
                    layout,
                    labels.key_names[usize::from(pitch - MIN_PITCH)],
                    area.x + 8.0,
                    area.y + area.height - 17.0,
                    1.0,
                    Color::rgb8(59, 67, 81),
                )?;
            }
        }
    }
    canvas.text(
        layout,
        labels.id(Label::KeyboardHelp),
        90.0,
        866.0,
        1.5,
        MUTED,
    )?;
    for (slot, mut visual) in &mut panels {
        *visual = canvas.panels[slot.0];
    }
    for (slot, mut visual) in &mut images {
        *visual = canvas.images[slot.0];
    }
    Ok(())
}

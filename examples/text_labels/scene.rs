use std::time::Duration;

use sim_engine::SceneBudget;
use sim_logic::prelude::*;

const FONT: &[u8] = include_bytes!("../../tests/assets/text/DejaVuSans.ttf");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Pause,
    Next,
    Exit,
}

#[derive(Component, Clone, Copy)]
enum Label {
    Static,
    Counter,
    Moving,
    Footer,
}

#[derive(Resource, Default)]
struct Clock {
    ticks: u64,
    phase: f32,
}

fn tick(mut clock: ResMut<Clock>) {
    clock.ticks = clock.ticks.saturating_add(1);
}

fn update(
    input: FrameInput<Action>,
    time: FrameTime,
    viewport: FrameViewport,
    mut clock: ResMut<Clock>,
    mut labels: Query<(&Label, &mut ScreenTextVisual)>,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(Action::Exit) {
        commands.request_exit()?;
        return Ok(());
    }
    if input.has_press_occurrence(Action::Pause) {
        commands.set_paused(!time.is_paused())?;
    }
    clock.phase = (clock.phase + time.seconds_f32().min(0.1)).rem_euclid(std::f32::consts::TAU);
    for (kind, mut label) in &mut labels {
        match kind {
            Label::Counter => {
                // Only ten distinct digits are needed as the counter changes.
                // set_text skips shaping for equal strings; this tiny example's
                // formatting allocation is separate from the library cache.
                label.set_text(&format!("{} seconds", clock.ticks / 10))?;
                label.set_position(LogicalScreenPosition::new(
                    (viewport.logical().width() - 76.0).max(300.0),
                    325.0,
                ))?;
            }
            Label::Moving => {
                label.set_position(LogicalScreenPosition::new(
                    -24.0 + 75.0 * clock.phase.sin(),
                    606.0,
                ))?;
                label.set_tint(
                    Color::rgb8(73, 218, 177).with_alpha(0.6 + 0.4 * clock.phase.cos().abs()),
                )?;
            }
            Label::Footer => label.set_position(LogicalScreenPosition::new(
                48.0,
                (viewport.logical().height() - 26.0).max(660.0),
            ))?,
            Label::Static => {}
        }
    }
    Ok(())
}

fn label(
    font: &TextFont,
    text: &str,
    x: f32,
    y: f32,
    tint: Color,
) -> LogicResult<ScreenTextVisual> {
    let mut label = ScreenTextVisual::new(font.clone(), text, LogicalScreenPosition::new(x, y))?;
    label.set_tint(tint)?;
    label.set_draw_order_depth(10.0)?;
    Ok(label)
}

fn panel(
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    color: Color,
    depth: f32,
) -> LogicResult<ScreenRectangleVisual> {
    let mut panel = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, y),
        LogicalScreenVector::new(width, height),
        color,
    )?;
    panel.set_draw_order_depth(depth)?;
    Ok(panel)
}

struct Board {
    camera: ActiveCamera2d,
    labels: Vec<(Label, ScreenTextVisual)>,
    panels: Vec<ScreenRectangleVisual>,
    image: ScreenImageVisual,
    background: WorldBackground,
}

impl Board {
    fn new(
        small: &TextFont,
        body: &TextFont,
        title: &TextFont,
        image: ImageAssetId,
        alternate: bool,
    ) -> LogicResult<Self> {
        let white = Color::rgb8(232, 240, 249);
        let muted = Color::rgb8(148, 166, 188);
        let mint = Color::rgb8(73, 218, 177);
        let mut counter = label(title, "0 seconds", 1204.0, 325.0, mint)?;
        counter.set_alignment(TextAlignment::Right)?;
        let mut image_label = label(title, "Aa", 1106.0, 478.0, Color::rgb8(12, 19, 27))?;
        image_label.set_alignment(TextAlignment::Center)?;
        image_label.set_draw_order_depth(30.0)?;
        let labels = vec![
            (
                Label::Static,
                label(small, "SIM;LOGIC  /  ENGINE 0.3", 48.0, 52.0, mint)?,
            ),
            (
                Label::Static,
                label(
                    title,
                    if alternate {
                        "Новый World. Тот же шрифт."
                    } else {
                        "Нормальный текст. Наконец-то."
                    },
                    48.0,
                    128.0,
                    white,
                )?,
            ),
            (
                Label::Static,
                label(
                    body,
                    "Real glyphs, shared fonts, ordinary Rust components.",
                    48.0,
                    180.0,
                    muted,
                )?,
            ),
            (
                Label::Static,
                label(small, "CHANGING CONTENT", 72.0, 261.0, muted)?,
            ),
            (
                Label::Static,
                label(body, "Hello, world!  Привет, мир!", 72.0, 319.0, white)?,
            ),
            (Label::Counter, counter),
            (
                Label::Static,
                label(
                    small,
                    "MIXED DRAW ORDER  /  RECTANGLE + TEXT + IMAGE",
                    72.0,
                    425.0,
                    muted,
                )?,
            ),
            (
                Label::Static,
                label(
                    body,
                    "This line passes behind the stripe.",
                    72.0,
                    478.0,
                    white,
                )?,
            ),
            (Label::Static, image_label),
            (
                Label::Moving,
                label(
                    body,
                    "Offscreen letters clip; movement and tint reuse the same run.",
                    -24.0,
                    606.0,
                    mint,
                )?,
            ),
            (
                Label::Footer,
                label(
                    small,
                    "Space: pause counter    Enter: next World (while running)    Escape: exit",
                    48.0,
                    694.0,
                    muted,
                )?,
            ),
        ];
        let panels = vec![
            panel(48.0, 225.0, 1184.0, 136.0, Color::rgb8(22, 35, 48), 0.0)?,
            panel(48.0, 392.0, 1184.0, 142.0, Color::rgb8(22, 35, 48), 0.0)?,
            panel(48.0, 225.0, 4.0, 136.0, mint, 0.0)?,
            panel(275.0, 450.0, 90.0, 40.0, Color::rgb8(217, 166, 89), 20.0)?,
        ];
        let mut image = ScreenImageVisual::new(
            image,
            LogicalScreenPosition::new(1056.0, 440.0),
            LogicalScreenVector::new(100.0, 58.0),
        )?;
        image.set_draw_order_depth(20.0)?;
        Ok(Self {
            camera: ActiveCamera2d::centered(1.0)?,
            labels,
            panels,
            image,
            background: WorldBackground::new(Color::rgb8(11, 18, 27))?,
        })
    }

    fn spawn(&self, world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
        world.insert_resource(self.background)?;
        world.insert_resource(Clock::default())?;
        world.spawn(self.camera)?;
        for &panel in &self.panels {
            world.spawn(panel)?;
        }
        world.spawn(self.image)?;
        for (kind, label) in &self.labels {
            world.spawn((*kind, label.clone()))?;
        }
        Ok(())
    }
}

pub fn build_application() -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(100), 8)?);
    config.set_entity_limit(32)?;
    config.set_text_limits(TextLimits::new(3, FONT.len() * 3));
    config.set_image_asset_limits(ImageAssetLimits::new(1, 1, 1, 4));
    let scene = SceneBudget::new(16, 0, 192, 32 * 1024, 64 * 1024, 64 * 1024, 16);
    config.set_render_limits(
        RenderLimits::new(
            0,
            scene,
            FrameLimits::new(32, 64, 12_000, 1024 * 1024, 3 * 4 * 1024 * 1024 + 4, 64),
        )
        .with_max_screen_rectangles(8)
        .with_screen_scene_budget(scene)
        .with_max_screen_images(1)
        .with_max_screen_texts(16)
        .with_max_screen_text_bytes(4096)
        .with_max_screen_text_glyphs(1600),
    );
    let mut app = Application::new(config)?;
    app.approve_component::<Label>()?;
    let small = app.register_font(FONT.to_vec(), TextSettings::new(18.0)?)?;
    let body = app.register_font(FONT.to_vec(), TextSettings::new(26.0)?)?;
    let title = app.register_font(FONT.to_vec(), TextSettings::new(40.0)?)?;
    let image = app.register_image_rgba8(1, 1, &[73, 218, 177, 255])?;
    app.bind_key(PhysicalKeyCode::Space, Action::Pause)?;
    app.bind_key(PhysicalKeyCode::Enter, Action::Next)?;
    app.bind_key(PhysicalKeyCode::Escape, Action::Exit)?;
    app.add_world_replacement_on_press_system();
    app.add_fixed_system(tick);
    app.add_fallible_frame_system(update);
    let alternate = Board::new(&small, &body, &title, image, true)?;
    let target = app.register_world("text-alternate", move |world| alternate.spawn(world))?;
    let board = Board::new(&small, &body, &title, image, false)?;
    let initial = app.register_world("text-labels", move |world| {
        board.spawn(world)?;
        world.insert_resource(WorldReplacementOnPress::new(Action::Next, target))?;
        Ok(())
    })?;
    Ok((app, initial))
}

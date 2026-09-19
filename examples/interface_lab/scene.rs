use sim_engine::{Layer, SceneBudget, Stroke};
use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Tab,
    Shift,
    Accept,
    Point,
    Pause,
    Fullscreen,
    Exit,
}
#[derive(Component)]
struct Spoke(usize);
#[derive(Component)]
struct Dot(usize);
#[derive(Component)]
struct Button(usize);
#[derive(Component)]
struct ButtonLabel(usize);
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Status;
#[derive(Resource)]
struct State {
    frames: u64,
    limit: Option<u64>,
    exercise: bool,
    phase: f32,
    fullscreen: bool,
    clicks: usize,
    focus: KeyboardFocus<usize, ()>,
    pointer: PointerButton<usize>,
    shifts: [bool; 2],
}

pub fn build(
    limit: Option<u64>,
    exercise: bool,
    fullscreen: bool,
) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(1)
            .with_max_screen_texts(8)
            .with_max_screen_text_bytes(4096)
            .with_max_screen_text_glyphs(1024)
            .with_screen_scene_budget(SceneBudget::new(
                256,
                0,
                64_000,
                2 * 1024 * 1024,
                4 * 1024 * 1024,
                8 * 1024 * 1024,
                256,
            ))
            .with_frame_limits(FrameLimits::new(
                24,
                20_000,
                2_100_000,
                160 * 1024 * 1024,
                16 * 1024 * 1024,
                20_000,
            )),
    );
    let mut app = Application::new(config)?;
    app.approve_components::<(Spoke, Dot, Button, ButtonLabel, Panel, Status)>()?;
    app.register_app_resource(WindowControls::default())?;
    for (key, action) in [
        (PhysicalKeyCode::Tab, Action::Tab),
        (PhysicalKeyCode::ShiftLeft, Action::Shift),
        (PhysicalKeyCode::ShiftRight, Action::Shift),
        (PhysicalKeyCode::Enter, Action::Accept),
        (PhysicalKeyCode::KeyP, Action::Pause),
        (PhysicalKeyCode::F11, Action::Fullscreen),
        (PhysicalKeyCode::Escape, Action::Exit),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, Action::Point)?;
    app.add_fallible_frame_system(update);
    let ui = app.register_font(
        include_bytes!("../../tests/assets/text/DejaVuSans.ttf").to_vec(),
        TextSettings::new(18.0)?,
    )?;
    let title_font = app.register_font_style(&ui, TextSettings::new(36.0)?)?;
    let title = ScreenTextVisual::new(
        title_font,
        "Sim;Logic - UI integration",
        LogicalScreenPosition::new(36.0, 65.0),
    )?;
    let hint = ScreenTextVisual::new(
        ui.clone(),
        "Tab / Shift-Tab, Enter, P, F11. Clipped animation continues while paused.",
        LogicalScreenPosition::new(36.0, 105.0),
    )?;
    let status =
        ScreenTextVisual::new(ui.clone(), "Ready", LogicalScreenPosition::new(36.0, 625.0))?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let image = app.register_image_rgba8(
        2,
        2,
        &[
            255, 70, 30, 255, 255, 230, 50, 255, 40, 130, 255, 255, 40, 240, 130, 255,
        ],
    )?;
    let world = app.register_world("interface", move |world| {
        let create = |world: &mut WorldBuilder| -> LogicResult {
            world.spawn(camera)?;
            world.insert_resource(WorldBackground::new(Color::rgb(0.018, 0.025, 0.04))?)?;
            world.insert_resource(State {
                frames: 0,
                limit,
                exercise,
                phase: 0.0,
                fullscreen,
                clicks: 0,
                focus: KeyboardFocus::new(3),
                pointer: PointerButton::new(MouseButton::Left),
                shifts: [false; 2],
            })?;
            world.spawn(title.clone())?;
            world.spawn(hint.clone())?;
            world.spawn((Status, status.clone()))?;
            for index in 0..3 {
                let mut card = ScreenRectangleVisual::rounded(
                    LogicalScreenPosition::new(36.0 + index as f32 * 310.0, 140.0),
                    LogicalScreenVector::new(286.0, 74.0),
                    Color::rgb(0.08, 0.13, 0.21),
                    18.0,
                )?;
                card.set_layer(Layer::new(-3));
                world.spawn((Button(index), card))?;
                world.spawn((
                    ButtonLabel(index),
                    ScreenTextVisual::new(
                        ui.clone(),
                        ["Run", "Settings", "About"][index],
                        LogicalScreenPosition::new(65.0 + index as f32 * 310.0, 186.0),
                    )?,
                ))?;
            }
            let mut panel = ScreenRectangleVisual::rounded(
                LogicalScreenPosition::new(36.0, 240.0),
                LogicalScreenVector::new(928.0, 330.0),
                Color::rgb(0.035, 0.055, 0.085),
                24.0,
            )?;
            panel.set_layer(Layer::new(-3));
            world.spawn((Panel, panel))?;
            for index in 0..48 {
                world.spawn((
                    Spoke(index),
                    ScreenLineVisual::new(
                        LogicalScreenPosition::new(80.0, 280.0),
                        LogicalScreenPosition::new(90.0, 290.0),
                        1.5,
                        Color::rgba(0.08, 0.7, 0.55, 0.6),
                    )?,
                ))?;
            }
            for index in 0..8 {
                world.spawn((
                    Dot(index),
                    ScreenCircleVisual::new(
                        LogicalScreenPosition::new(80.0, 280.0),
                        7.0,
                        Color::rgb(0.15, 0.65, 0.9),
                    )?,
                ))?;
            }
            let mut image = ScreenImageVisual::new(
                image,
                LogicalScreenPosition::new(570.0, 325.0),
                LogicalScreenVector::new(160.0, 100.0),
            )?;
            image.set_layer(Layer::new(2));
            world.spawn(image)?;
            Ok(())
        };
        create(world).map_err(|error| WorldBuildError::user(error.to_string()))
    })?;
    Ok((app, world))
}

#[allow(
    clippy::too_many_arguments,
    reason = "one owner routes frame input and updates presentation without order-dependent systems"
)]
fn update(
    input: FrameInput<Action>,
    time: FrameTime,
    viewport: FrameViewport,
    mut state: ResMut<State>,
    mut window: AppResMut<WindowControls>,
    mut buttons: Query<(&Button, &mut ScreenRectangleVisual)>,
    mut panels: Query<&mut ScreenRectangleVisual, (With<Panel>, Without<Button>)>,
    mut lines: Query<(&Spoke, &mut ScreenLineVisual)>,
    mut circles: Query<(&Dot, &mut ScreenCircleVisual)>,
    mut images: Query<&mut ScreenImageVisual>,
    mut labels: Query<(Option<&Status>, Option<&ButtonLabel>, &mut ScreenTextVisual)>,
    mut commands: Commands,
) -> LogicResult {
    state.frames += 1;
    state.phase = (state.phase + time.seconds_f32().min(0.1) * 0.6) % std::f32::consts::TAU;
    if state.frames == 1 {
        commands.set_paused(true)?;
    }
    let width = viewport.logical().width();
    let height = viewport.logical().height();
    let card_width = ((width - 120.0) / 3.0).max(1.0);
    let panel_height = (height - 370.0).max(40.0);
    for (button, mut visual) in &mut buttons {
        visual.set_geometry(
            LogicalScreenPosition::new(36.0 + button.0 as f32 * (card_width + 24.0), 140.0),
            LogicalScreenVector::new(card_width, 74.0),
        )?;
    }
    for mut panel in &mut panels {
        panel.set_geometry(
            LogicalScreenPosition::new(36.0, 240.0),
            LogicalScreenVector::new((width - 72.0).max(1.0), panel_height),
        )?;
    }
    if input.focus_lost() {
        state.focus.clear();
    }
    let mut paused = time.is_paused();
    for edge in input.edges() {
        if edge.action() == Action::Shift {
            let index =
                usize::from(edge.control() == InputControl::Key(PhysicalKeyCode::ShiftRight));
            state.shifts[index] = edge.state() == ButtonState::Pressed;
        }
        let hit = edge.pointer().and_then(|p| {
            buttons
                .iter()
                .find_map(|(button, visual)| visual.contains_pointer(p).then_some(button.0))
        });
        if let Some(PointerButtonEvent::Clicked { target, .. }) =
            state.pointer.process(edge, hit).event()
        {
            state.focus.set_focused((), &[0, 1, 2], Some(target))?;
            state.clicks += 1;
        }
        if edge.state() != ButtonState::Pressed || edge.is_cancelled() {
            continue;
        }
        match edge.action() {
            Action::Tab => {
                let command = if state.shifts.iter().any(|held| *held) {
                    FocusCommand::Previous
                } else {
                    FocusCommand::Next
                };
                state.focus.process((), &[0, 1, 2], command)?;
            }
            Action::Accept => {
                if state
                    .focus
                    .process((), &[0, 1, 2], FocusCommand::Activate)?
                    .activated
                    .is_some()
                {
                    state.clicks += 1;
                }
            }
            Action::Pause => {
                paused = !paused;
                commands.set_paused(paused)?;
            }
            Action::Fullscreen => {
                state.fullscreen = !state.fullscreen;
                window.request_mode(mode(state.fullscreen));
            }
            Action::Exit => {
                commands.request_exit()?;
            }
            _ => {}
        }
    }
    if state.exercise && (state.frames == 30 || state.frames == 90) {
        state.fullscreen = state.frames == 30;
        window.request_mode(mode(state.fullscreen));
    }
    let hover = input.pointer().and_then(|p| {
        buttons
            .iter()
            .find_map(|(button, visual)| visual.contains_pointer(p).then_some(button.0))
    });
    window.request_cursor(if hover.is_some() {
        CursorShape::Pointer
    } else {
        CursorShape::Default
    });
    for (button, mut visual) in &mut buttons {
        visual.set_stroke(
            (hover == Some(button.0) || state.focus.focused() == Some(button.0))
                .then_some(Stroke::new(2.0, Color::rgb(0.2, 0.8, 0.7))),
        )?;
    }
    let clip = ScreenClip::new(
        LogicalScreenPosition::new(52.0, 255.0),
        LogicalScreenVector::new((width - 104.0).max(1.0), (panel_height - 30.0).max(1.0)),
    )?;
    let center = Vec2::new(width * 0.36, 240.0 + panel_height * 0.5);
    let radius = (panel_height * 0.7).clamp(60.0, 205.0);
    for (spoke, mut line) in &mut lines {
        let angle = state.phase + spoke.0 as f32 * std::f32::consts::TAU / 48.0;
        let from = center + Vec2::new(angle.cos(), angle.sin()) * 35.0;
        let to = center + Vec2::new((angle + 0.5).cos(), (angle + 0.5).sin()) * radius;
        line.set_endpoints(
            LogicalScreenPosition::from_vec2(from),
            LogicalScreenPosition::from_vec2(to),
        )?;
        line.set_clip(clip);
    }
    for (dot, mut circle) in &mut circles {
        let angle = -state.phase + dot.0 as f32 * std::f32::consts::TAU / 8.0;
        circle.set_center(LogicalScreenPosition::from_vec2(
            center + Vec2::new(angle.cos(), angle.sin()) * radius,
        ))?;
        circle.set_clip(clip);
    }
    for mut image in &mut images {
        image.set_position(LogicalScreenPosition::new(
            width * 0.67 - 80.0,
            center.y() - 50.0,
        ))?;
        image.set_rotation(state.phase)?;
        image.set_clip(clip);
    }
    for (status, button, mut label) in &mut labels {
        if let Some(button) = button {
            label.set_position(LogicalScreenPosition::new(
                65.0 + button.0 as f32 * (card_width + 24.0),
                186.0,
            ))?;
        }
        if status.is_none() {
            continue;
        }
        let baseline = 280.0 + panel_height;
        label.set_position(LogicalScreenPosition::new(36.0, baseline))?;
        label.set_text(&format!(
            "Paused: {paused} | Activations: {} | Window: {:?}",
            state.clicks,
            window.mode_status()
        ))?;
        label.set_clip(ScreenClip::new(
            LogicalScreenPosition::new(36.0, baseline - 25.0),
            LogicalScreenVector::new((width - 72.0).max(1.0), 40.0),
        )?);
    }
    if state.limit.is_some_and(|limit| state.frames >= limit) {
        commands.request_exit()?;
    }
    Ok(())
}

fn mode(fullscreen: bool) -> WindowMode {
    if fullscreen {
        WindowMode::BorderlessFullscreen(FullscreenMonitor::Automatic)
    } else {
        WindowMode::Windowed
    }
}

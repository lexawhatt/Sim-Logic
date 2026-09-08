//! Piano editing and input policy; musical rules remain in the music module.

use super::{
    control::{self, Control, Settings},
    labels::LabelAssets,
    layout::{self, Button, Layout},
    model3d as piano,
    music::{MIN_PITCH, Note, STEP_COUNT, Sequence},
    view::{self, Canvas, ImageSlot, Panel},
};
use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    Click,
    Erase,
    PlayPause,
    SwitchView,
    Shorter,
    Longer,
    Slower,
    Faster,
    KeyC,
    KeyD,
    KeyE,
    KeyF,
    Exit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    TwoD,
    ThreeD,
}

#[derive(Debug, Clone, Copy)]
pub struct Session {
    pub settings: Settings,
    pub view: View,
    pub playing: bool,
    pub length: u8,
    pub selected: Option<usize>,
    pub notice: f32,
    drag: Option<(usize, u8, bool)>,
    mouse_pitch: Option<u8>,
    blocked_keys: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            view: View::TwoD,
            playing: false,
            length: 2,
            selected: None,
            notice: 0.0,
            drag: None,
            mouse_pitch: None,
            blocked_keys: 0,
        }
    }
}

fn commit(
    session: &mut Session,
    control: &mut Control,
    proposed: Session,
    action: control::Action,
) {
    if control.submit(proposed.settings, action) {
        *session = proposed;
    } else {
        session.notice = 2.0;
    }
}

fn button(session: &mut Session, control: &mut Control, button: Button, held: u64) {
    let mut proposed = *session;
    match button {
        Button::View2d | Button::View3d => {
            session.view = if button == Button::View2d {
                View::TwoD
            } else {
                View::ThreeD
            };
            session.drag = None;
            return;
        }
        Button::Stop => {
            control.stop();
            session.playing = false;
            session.blocked_keys |= held;
            session.mouse_pitch = None;
            return;
        }
        Button::Play => {
            proposed.playing = !session.playing;
            if !proposed.playing {
                proposed.blocked_keys |= held;
                proposed.mouse_pitch = None;
            }
            let action = if proposed.playing {
                control::Action::Play
            } else {
                control::Action::Pause
            };
            commit(session, control, proposed, action);
            return;
        }
        Button::Demo => {
            proposed.settings.sequence = Sequence::demo();
            proposed.selected = None;
            proposed.drag = None;
        }
        Button::Clear => {
            proposed.settings.sequence = Sequence::new();
            proposed.selected = None;
            proposed.drag = None;
        }
        Button::TempoDown => {
            proposed.settings.tempo = proposed.settings.tempo.saturating_sub(5).max(60)
        }
        Button::TempoUp => proposed.settings.tempo = (proposed.settings.tempo + 5).min(180),
        Button::VolumeDown => {
            proposed.settings.volume =
                ((proposed.settings.volume * 20.0).round() - 1.0).max(0.0) / 20.0
        }
        Button::VolumeUp => {
            proposed.settings.volume =
                ((proposed.settings.volume * 20.0).round() + 1.0).min(20.0) / 20.0
        }
        Button::LengthDown | Button::LengthUp => {
            proposed.length = if button == Button::LengthDown {
                proposed.length.saturating_sub(1).max(1)
            } else {
                (proposed.length + 1).min(STEP_COUNT)
            };
            if let Some(index) = proposed.selected {
                let Some(note) = proposed
                    .settings
                    .sequence
                    .notes()
                    .get(index)
                    .copied()
                    .flatten()
                else {
                    session.selected = None;
                    return;
                };
                proposed.length = proposed.length.min(STEP_COUNT - note.start());
                if proposed
                    .settings
                    .sequence
                    .resize(index, proposed.length)
                    .is_err()
                {
                    session.notice = 2.0;
                    return;
                }
            } else {
                session.length = proposed.length;
                return;
            }
        }
    }
    commit(session, control, proposed, control::Action::Update);
}

fn note_at(sequence: &Sequence, pitch: u8, step: u8) -> Option<usize> {
    sequence.notes().iter().position(|entry| {
        entry.is_some_and(|note| {
            note.pitch() == pitch && step >= note.start() && step < note.start() + note.duration()
        })
    })
}

fn held_keyboard(input: &FrameInput<Action>) -> u64 {
    [
        (Action::KeyC, 0),
        (Action::KeyD, 2),
        (Action::KeyE, 4),
        (Action::KeyF, 5),
    ]
    .into_iter()
    .fold(0, |mask, (key, offset)| {
        mask | (u64::from(input.held(key)) << offset)
    })
}

fn resize_drag(
    session: &mut Session,
    control: &mut Control,
    pointer: PointerSample,
) -> LogicResult {
    let Some((index, pressed_step, moved)) = session.drag else {
        return Ok(());
    };
    let Some((_, step)) =
        layout::grid_cell(Layout::new(pointer.viewport()).unproject(pointer.position()))
    else {
        return Ok(());
    };
    if step == pressed_step && !moved {
        return Ok(());
    }
    if let Some(note) = session.settings.sequence.notes()[index] {
        let duration = step.saturating_sub(note.start()) + 1;
        let mut proposed = *session;
        proposed.drag = Some((index, pressed_step, true));
        proposed.length = duration;
        if duration == note.duration() {
            *session = proposed;
        } else {
            proposed.settings.sequence.resize(index, duration)?;
            commit(session, control, proposed, control::Action::Update);
        }
    }
    Ok(())
}

fn interact(
    input: FrameInput<Action>,
    time: FrameTime,
    mut session: AppResMut<Session>,
    mut control: AppResMut<Control>,
    mut commands: Commands,
) -> LogicResult {
    session.notice = (session.notice - time.seconds_f32()).max(0.0);
    if control.poll_fault() {
        session.playing = false;
    }
    if input.has_press_occurrence(Action::Exit) {
        control.stop();
        commands.request_exit()?;
        return Ok(());
    }
    let held = held_keyboard(&input);
    // A release/repress pair can fit one display frame; final held state alone
    // must not keep a key suppressed by an earlier Pause or Stop.
    for (action, offset) in [
        (Action::KeyC, 0),
        (Action::KeyD, 2),
        (Action::KeyE, 4),
        (Action::KeyF, 5),
    ] {
        if input.has_release_occurrence(action) {
            session.blocked_keys &= !(1 << offset);
        }
    }
    session.blocked_keys &= held;
    for (action, mapped) in [
        (Action::PlayPause, Button::Play),
        (Action::Shorter, Button::LengthDown),
        (Action::Longer, Button::LengthUp),
        (Action::Slower, Button::TempoDown),
        (Action::Faster, Button::TempoUp),
    ] {
        for _ in input.pressed(action) {
            button(&mut session, &mut control, mapped, held);
        }
    }
    if input.pressed(Action::SwitchView).count() % 2 == 1 {
        session.view = if session.view == View::TwoD {
            View::ThreeD
        } else {
            View::TwoD
        };
        session.drag = None;
    }
    for (action, pitch) in [
        (Action::KeyC, 48),
        (Action::KeyD, 50),
        (Action::KeyE, 52),
        (Action::KeyF, 53),
    ] {
        if input.has_press_occurrence(action) && !input.held(action) {
            control.preview(pitch);
        }
    }
    for edge in input.edges().filter(|edge| edge.action() == Action::Click) {
        if edge.state() == ButtonState::Released {
            if let Some(pointer) = edge.pointer() {
                resize_drag(&mut session, &mut control, pointer)?;
            }
            session.drag = None;
            session.mouse_pitch = None;
            continue;
        }
        session.drag = None;
        let Some(pointer) = edge.pointer() else {
            continue;
        };
        let point = Layout::new(pointer.viewport()).unproject(pointer.position());
        if let Some(hit) = layout::BUTTONS
            .into_iter()
            .find(|button| button.area().contains(point))
        {
            button(&mut session, &mut control, hit, held);
            continue;
        }
        if let Some(pitch) = layout::keyboard_pitch(point) {
            session.mouse_pitch = Some(pitch);
            control.preview(pitch);
            continue;
        }
        if session.view != View::TwoD {
            continue;
        }
        let Some((pitch, step)) = layout::grid_cell(point) else {
            continue;
        };
        let mut proposed = *session;
        let index = if let Some(index) = note_at(&session.settings.sequence, pitch, step) {
            index
        } else {
            let note = Note::new(pitch, step, session.length.min(STEP_COUNT - step), 96)?;
            match proposed.settings.sequence.add(note) {
                Ok(index) => index,
                Err(_) => {
                    session.notice = 2.0;
                    continue;
                }
            }
        };
        proposed.selected = Some(index);
        proposed.drag = Some((index, step, false));
        if let Some(note) = proposed.settings.sequence.notes()[index] {
            proposed.length = note.duration();
        }
        if proposed.settings == session.settings {
            *session = proposed;
        } else {
            commit(
                &mut session,
                &mut control,
                proposed,
                control::Action::Update,
            );
        }
        control.preview(pitch);
    }
    // Erase has explicit priority after left-click edits in the same frame.
    for edge in input.pressed(Action::Erase) {
        let Some(pointer) = edge.pointer() else {
            continue;
        };
        if session.view != View::TwoD {
            continue;
        }
        let Some((pitch, step)) =
            layout::grid_cell(Layout::new(pointer.viewport()).unproject(pointer.position()))
        else {
            continue;
        };
        if let Some(index) = note_at(&session.settings.sequence, pitch, step) {
            let mut proposed = *session;
            proposed.settings.sequence.remove(index)?;
            proposed.selected = None;
            proposed.drag = None;
            commit(
                &mut session,
                &mut control,
                proposed,
                control::Action::Update,
            );
        }
    }
    if !input.held(Action::Click) || input.pointer().is_none() {
        session.drag = None;
        session.mouse_pitch = None;
    } else if let Some(pointer) = input.pointer() {
        resize_drag(&mut session, &mut control, pointer)?;
    }
    let mouse = session
        .mouse_pitch
        .map_or(0, |pitch| 1 << (pitch - MIN_PITCH));
    control.set_held_keys((held & !session.blocked_keys) | mouse);
    control.advance_offline(f64::from(time.seconds_f32()));
    Ok(())
}

pub fn build_application(
    control: Control,
    initial_view: View,
) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_entity_limit(1024)?;
    config.set_image_asset_limits(ImageAssetLimits::new(
        super::labels::ASSET_COUNT,
        512,
        16,
        super::labels::PIXEL_BYTES,
    ));
    let scene = sim_engine::SceneBudget::new(
        view::MAX_PANELS,
        0,
        20_000,
        1024 * 1024,
        2 * 1024 * 1024,
        2 * 1024 * 1024,
        view::MAX_PANELS,
    );
    config.set_render_limits(
        RenderLimits::new(
            0,
            scene,
            FrameLimits::new(256, 4096, 50_000, 8 * 1024 * 1024, 128 * 1024 * 1024, 4096),
        )
        .with_max_world_rectangles(0)
        .with_max_world_lines(0)
        .with_max_screen_rectangles(view::MAX_PANELS)
        .with_screen_scene_budget(scene)
        .with_max_screen_images(view::MAX_LABELS),
    );
    config.set_three_d_render_limits(ThreeDRenderLimits::new(64, 64 * 12, 8_388_608));
    let mut app = Application::new(config)?;
    app.approve_components::<(Panel, ImageSlot, piano::Part)>()?;
    let labels = LabelAssets::new(&mut app)?;
    let initial_label = labels.id(super::labels::Label::Title);
    app.register_app_resource(labels)?;
    app.register_app_resource(Session {
        view: initial_view,
        ..Session::default()
    })?;
    app.register_app_resource(control)?;
    for (key, action) in [
        (PhysicalKeyCode::Space, Action::PlayPause),
        (PhysicalKeyCode::Enter, Action::SwitchView),
        (PhysicalKeyCode::Escape, Action::Exit),
        (PhysicalKeyCode::ArrowLeft, Action::Shorter),
        (PhysicalKeyCode::ArrowRight, Action::Longer),
        (PhysicalKeyCode::ArrowDown, Action::Slower),
        (PhysicalKeyCode::ArrowUp, Action::Faster),
        (PhysicalKeyCode::KeyA, Action::KeyC),
        (PhysicalKeyCode::KeyS, Action::KeyD),
        (PhysicalKeyCode::KeyD, Action::KeyE),
        (PhysicalKeyCode::KeyW, Action::KeyF),
    ] {
        app.bind_key(key, action)?;
    }
    app.bind_mouse_button(MouseButton::Left, Action::Click)?;
    app.bind_mouse_button(MouseButton::Right, Action::Erase)?;
    app.add_fallible_frame_system(interact);
    app.add_fallible_frame_system(view::draw);
    app.add_fallible_frame_system(piano::update);
    let camera = ActiveCamera2d::centered(1.0)?;
    let blank = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(1.0, 1.0),
        Color::TRANSPARENT,
    )?;
    let mut blank_image = ScreenImageVisual::new(
        initial_label,
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(1.0, 1.0),
    )?;
    blank_image.set_tint(Color::TRANSPARENT)?;
    let parts = piano::parts()?;
    let world = app.register_world("piano-roll", move |world| {
        world.spawn(camera)?;
        world.insert_resource(
            WorldBackground::new(view::BACKGROUND)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        world.insert_resource(Canvas::new(blank, blank_image))?;
        let mut view = piano::camera().map_err(|error| WorldBuildError::user(error.to_string()))?;
        view.set_enabled(initial_view == View::ThreeD);
        world.insert_resource(view)?;
        for index in 0..view::MAX_PANELS {
            world.spawn((Panel(index), blank))?;
        }
        for index in 0..view::MAX_LABELS {
            world.spawn((ImageSlot(index), blank_image))?;
        }
        for &(part, visual) in &parts {
            world.spawn((part, visual))?;
        }
        Ok(())
    })?;
    Ok((app, world))
}

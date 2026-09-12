//! One owner routes frame occurrences to UI or background application behavior.

use sim_logic::prelude::*;

use super::{scene, view};

/// Maximum background markers in the current World.
pub const MAX_MARKERS: usize = 64;

/// Physical bindings for the example; mouse ownership is independent of aliases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Left mouse, plus Space as an intentionally ignored keyboard alias.
    Point,
    /// P toggles fixed simulation pause.
    Pause,
    /// D changes COUNT eligibility, even during an active gesture.
    ToggleCount,
    /// R removes the current World's placed circles.
    Clear,
    /// Escape requests application exit.
    Exit,
}

/// Application counters survive both World factories and do not own UI capture.
#[derive(Debug, Default)]
pub struct Counters {
    /// Successful COUNT releases.
    pub clicks: u32,
    /// Accepted background presses, distinct from button clicks.
    pub background: u32,
    /// Captured gestures cancelled by input, geometry, or explicit invalidation.
    pub cancellations: u32,
    /// Requested replacements through the NEXT WORLD button.
    pub replacements: u32,
}

/// Persistent route handles allow the two immutable factories to link both ways.
struct Routes([WorldFactoryId; 2]);

fn tick(time: FixedTime, mut board: ResMut<scene::Board>) {
    board.ticks = board.ticks.saturating_add(1);
    board.elapsed = board.elapsed.saturating_add(time.delta());
}

fn toggle_count(board: &mut scene::Board, counters: &mut Counters) {
    board.count_enabled = !board.count_enabled;
    if !board.count_enabled && board.pointer.captured() == Some(board.count_target) {
        board.pointer.cancel();
        counters.cancellations = counters.cancellations.saturating_add(1);
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "one frame owner routes ordered input and refreshes disjoint presentation queries"
)]
fn route_frame(
    input: FrameInput<Action>,
    time: FrameTime,
    camera: Single<&ActiveCamera2d>,
    routes: AppRes<Routes>,
    mut counters: AppResMut<Counters>,
    mut board: ResMut<scene::Board>,
    mut buttons: Query<(LogicEntityRef, &scene::Button, &mut ScreenRectangleVisual)>,
    markers: Query<LogicEntityRef, With<scene::Marker>>,
    mut labels: Query<(&mut view::Label, &mut ScreenTextVisual)>,
    mut indicators: Query<(&view::Indicator, &mut ScreenRectangleVisual), Without<scene::Button>>,
    mut commands: Commands,
) -> LogicResult {
    let mut paused = time.is_paused();
    let mut departing = false;
    let mut marker_count = markers.iter().count();
    let mut cleared = false;

    // Only this FrameUpdate path feeds the controller. In particular, retained
    // FixedInput edges never enter it again after a zero-tick display frame.
    for edge in input.edges() {
        if let Some(captured) = board.pointer.captured() {
            let eligible = buttons.iter().any(|(entity, button, _)| {
                entity.handle() == captured && button.eligible(board.count_enabled)
            });
            if !eligible {
                board.pointer.cancel();
                counters.cancellations = counters.cancellations.saturating_add(1);
            }
        }
        // Layout is fixed in logical screen pixels. The edge supplies its own
        // pointer and viewport; the frame's final pointer is only for hover.
        let target = if departing {
            None
        } else {
            edge.pointer().and_then(|pointer| {
                buttons.iter().find_map(|(entity, button, rectangle)| {
                    (button.eligible(board.count_enabled) && rectangle.contains_pointer(pointer))
                        .then_some(entity.handle())
                })
            })
        };
        let outcome = board.pointer.process(edge, target);
        if departing {
            continue;
        }

        if let Some(event) = outcome.event() {
            match event {
                PointerButtonEvent::Clicked { target, intent, .. } => {
                    let button = buttons.iter().find_map(|(entity, button, _)| {
                        (entity.handle() == target).then_some(*button)
                    });
                    match button {
                        Some(scene::Button::Count) => {
                            counters.clicks = counters.clicks.saturating_add(1);
                        }
                        Some(scene::Button::Pause) => {
                            paused = !paused;
                            commands.set_paused(paused)?;
                        }
                        Some(scene::Button::ToggleCount) => toggle_count(&mut board, &mut counters),
                        Some(scene::Button::NextWorld) => {
                            board.pointer.cancel();
                            commands.replace_world(intent, routes.0[1 - board.world_index])?;
                            counters.replacements = counters.replacements.saturating_add(1);
                            departing = true;
                        }
                        None => {}
                    }
                }
                PointerButtonEvent::Cancelled { .. } => {
                    counters.cancellations = counters.cancellations.saturating_add(1);
                }
                _ => {}
            }
        }

        if edge.state() == ButtonState::Pressed {
            match edge.control() {
                InputControl::Key(_) => match edge.action() {
                    Action::Pause => {
                        paused = !paused;
                        commands.set_paused(paused)?;
                    }
                    Action::ToggleCount => toggle_count(&mut board, &mut counters),
                    Action::Clear if !cleared => {
                        for marker in &markers {
                            commands.despawn(marker.handle())?;
                        }
                        // New markers queued earlier in this batch keep their
                        // identity until the barrier, so clearing affects only
                        // markers that were already visible at frame start.
                        marker_count -= markers.iter().count();
                        cleared = true;
                    }
                    Action::Exit => {
                        commands.request_exit()?;
                        departing = true;
                        board.pointer.cancel();
                    }
                    _ => {}
                },
                InputControl::MouseButton(MouseButton::Left)
                    if !outcome.claimed() && !paused && marker_count < MAX_MARKERS =>
                {
                    if let Some(pointer) = edge.pointer()
                        && scene::background_contains(pointer)
                    {
                        commands.spawn((
                            scene::Marker,
                            Transform2d::new(pointer.world_position(camera.camera())?)?,
                            CircleVisual::new(0.2, scene::accent(board.world_index))?,
                        ))?;
                        marker_count += 1;
                        counters.background = counters.background.saturating_add(1);
                    }
                }
                _ => {}
            }
        }
    }

    view::refresh(
        &board,
        &counters,
        paused,
        input.pointer(),
        &mut buttons,
        &mut labels,
        &mut indicators,
    )
}

/// Builds the identical bounded desktop/headless board using the existing licensed font fixture.
pub fn build_application(time: TimeConfig) -> LogicResult<(Application<Action>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    config.set_entity_limit(view::RECTANGLE_COUNT + view::TEXT_COUNT + MAX_MARKERS + 1)?;
    config.set_input_event_limit(128)?;
    config.set_command_limit(128 + MAX_MARKERS)?;
    config.set_render_limits(view::render_limits()?);
    config.set_text_limits(TextLimits::new(2, view::FONT.len() * 2));
    let mut application = Application::new(config)?;
    application
        .approve_components::<(scene::Button, scene::Marker, view::Label, view::Indicator)>()?;
    application.register_app_resource(Counters::default())?;
    let body = application.register_font(view::FONT.to_vec(), TextSettings::new(18.0)?)?;
    let title = application.register_font(view::FONT.to_vec(), TextSettings::new(30.0)?)?;
    let first = scene::Scene::new(0, &body, &title)?;
    let second = scene::Scene::new(1, &body, &title)?;
    application.bind_mouse_button(MouseButton::Left, Action::Point)?;
    application.bind_key(PhysicalKeyCode::Space, Action::Point)?;
    for (key, action) in [
        (PhysicalKeyCode::KeyP, Action::Pause),
        (PhysicalKeyCode::KeyD, Action::ToggleCount),
        (PhysicalKeyCode::KeyR, Action::Clear),
        (PhysicalKeyCode::Escape, Action::Exit),
    ] {
        application.bind_key(key, action)?;
    }
    application.add_fixed_system(tick);
    application.add_fallible_frame_system(route_frame);
    let initial =
        application.register_world("ui-buttons-garden", move |world| first.spawn(world))?;
    let alternate =
        application.register_world("ui-buttons-harbor", move |world| second.spawn(world))?;
    application.register_app_resource(Routes([initial, alternate]))?;
    Ok((application, initial))
}

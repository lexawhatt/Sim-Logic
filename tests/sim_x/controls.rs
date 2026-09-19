use sim_logic::prelude::*;
use std::time::Duration;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Ui,
}
#[derive(Default)]
struct Seen {
    edges: Vec<ActionEdge<Action>>,
    held: bool,
}

#[test]
fn new_keys_keep_physical_identity_aliases_repeat_filtering_and_cancellation() -> LogicResult {
    let keys = [
        PhysicalKeyCode::Tab,
        PhysicalKeyCode::Digit0,
        PhysicalKeyCode::F11,
        PhysicalKeyCode::ShiftLeft,
        PhysicalKeyCode::ShiftRight,
        PhysicalKeyCode::ControlLeft,
        PhysicalKeyCode::AltRight,
        PhysicalKeyCode::SuperLeft,
    ];
    let mut app = Application::<Action>::new(AppConfig::default())?;
    for key in keys {
        app.bind_key(key, Action::Ui)?;
    }
    app.register_app_resource(Seen::default())?;
    app.register_app_resource(WindowControls::default())?;
    app.add_frame_system(
        |input: FrameInput<Action>,
         mut seen: AppResMut<Seen>,
         mut window: AppResMut<WindowControls>| {
            seen.edges = input.edges().collect();
            seen.held = input.held(Action::Ui);
            if input
                .pressed(Action::Ui)
                .any(|edge| edge.control() == InputControl::Key(PhysicalKeyCode::F11))
            {
                window.request_mode(WindowMode::BorderlessFullscreen(
                    FullscreenMonitor::Automatic,
                ));
                window.request_cursor(CursorShape::Pointer);
            }
        },
    );
    let initial = app.register_world("ui-input", |world| {
        world.spawn(
            ActiveCamera2d::centered(1.0)
                .map_err(|error| WorldBuildError::user(error.to_string()))?,
        )?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let events: Vec<_> = keys
        .into_iter()
        .flat_map(|key| [InputEvent::key(key, ButtonState::Pressed); 2])
        .collect();
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &events, viewport))
    else {
        return Err("frame rejected".into());
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    let seen = runner.app_resource::<Seen>().ok_or("seen")?;
    assert!(seen.held);
    assert_eq!(seen.edges.len(), keys.len());
    assert_eq!(
        seen.edges
            .iter()
            .map(|edge| edge.control())
            .collect::<Vec<_>>(),
        keys.map(InputControl::Key)
    );
    let window = runner.app_resource::<WindowControls>().ok_or("window")?;
    assert_eq!(window.mode_status(), WindowModeStatus::NotSubmitted);
    assert_eq!(
        window.pending_mode(),
        Some(WindowMode::BorderlessFullscreen(
            FullscreenMonitor::Automatic
        ))
    );
    assert_eq!(window.pending_cursor(), Some(CursorShape::Pointer));
    let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
        Duration::ZERO,
        &[InputEvent::FocusLost],
        viewport,
    )) else {
        return Err("frame rejected".into());
    };
    assert!(report.failure().is_none());
    let seen = runner.app_resource::<Seen>().ok_or("seen")?;
    assert!(!seen.held);
    assert_eq!(seen.edges.len(), keys.len());
    assert!(
        seen.edges
            .iter()
            .all(|edge| edge.cancellation_reason() == Some(InputCancellationReason::FocusLost))
    );
    Ok(())
}

#[test]
fn window_controls_are_bounded_last_request_wins() {
    let mut controls = WindowControls::default();
    controls.request_mode(WindowMode::BorderlessFullscreen(FullscreenMonitor::Primary));
    controls.request_mode(WindowMode::Windowed);
    controls.request_cursor(CursorShape::Pointer);
    controls.request_cursor(CursorShape::Text);
    assert_eq!(controls.pending_mode(), Some(WindowMode::Windowed));
    assert_eq!(controls.pending_cursor(), Some(CursorShape::Text));
    assert_eq!(controls.mode_status(), WindowModeStatus::NotSubmitted);
}

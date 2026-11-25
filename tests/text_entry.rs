use sim_logic::{input::*, prelude::*};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Unused,
}
#[derive(Resource, Default)]
struct Seen {
    text: Vec<TextEvent>,
    order: Vec<&'static str>,
    frames: usize,
}
fn collect(input: FrameText, mut seen: ResMut<Seen>) {
    seen.frames += 1;
    seen.text.extend_from_slice(input.events());
    for event in input.ordered() {
        seen.order.push(match event {
            EditorEvent::Text(_) => "text",
            EditorEvent::Physical(_) => "physical",
        });
    }
}
fn fixed(input: FrameText) {
    assert!(input.events().is_empty());
}
fn runner() -> LogicResult<HeadlessRunner<Action>> {
    let mut app = Application::new(AppConfig::default())?;
    app.bind_key(PhysicalKeyCode::KeyN, Action::Unused)?;
    app.add_system(Stage::FrameUpdate, collect);
    app.add_system(Stage::FixedUpdate, fixed);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let world = app.register_world("editor", move |world| {
        world.insert_resource(Seen::default())?;
        world.spawn(camera)?;
        Ok(())
    })?;
    Ok(app.build_headless(world)?)
}
#[test]
fn unicode_order_once_and_atomic_rejection() -> LogicResult {
    let mut runner = runner()?;
    let viewport = LogicalViewport::new(320.0, 240.0)?;
    let text = [
        TextEvent::Commit("Привет".into()),
        TextEvent::Paste(" Latin".into()),
    ];
    let physical = [
        InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Pressed),
        InputEvent::Text { index: 0 },
        InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Released),
        InputEvent::Text { index: 1 },
    ];
    let result = runner.advance_frame(
        FrameRequest::new(Duration::from_millis(20), &physical, viewport).with_text(&text),
    );
    assert!(matches!(result, FrameOutcome::Advanced(ref report) if report.failure().is_none()));
    let seen = runner.resource::<Seen>().unwrap();
    assert_eq!(seen.text, text);
    assert_eq!(seen.order, ["physical", "text", "physical", "text"]);
    let invalid = [TextEvent::Preedit {
        text: "я".into(),
        selection: Some((1, 2)),
    }];
    assert!(matches!(
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport).with_text(&invalid)),
        FrameOutcome::Rejected(BeginFrameRejection::Text(TextInputError::InvalidSelection))
    ));
    assert_eq!(runner.resource::<Seen>().unwrap().frames, 1);
    runner.advance_frame(FrameRequest::new(Duration::from_millis(20), &[], viewport));
    assert_eq!(runner.resource::<Seen>().unwrap().text, text);
    Ok(())
}
#[test]
fn focus_loss_discards_commits_and_limits_are_exact() -> LogicResult {
    let mut runner = runner()?;
    let text = [TextEvent::Commit("discard".into())];
    runner.advance_frame(
        FrameRequest::new(
            Duration::ZERO,
            &[InputEvent::FocusLost],
            LogicalViewport::new(320.0, 240.0)?,
        )
        .with_text(&text),
    );
    assert_eq!(runner.resource::<Seen>().unwrap().text, [TextEvent::Cancel]);
    assert!(validate_text_events(&[TextEvent::Paste("я".repeat(8192))]).is_ok());
    assert_eq!(
        validate_text_events(&[TextEvent::Paste("я".repeat(8193))]),
        Err(TextInputError::LimitExceeded)
    );
    assert!(validate_text_events(&vec![TextEvent::Commit("a".repeat(16384)); 4]).is_ok());
    assert_eq!(
        validate_text_events(&vec![TextEvent::Commit("a".repeat(16384)); 5]),
        Err(TextInputError::LimitExceeded)
    );
    assert!(!format!("{:?}", TextEvent::Commit("private note".into())).contains("private note"));
    Ok(())
}
#[test]
fn modal_changes_cancel_and_quarantine_until_release() {
    let mut router = ModalKeyboard::new(0);
    let key = PhysicalKeyCode::KeyN;
    assert_eq!(
        router
            .route(key, ButtonState::Pressed, false)
            .unwrap()
            .scope,
        0
    );
    let cancelled = router.set_scope(1);
    assert_eq!(cancelled.len(), 1);
    assert!(cancelled[0].cancelled);
    assert!(router.route(key, ButtonState::Pressed, false).is_none());
    assert!(router.route(key, ButtonState::Released, false).is_none());
    assert_eq!(
        router
            .route(key, ButtonState::Pressed, false)
            .unwrap()
            .scope,
        1
    );
    assert!(
        router
            .route(key, ButtonState::Released, true)
            .unwrap()
            .cancelled
    );
    assert!(router.route(key, ButtonState::Released, false).is_none());
}

#[test]
fn invalid_text_markers_reject_before_systems() -> LogicResult {
    let mut runner = runner()?;
    let text = [TextEvent::Commit("x".into())];
    let viewport = LogicalViewport::new(320.0, 240.0)?;
    for events in [
        vec![InputEvent::Text { index: 1 }],
        vec![InputEvent::Text { index: 0 }; 2],
    ] {
        assert!(matches!(
            runner.advance_frame(
                FrameRequest::new(Duration::ZERO, &events, viewport).with_text(&text)
            ),
            FrameOutcome::Rejected(BeginFrameRejection::Text(TextInputError::InvalidOrder))
        ));
    }
    assert_eq!(runner.resource::<Seen>().unwrap().frames, 0);
    Ok(())
}

#[derive(Resource)]
struct Editor {
    router: ModalKeyboard<bool>,
    note: String,
    gameplay: usize,
}
fn route_editor(input: FrameText, mut editor: ResMut<Editor>) {
    for event in input.ordered() {
        match event {
            EditorEvent::Physical(InputEvent::FocusLost) => {
                editor.router.focus_lost();
            }
            EditorEvent::Physical(InputEvent::Key { key, state }) => {
                if let Some(delivery) = editor.router.route(key, state, false)
                    && delivery.state == ButtonState::Pressed
                {
                    if key == PhysicalKeyCode::KeyN && !delivery.scope {
                        editor.router.set_scope(true);
                    } else if key == PhysicalKeyCode::Escape {
                        editor.router.set_scope(false);
                    } else if !delivery.scope {
                        editor.gameplay += 1;
                    }
                }
            }
            EditorEvent::Text(TextEvent::Commit(text) | TextEvent::Paste(text))
                if editor.router.scope() =>
            {
                editor.note.push_str(text)
            }
            _ => {}
        }
    }
}
#[test]
fn same_frame_open_type_close_does_not_dispatch_note_keys_to_gameplay() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_system(Stage::FrameUpdate, route_editor);
    let camera = ActiveCamera2d::centered(1.0)?;
    let world = app.register_world("modal", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Editor {
            router: ModalKeyboard::new(false),
            note: String::new(),
            gameplay: 0,
        })?;
        Ok(())
    })?;
    let mut runner = app.build_headless(world)?;
    let text = [
        TextEvent::Commit("Русский N/J/V".into()),
        TextEvent::Commit("discard".into()),
    ];
    let events = [
        InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Pressed),
        InputEvent::Text { index: 0 },
        InputEvent::key(PhysicalKeyCode::KeyJ, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::Escape, ButtonState::Pressed),
        InputEvent::Text { index: 1 },
        InputEvent::key(PhysicalKeyCode::KeyJ, ButtonState::Released),
    ];
    let result = runner.advance_frame(
        FrameRequest::new(Duration::ZERO, &events, LogicalViewport::new(320.0, 240.0)?)
            .with_text(&text),
    );
    assert!(matches!(result, FrameOutcome::Advanced(ref report) if report.failure().is_none()));
    let editor = runner.resource::<Editor>().unwrap();
    assert_eq!(editor.note, "Русский N/J/V");
    assert_eq!(editor.gameplay, 0);
    Ok(())
}

#[test]
fn modal_focus_loss_allows_fresh_press_without_a_native_release() {
    let mut router = ModalKeyboard::new(1);
    let key = PhysicalKeyCode::KeyN;
    assert!(router.route(key, ButtonState::Pressed, false).is_some());
    assert_eq!(router.focus_lost().len(), 1);
    assert!(router.route(key, ButtonState::Pressed, false).is_some());
}

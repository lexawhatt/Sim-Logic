use super::*;
use sim_engine::{LogicalScreenPosition, LogicalViewport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Activate,
}

fn input() -> InputState<TestAction> {
    let mut bindings = ActionBindings::new();
    bindings
        .bind(PhysicalKeyCode::Space, TestAction::Activate)
        .unwrap();
    bindings
        .bind_mouse_button(MouseButton::Left, TestAction::Activate)
        .unwrap();
    InputState::new(bindings, 16).unwrap()
}

fn pointer() -> PointerSample {
    PointerSample::new(
        LogicalScreenPosition::new(20.0, 30.0),
        LogicalViewport::new(800.0, 600.0).unwrap(),
    )
    .unwrap()
}

fn collect(
    input: &mut InputState<TestAction>,
    events: &[InputEvent],
    paused: bool,
) -> Result<InputCollectionReport, InputCollectionError> {
    let application = ApplicationId::from_raw(12);
    input.collect_frame(
        events,
        paused,
        application,
        WorldGeneration::new(application, 1),
        0,
    )
}

#[test]
fn cancelled_releases_preflight_occurrence_exhaustion_before_any_change() {
    let mut input = input();
    collect(
        &mut input,
        &[
            InputEvent::pointer_moved(pointer()),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        ],
        false,
    )
    .unwrap();
    let frame = input.frame_edges.clone();
    let fixed = input.fixed_edges.clone();
    input.next_occurrence = u64::MAX - 1;
    assert_eq!(
        collect(&mut input, &[InputEvent::FocusLost], false),
        Err(InputCollectionError::OccurrenceIdentityExhausted),
    );
    assert_eq!(input.next_occurrence, u64::MAX - 1);
    assert_eq!(input.pointer, Some(pointer()));
    assert_eq!(input.frame_edges, frame);
    assert_eq!(input.fixed_edges, fixed);
    assert_eq!(input.held_action_counts[&TestAction::Activate], 2);

    input.next_occurrence = u64::MAX - 2;
    collect(&mut input, &[InputEvent::FocusLost], false).unwrap();
    assert_eq!(input.next_occurrence, u64::MAX);
    assert_eq!(input.pointer, None);
    assert!(input.held_action_counts.is_empty());
    assert_eq!(input.frame_edges.len(), 2);
    assert_ne!(input.frame_edges[0].intent(), input.frame_edges[1].intent());
    assert!(
        input
            .frame_edges
            .iter()
            .all(|edge| { edge.cancellation_reason() == Some(InputCancellationReason::FocusLost) })
    );
    let completed = input.frame_edges.clone();
    assert_eq!(
        collect(
            &mut input,
            &[InputEvent::key(
                PhysicalKeyCode::Space,
                ButtonState::Pressed
            )],
            false
        ),
        Err(InputCollectionError::OccurrenceIdentityExhausted),
    );
    assert!(input.held_action_counts.is_empty());
    assert_eq!(input.frame_edges, completed);
}

#[test]
fn short_event_sequences_keep_preflight_and_collection_in_sync() {
    let alphabet = [
        InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
        InputEvent::pointer_moved(pointer()),
        InputEvent::PointerLeft,
        InputEvent::FocusLost,
        InputEvent::mouse_button(MouseButton::Right, ButtonState::Pressed),
    ];
    for paused in [false, true] {
        for encoded in 0..alphabet.len().pow(4) {
            let mut value = encoded;
            let events = std::array::from_fn::<_, 4, _>(|_| {
                let event = alphabet[value % alphabet.len()];
                value /= alphabet.len();
                event
            });
            let mut input = input();
            let expected = input.preflight_frame(&events, paused).unwrap();
            let actual = collect(&mut input, &events, paused).unwrap();
            assert_eq!(actual, expected, "{events:?}");
            assert_eq!(actual.logical_edges, input.frame_edges.len(), "{events:?}");
            assert_eq!(input.next_occurrence as usize, actual.logical_edges);
            assert_eq!(
                input.fixed_edges.len(),
                if paused { 0 } else { actual.logical_edges },
            );
            let held = input
                .held_keys
                .iter()
                .chain(&input.held_mouse)
                .filter(|held| **held)
                .count();
            assert_eq!(
                held,
                input.held_action_counts.values().sum::<usize>(),
                "{events:?}"
            );
            for edge in &input.frame_edges {
                if edge.is_cancelled() {
                    assert_eq!(edge.state(), ButtonState::Released);
                    assert_eq!(edge.pointer(), None);
                }
                if matches!(edge.control(), InputControl::Key(_)) {
                    assert_eq!(edge.pointer(), None);
                }
            }
        }
    }
}

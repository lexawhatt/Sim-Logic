use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;

use sim_engine::{Camera2d, Camera2dError, LogicalScreenPosition, LogicalViewport, Projection2d};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Primary,
    Secondary,
    Middle,
}

fn sample(x: f32, y: f32) -> PointerSample {
    PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(800.0, 600.0).unwrap(),
    )
    .unwrap()
}

fn state(limit: usize) -> InputState<TestAction> {
    let mut bindings = ActionBindings::new();
    bindings
        .bind_mouse_button(MouseButton::Left, TestAction::Primary)
        .unwrap();
    bindings
        .bind_mouse_button(MouseButton::Right, TestAction::Secondary)
        .unwrap();
    bindings
        .bind_mouse_button(MouseButton::Middle, TestAction::Middle)
        .unwrap();
    bindings
        .bind(PhysicalKeyCode::Space, TestAction::Primary)
        .unwrap();
    InputState::new(bindings, limit).unwrap()
}

fn collect(
    input: &mut InputState<TestAction>,
    events: &[InputEvent],
    paused: bool,
    frame: u64,
) -> Result<InputCollectionReport, InputCollectionError> {
    let application = ApplicationId::from_raw(31);
    input.collect_frame(
        events,
        paused,
        application,
        WorldGeneration::new(application, 2),
        frame,
    )
}

fn mouse(button: MouseButton, state: ButtonState) -> InputEvent {
    InputEvent::mouse_button(button, state)
}

fn hold_all(input: &mut InputState<TestAction>, paused: bool) {
    for (index, button) in ALL_MOUSE_BUTTONS.into_iter().enumerate() {
        collect(
            input,
            &[mouse(button, ButtonState::Pressed)],
            paused,
            index as u64,
        )
        .unwrap();
    }
}

#[derive(Debug, PartialEq, Eq)]
struct StateSnapshot {
    held_keys: [bool; SUPPORTED_PHYSICAL_KEY_COUNT],
    held_mouse: [bool; ALL_MOUSE_BUTTONS.len()],
    held_actions: HashMap<TestAction, usize>,
    pointer: Option<PointerSample>,
    frame_edges: Vec<ActionEdge<TestAction>>,
    fixed_edges: Vec<ActionEdge<TestAction>>,
    next_occurrence: u64,
}

fn snapshot(input: &InputState<TestAction>) -> StateSnapshot {
    StateSnapshot {
        held_keys: input.held_keys,
        held_mouse: input.held_mouse,
        held_actions: input.held_action_counts.clone(),
        pointer: input.pointer,
        frame_edges: input.frame_edges.clone(),
        fixed_edges: input.fixed_edges.clone(),
        next_occurrence: input.next_occurrence,
    }
}

#[test]
fn sample_rejects_nonfinite_but_preserves_outside_coordinates() {
    let viewport = LogicalViewport::new(800.0, 600.0).unwrap();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            PointerSample::new(LogicalScreenPosition::new(bad, 0.0), viewport),
            Err(PointerSampleError)
        );
        assert_eq!(
            PointerSample::new(LogicalScreenPosition::new(0.0, bad), viewport),
            Err(PointerSampleError)
        );
    }
    let outside = sample(-30.0, 700.0);
    assert_eq!(outside.position(), LogicalScreenPosition::new(-30.0, 700.0));
    assert_eq!(outside.viewport(), viewport);
}

#[test]
fn signed_zero_samples_and_events_have_lawful_equality_and_hashing() {
    fn hash(value: impl Hash) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }
    let positive = sample(0.0, 0.0);
    let negative = sample(-0.0, -0.0);
    assert_eq!(positive, negative);
    assert_eq!(hash(positive), hash(negative));
    assert_eq!(
        hash(InputEvent::pointer_moved(positive)),
        hash(InputEvent::pointer_moved(negative))
    );
    let mut input = state(8);
    collect(
        &mut input,
        &[
            InputEvent::pointer_moved(positive),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
        false,
        0,
    )
    .unwrap();
    let edge = input.frame_edges[0];
    let negative_edge = ActionEdge {
        pointer: Some(negative),
        ..edge
    };
    assert_eq!(edge, negative_edge);
    assert_eq!(hash(edge), hash(negative_edge));
}

#[test]
fn explicit_camera_conversion_preserves_engine_results_and_errors() {
    let pointer = sample(600.0, 150.0);
    let mut camera = Camera2d::new(Vec2::new(5.0, -3.0), 50.0).unwrap();
    assert_eq!(pointer.world_position(camera).unwrap(), Vec2::new(9.0, 0.0));
    camera.set_rotation(0.75).unwrap();
    assert_eq!(
        pointer.world_position(camera),
        camera.screen_to_world(pointer.position(), pointer.viewport())
    );
    camera.set_projection(Projection2d::new(std::f32::consts::FRAC_PI_2, 1.0).unwrap());
    assert!(matches!(
        pointer.world_position(camera),
        Err(Camera2dError::SingularProjection { .. })
    ));
}

#[test]
fn duplicate_mouse_binding_preserves_original_and_catalog_indices() {
    let mut input = state(8);
    let error = input
        .bindings
        .bind_mouse_button(MouseButton::Left, TestAction::Middle)
        .unwrap_err();
    assert_eq!(error.button(), MouseButton::Left);
    assert_eq!(
        input.bindings.mouse_action_for(MouseButton::Left),
        Some(TestAction::Primary)
    );
    for (index, button) in ALL_MOUSE_BUTTONS.into_iter().enumerate() {
        assert_eq!(mouse_button_index(button), index);
    }
}

#[test]
fn mouse_edges_capture_event_time_sample_and_share_delivery_identity() {
    let mut input = state(8);
    let first = sample(100.0, 150.0);
    let last = sample(500.0, 550.0);
    let report = collect(
        &mut input,
        &[
            InputEvent::pointer_moved(first),
            mouse(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(last),
            mouse(MouseButton::Left, ButtonState::Released),
        ],
        false,
        0,
    )
    .unwrap();
    assert_eq!(report.physical_events, 4);
    assert_eq!(report.logical_edges, 2);
    assert_eq!(input.frame_edges[0].pointer(), Some(first));
    assert_eq!(input.frame_edges[1].pointer(), Some(last));
    assert_eq!(input.pointer, Some(last));
    assert_eq!(input.frame_edges, input.fixed_edges);
    assert_ne!(input.frame_edges[0].intent(), input.frame_edges[1].intent());
    assert!(!input.frame_input().held(TestAction::Primary));
}

#[test]
fn motion_works_without_bindings_and_does_not_count_as_unmapped() {
    let mut input = InputState::new(ActionBindings::<TestAction>::new(), 4).unwrap();
    let report = collect(
        &mut input,
        &[
            InputEvent::pointer_moved(sample(30.0, 40.0)),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
        false,
        0,
    )
    .unwrap();
    assert_eq!(report.logical_edges, 0);
    assert_eq!(report.unmapped_events, 1);
    assert_eq!(input.pointer, Some(sample(30.0, 40.0)));
}

#[test]
fn keyboard_edges_and_mouse_edges_without_motion_have_no_sample() {
    let mut input = state(8);
    collect(
        &mut input,
        &[
            mouse(MouseButton::Left, ButtonState::Pressed),
            InputEvent::pointer_moved(sample(40.0, 50.0)),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
        false,
        0,
    )
    .unwrap();
    assert_eq!(input.frame_edges.len(), 2);
    assert!(
        input
            .frame_edges
            .iter()
            .all(|edge| edge.pointer().is_none())
    );
    collect(&mut input, &[InputEvent::PointerLeft], false, 1).unwrap();
    assert!(input.frame_input().held(TestAction::Primary));
    assert_eq!(input.held_action_counts[&TestAction::Primary], 1);
}

#[test]
fn leave_synthesizes_ordered_releases_without_coordinates_or_repeat_edges() {
    let mut input = state(8);
    hold_all(&mut input, false);
    collect(
        &mut input,
        &[InputEvent::pointer_moved(sample(10.0, 20.0))],
        false,
        3,
    )
    .unwrap();
    let report = collect(
        &mut input,
        &[InputEvent::PointerLeft, InputEvent::PointerLeft],
        false,
        4,
    )
    .unwrap();
    assert_eq!(report.physical_events, 2);
    assert_eq!(report.logical_edges, 3);
    assert_eq!(report.suppressed_repeats, 0);
    assert_eq!(input.pointer, None);
    assert_eq!(
        input
            .frame_edges
            .iter()
            .map(|edge| edge.action())
            .collect::<Vec<_>>(),
        [
            TestAction::Primary,
            TestAction::Secondary,
            TestAction::Middle
        ]
    );
    assert!(
        input
            .frame_edges
            .iter()
            .all(|edge| edge.state() == ButtonState::Released && edge.pointer().is_none())
    );
    assert!(input.held_action_counts.is_empty());
}

#[test]
fn generated_frame_edge_overflow_is_atomic_even_while_paused() {
    for paused in [false, true] {
        let mut input = state(2);
        hold_all(&mut input, true);
        collect(
            &mut input,
            &[InputEvent::pointer_moved(sample(10.0, 20.0))],
            true,
            3,
        )
        .unwrap();
        let before = snapshot(&input);
        let error = collect(&mut input, &[InputEvent::PointerLeft], paused, 4);
        assert_eq!(
            error,
            Err(InputCollectionError::FrameEdgeLimitExceeded {
                limit: 2,
                incoming: 3
            })
        );
        assert_eq!(snapshot(&input), before);
    }
}

#[test]
fn synthesized_releases_obey_retained_fixed_edge_bound_atomically() {
    let mut input = state(3);
    hold_all(&mut input, false);
    let before = snapshot(&input);
    assert_eq!(
        collect(&mut input, &[InputEvent::PointerLeft], false, 3),
        Err(InputCollectionError::RetainedFixedEdgeLimitExceeded {
            limit: 3,
            retained: 3,
            incoming: 3
        })
    );
    assert_eq!(snapshot(&input), before);
    input.consume_fixed_delivery();
    collect(&mut input, &[InputEvent::PointerLeft], false, 4).unwrap();
    assert_eq!(input.fixed_edges.len(), 3);
}

#[test]
fn frame_edge_preflight_counts_new_press_after_synthetic_releases() {
    let mut input = state(2);
    collect(
        &mut input,
        &[
            mouse(MouseButton::Left, ButtonState::Pressed),
            mouse(MouseButton::Right, ButtonState::Pressed),
        ],
        true,
        0,
    )
    .unwrap();
    let before = snapshot(&input);
    assert_eq!(
        collect(
            &mut input,
            &[
                InputEvent::PointerLeft,
                mouse(MouseButton::Left, ButtonState::Pressed)
            ],
            true,
            1
        ),
        Err(InputCollectionError::FrameEdgeLimitExceeded {
            limit: 2,
            incoming: 3
        })
    );
    assert_eq!(snapshot(&input), before);
}

#[test]
fn occurrence_exhaustion_preserves_mouse_pointer_and_queues() {
    let mut input = state(8);
    hold_all(&mut input, false);
    input.next_occurrence = u64::MAX - 2;
    let before = snapshot(&input);
    assert_eq!(
        collect(
            &mut input,
            &[
                InputEvent::pointer_moved(sample(70.0, 80.0)),
                InputEvent::PointerLeft
            ],
            false,
            3
        ),
        Err(InputCollectionError::OccurrenceIdentityExhausted)
    );
    assert_eq!(snapshot(&input), before);
}

#[test]
fn edge_cleanup_preserves_pointer_but_inactive_snapshot_cleanup_clears_it() {
    let mut input = state(8);
    let pointer = sample(20.0, 30.0);
    collect(
        &mut input,
        &[
            InputEvent::pointer_moved(pointer),
            mouse(MouseButton::Left, ButtonState::Pressed),
        ],
        false,
        0,
    )
    .unwrap();
    let token = input.frame_edges[0].intent();
    assert!(input.consume_token_occurrence(token));
    assert!(!input.token_is_live(token));
    input.end_frame();
    input.consume_fixed_delivery();
    input.discard_fixed_edges();
    input.clear_world_edges();
    assert_eq!(input.pointer, Some(pointer));
    assert!(input.frame_input().held(TestAction::Primary));
    let mut frame = input.frame_input();
    let mut fixed = input.fixed_input();
    assert_eq!(frame.pointer, Some(pointer));
    assert_eq!(fixed.pointer, Some(pointer));
    frame.clear_reusing_storage();
    fixed.clear_reusing_storage();
    assert_eq!(frame.pointer, None);
    assert_eq!(fixed.pointer, None);
}

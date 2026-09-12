use super::*;

fn collect(input: &mut InputState<u8>, events: &[InputEvent]) -> Result<(), InputCollectionError> {
    let application = ApplicationId::from_raw(52);
    input.collect_frame(
        events,
        false,
        application,
        WorldGeneration::new(application, 1),
        0,
    )?;
    Ok(())
}

#[test]
fn wheel_snapshot_reuses_storage_and_end_and_world_boundaries_clear_it() {
    let mut input = InputState::new(ActionBindings::<u8>::new(), 8).unwrap();
    let wheel = InputEvent::mouse_wheel(ScrollDelta::lines(0.5, 1.0).unwrap());
    collect(&mut input, &[wheel; 8]).unwrap();
    let mut frame = FrameInputState::empty();
    input.copy_frame_snapshot_into(&mut frame);
    assert_eq!(frame.scrolls.len(), 8);
    let state_pointer = input.frame_scrolls.as_ptr();
    let state_capacity = input.frame_scrolls.capacity();
    let frame_pointer = frame.scrolls.as_ptr();
    let frame_capacity = frame.scrolls.capacity();
    for index in 0..64 {
        frame.clear_reusing_storage();
        assert!(frame.scrolls.is_empty());
        if index % 2 == 0 {
            input.end_frame();
        } else {
            input.clear_world_edges();
        }
        assert!(input.frame_scrolls.is_empty());
        collect(&mut input, &[wheel; 8]).unwrap();
        input.copy_frame_snapshot_into(&mut frame);
        assert_eq!(frame.scrolls.len(), 8);
        assert_eq!(input.frame_scrolls.as_ptr(), state_pointer);
        assert_eq!(input.frame_scrolls.capacity(), state_capacity);
        assert_eq!(frame.scrolls.as_ptr(), frame_pointer);
        assert_eq!(frame.scrolls.capacity(), frame_capacity);
        assert_eq!(
            input.next_occurrence, 0,
            "wheels do not consume transition identities"
        );
        assert!(input.fixed_edges.is_empty());
    }
}

#[test]
fn rejected_fixed_backlog_preserves_wheel_state_and_retires_no_tokens() {
    let mut bindings = ActionBindings::new();
    bindings.bind_mouse_button(MouseButton::Left, 0_u8).unwrap();
    let mut input = InputState::new(bindings, 2).unwrap();
    let press = InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed);
    let release = InputEvent::mouse_button(MouseButton::Left, ButtonState::Released);
    let wheel = InputEvent::mouse_wheel(ScrollDelta::pixels(0.0, 2.0).unwrap());
    collect(&mut input, &[press, release]).unwrap();
    collect(&mut input, &[wheel]).unwrap();
    let before = input.frame_scrolls.clone();
    let tokens: Vec<_> = input.fixed_edges.iter().map(|edge| edge.intent()).collect();
    assert!(matches!(
        collect(&mut input, &[press, wheel]),
        Err(InputCollectionError::RetainedFixedEdgeLimitExceeded {
            retained: 2,
            incoming: 1,
            ..
        })
    ));
    assert_eq!(input.frame_scrolls, before);
    assert!(tokens.iter().all(|token| input.token_is_live(*token)));
    assert_eq!(input.next_occurrence, 2);
    assert!(input.held_action_counts.is_empty());
    input.discard_fixed_edges();
    collect(&mut input, &[press]).unwrap();
    assert!(input.frame_scrolls.is_empty());
}

#[test]
fn consuming_an_action_occurrence_preserves_ordered_wheel_merge() {
    let mut bindings = ActionBindings::new();
    bindings.bind_mouse_button(MouseButton::Left, 0_u8).unwrap();
    let mut input = InputState::new(bindings, 4).unwrap();
    let wheel = InputEvent::mouse_wheel(ScrollDelta::lines(0.0, 1.0).unwrap());
    collect(
        &mut input,
        &[
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
            wheel,
            InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
            wheel,
        ],
    )
    .unwrap();
    let first = input.frame_edges[0].intent();
    assert!(input.consume_token_occurrence(first));
    let events: Vec<_> = scroll::ordered_events(&input.frame_edges, &input.frame_scrolls).collect();
    assert!(matches!(
        events.as_slice(),
        [
            FrameInputEvent::Scroll(_),
            FrameInputEvent::Action(_),
            FrameInputEvent::Scroll(_)
        ]
    ));
    assert!(!input.token_is_live(first));
}

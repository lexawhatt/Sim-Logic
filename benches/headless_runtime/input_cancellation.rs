//! Warmed cancellation metadata through FrameUpdate and delayed FixedUpdate.
//! Uses two-frame cycles: zero ticks, then one tick. No renderer or font work.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum CancellationAction {
    Shared,
}

type ExpectedEdge = (
    InputControl,
    ButtonState,
    Option<InputCancellationReason>,
    bool,
);
const KEY: InputControl = InputControl::Key(PhysicalKeyCode::Space);
const MOUSE: InputControl = InputControl::MouseButton(MouseButton::Left);
const EXPECTED: [ExpectedEdge; 6] = [
    (KEY, ButtonState::Pressed, None, false),
    (MOUSE, ButtonState::Pressed, None, true),
    (
        MOUSE,
        ButtonState::Released,
        Some(InputCancellationReason::PointerLeft),
        false,
    ),
    (MOUSE, ButtonState::Pressed, None, true),
    (
        KEY,
        ButtonState::Released,
        Some(InputCancellationReason::FocusLost),
        false,
    ),
    (
        MOUSE,
        ButtonState::Released,
        Some(InputCancellationReason::FocusLost),
        false,
    ),
];

#[derive(Resource, Clone, Copy)]
struct Observations {
    sample: PointerSample,
    frames: usize,
    fixed_ticks: usize,
    frame_edges: usize,
    fixed_edges: usize,
    frame_tokens: [Option<TransitionIntentToken>; 3],
    fixed_tail: [Option<TransitionIntentToken>; 3],
}

fn validate<const N: usize>(
    edges: impl Iterator<Item = ActionEdge<CancellationAction>>,
    expected: &[ExpectedEdge],
    sample: PointerSample,
) -> [Option<TransitionIntentToken>; N] {
    assert_eq!(expected.len(), N);
    let mut tokens = [None; N];
    let mut count = 0;
    for (index, edge) in edges.enumerate() {
        let (control, state, reason, has_pointer) = expected[index];
        assert_eq!(edge.action(), CancellationAction::Shared);
        assert_eq!(
            (edge.control(), edge.state(), edge.cancellation_reason()),
            (control, state, reason)
        );
        assert_eq!(edge.is_cancelled(), reason.is_some());
        assert_eq!(edge.pointer(), has_pointer.then_some(sample));
        tokens[index] = Some(edge.intent());
        count += 1;
    }
    assert_eq!(
        count, N,
        "input cancellation observer received the wrong edge count"
    );
    tokens
}

fn observe_frame(input: FrameInput<CancellationAction>, mut observed: ResMut<Observations>) {
    let first = observed.frames.is_multiple_of(2);
    let offset = if first { 0 } else { 3 };
    let tokens = validate::<3>(
        input.edges(),
        &EXPECTED[offset..offset + 3],
        observed.sample,
    );
    assert_eq!(input.held(CancellationAction::Shared), first);
    assert!(input.pointer().is_none());
    assert_eq!(
        input.pressed(CancellationAction::Shared).count(),
        if first { 2 } else { 1 }
    );
    assert_eq!(
        input.released(CancellationAction::Shared).count(),
        if first { 1 } else { 2 }
    );
    if !first {
        // The second frame's occurrences were already inspected by FixedUpdate.
        assert_eq!(tokens, observed.fixed_tail);
    }
    observed.frame_tokens = tokens;
    observed.frames += 1;
    observed.frame_edges += tokens.len();
}

fn observe_fixed(input: FixedInput<CancellationAction>, mut observed: ResMut<Observations>) {
    assert!(!observed.frames.is_multiple_of(2));
    assert!(!input.held(CancellationAction::Shared));
    assert!(input.pointer().is_none());
    let presses = validate::<3>(
        input.pressed(CancellationAction::Shared),
        &[EXPECTED[0], EXPECTED[1], EXPECTED[3]],
        observed.sample,
    );
    let releases = validate::<3>(
        input.released(CancellationAction::Shared),
        &[EXPECTED[2], EXPECTED[4], EXPECTED[5]],
        observed.sample,
    );
    // A zero-tick frame retains its exact origin, pointer, cause, and token.
    assert_eq!([presses[0], presses[1], releases[0]], observed.frame_tokens);
    observed.fixed_tail = [presses[2], releases[1], releases[2]];
    observed.fixed_ticks += 1;
    observed.fixed_edges += presses.len() + releases.len();
}

fn observations(
    runner: &HeadlessRunner<CancellationAction>,
) -> Result<Observations, Box<dyn Error>> {
    runner
        .resource::<Observations>()
        .copied()
        .ok_or_else(|| io::Error::other("input cancellation counters disappeared").into())
}

pub(super) fn run_allocation_case() -> Result<(), Box<dyn Error>> {
    assert!(WARM_UP_FRAMES.is_multiple_of(2));
    assert!(ALLOCATION_CHECK_FRAMES.is_multiple_of(2));
    let viewport = LogicalViewport::new(1280.0, 720.0)?;
    let sample = PointerSample::new(LogicalScreenPosition::new(320.0, 180.0), viewport)?;
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
    config.set_input_event_limit(16)?;
    let mut app = Application::<CancellationAction>::new(config)?;
    app.bind_key(PhysicalKeyCode::Space, CancellationAction::Shared)?;
    app.bind_mouse_button(MouseButton::Left, CancellationAction::Shared)?;
    app.add_fixed_system(observe_fixed);
    app.add_frame_system(observe_frame);
    let camera = ActiveCamera2d::centered(40.0)?;
    let initial = app.register_world("input-cancellation-allocation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Observations {
            sample,
            frames: 0,
            fixed_ticks: 0,
            frame_edges: 0,
            fixed_edges: 0,
            frame_tokens: [None; 3],
            fixed_tail: [None; 3],
        })?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let first = [
        InputEvent::pointer_moved(sample),
        InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::PointerLeft,
    ];
    let second = [
        InputEvent::pointer_moved(sample),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::FocusLost,
        InputEvent::FocusLost,
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Released),
    ];
    let advance = |runner: &mut HeadlessRunner<CancellationAction>,
                   index: usize|
     -> Result<(), Box<dyn Error>> {
        let ticks = u32::from(!index.is_multiple_of(2));
        let events = if ticks == 0 { &first[..] } else { &second[..] };
        let report =
            match runner.advance_frame(FrameRequest::new(FIXED_STEP * ticks, events, viewport)) {
                FrameOutcome::Advanced(report) => report,
                FrameOutcome::Rejected(error) => return Err(error.into()),
            };
        if report.failure().is_some()
            || report.fixed_ticks_attempted() != ticks
            || report.exit_requested()
            || !matches!(report.transition(), FrameTransition::None)
            || report.extracted_generation() != Some(runner.world_generation())
        {
            return Err(
                io::Error::other("cancellation benchmark violated its frame contract").into(),
            );
        }
        black_box(report);
        Ok(())
    };
    for index in 0..WARM_UP_FRAMES {
        advance(&mut runner, index)?;
    }
    let before = observations(&runner)?;
    begin_allocation_count();
    let measured = (|| -> Result<(), Box<dyn Error>> {
        for index in 0..ALLOCATION_CHECK_FRAMES {
            advance(&mut runner, index)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured?;
    let after = observations(&runner)?;
    assert_eq!(after.frames - before.frames, ALLOCATION_CHECK_FRAMES);
    assert_eq!(
        after.fixed_ticks - before.fixed_ticks,
        ALLOCATION_CHECK_FRAMES / 2
    );
    assert_eq!(
        after.frame_edges - before.frame_edges,
        ALLOCATION_CHECK_FRAMES * 3
    );
    assert_eq!(
        after.fixed_edges - before.fixed_edges,
        ALLOCATION_CHECK_FRAMES * 3
    );
    println!(
        "warmed_input_cancellation frames={ALLOCATION_CHECK_FRAMES} controls=2 shared_action=true ticks=0/1 frame_edges={} fixed_edges={} allocation_calls={allocations}",
        after.frame_edges - before.frame_edges,
        after.fixed_edges - before.fixed_edges
    );
    assert_eq!(allocations, 0, "warmed cancellation delivery allocated");
    Ok(())
}

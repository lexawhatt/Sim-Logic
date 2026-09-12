//! Real frame routing with clicks, cancellation, invalidation and outside holds.

use super::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ButtonAction {
    Activate,
    Disable,
}

#[derive(Resource)]
struct ButtonProbe {
    controller: PointerButton<u8>,
    rectangle: ScreenRectangleVisual,
    pressed: usize,
    clicked: usize,
    cancelled: usize,
    invalidated: usize,
    claimed: usize,
    background: usize,
}

fn route(input: FrameInput<ButtonAction>, mut state: ResMut<ButtonProbe>) {
    for edge in input.edges() {
        if edge.action() == ButtonAction::Disable && edge.state() == ButtonState::Pressed {
            assert_eq!(state.controller.cancel(), Some(1));
            state.invalidated += 1;
        }
        let hit = edge
            .pointer()
            .filter(|pointer| state.rectangle.contains_pointer(*pointer))
            .map(|_| 1);
        let result = state.controller.process(edge, hit);
        state.claimed += usize::from(result.claimed());
        if !result.claimed()
            && edge.control() == InputControl::MouseButton(MouseButton::Left)
            && edge.state() == ButtonState::Pressed
        {
            state.background += 1;
        }
        match result.event() {
            Some(PointerButtonEvent::Pressed {
                target,
                pointer,
                intent,
            }) => {
                assert_eq!(target, 1);
                assert_eq!(Some(pointer), edge.pointer());
                assert_eq!(intent, edge.intent());
                state.pressed += 1;
            }
            Some(PointerButtonEvent::Clicked {
                target,
                pointer,
                intent,
            }) => {
                assert_eq!(target, 1);
                assert_eq!(Some(pointer), edge.pointer());
                assert_eq!(intent, edge.intent());
                state.clicked += 1;
            }
            Some(PointerButtonEvent::Cancelled {
                target,
                reason,
                intent,
            }) => {
                assert_eq!(target, 1);
                assert_eq!(
                    reason,
                    PointerButtonCancellation::Input(InputCancellationReason::FocusLost)
                );
                assert_eq!(intent, edge.intent());
                state.cancelled += 1;
            }
            None => {}
            Some(_) => panic!("unexpected pointer button event"),
        }
    }
    assert_eq!(state.controller.captured(), None);
}

fn counts(runner: &HeadlessRunner<ButtonAction>) -> Result<[usize; 6], Box<dyn Error>> {
    let state = runner
        .resource::<ButtonProbe>()
        .ok_or("button counters missing")?;
    Ok([
        state.pressed,
        state.clicked,
        state.cancelled,
        state.invalidated,
        state.claimed,
        state.background,
    ])
}

pub(super) fn run_allocation_case() -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let inside = PointerSample::new(LogicalScreenPosition::new(20.0, 20.0), viewport)?;
    let outside = PointerSample::new(LogicalScreenPosition::new(200.0, 200.0), viewport)?;
    let mut app = Application::<ButtonAction>::new(AppConfig::default())?;
    app.bind_mouse_button(MouseButton::Left, ButtonAction::Activate)?;
    app.bind_key(PhysicalKeyCode::KeyD, ButtonAction::Disable)?;
    app.add_frame_system(route);
    let rectangle = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenVector::new(80.0, 30.0),
        Color::WHITE,
    )?;
    let camera = ActiveCamera2d::centered(10.0)?;
    let initial = app.register_world("button-allocation", move |world| {
        world.spawn(camera)?;
        world.spawn(rectangle)?;
        world.insert_resource(ButtonProbe {
            controller: PointerButton::new(MouseButton::Left),
            rectangle,
            pressed: 0,
            clicked: 0,
            cancelled: 0,
            invalidated: 0,
            claimed: 0,
            background: 0,
        })?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    // Frame UI still works while fixed simulation is paused and queues no
    // repeated copies of already interpreted UI edges for a later fixed tick.
    runner.set_paused(true);
    let press = InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed);
    let release = InputEvent::mouse_button(MouseButton::Left, ButtonState::Released);
    let events = [
        InputEvent::pointer_moved(inside),
        press,
        release,
        InputEvent::pointer_moved(outside),
        press,
        InputEvent::pointer_moved(inside),
        release,
        press,
        InputEvent::FocusLost,
        InputEvent::pointer_moved(inside),
        press,
        InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
        release,
        InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Released),
    ];
    let advance = |runner: &mut HeadlessRunner<ButtonAction>| -> Result<(), Box<dyn Error>> {
        let FrameOutcome::Advanced(report) =
            runner.advance_frame(FrameRequest::new(FIXED_STEP, &events, viewport))
        else {
            return Err("button frame rejected".into());
        };
        if report.failure().is_some()
            || report.fixed_ticks_attempted() != 0
            || report.exit_requested()
            || !matches!(report.transition(), FrameTransition::None)
            || report.extracted_generation() != Some(runner.world_generation())
        {
            return Err("button frame violated routing contract".into());
        }
        black_box(report);
        Ok(())
    };
    for _ in 0..WARM_UP_FRAMES {
        advance(&mut runner)?;
    }
    let before = counts(&runner)?;
    begin_allocation_count();
    let measured = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            advance(&mut runner)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured?;
    let after = counts(&runner)?;
    let delta = std::array::from_fn::<_, 6, _>(|i| after[i] - before[i]);
    assert_eq!(
        delta,
        [3, 1, 1, 1, 6, 1].map(|n| n * ALLOCATION_CHECK_FRAMES)
    );
    println!(
        "warmed_pointer_buttons frames={ALLOCATION_CHECK_FRAMES} pressed/clicked/cancelled/invalidated/claimed/background={delta:?} allocation_calls={allocations}"
    );
    assert_eq!(allocations, 0, "warmed button routing allocated");
    Ok(())
}

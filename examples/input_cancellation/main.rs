//! Scripted headless input diagnostics: no window, renderer, or font dependency.

use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Action {
    Activate,
}

#[derive(Default, Resource)]
struct Counts {
    frames: usize,
    presses: usize,
    ordinary_releases: usize,
    cancellations: usize,
}

fn inspect(input: FrameInput<Action>, mut counts: ResMut<Counts>) {
    counts.frames += 1;
    println!(
        "Frame {}: held={}, pointer_known={}",
        counts.frames,
        input.held(Action::Activate),
        input.pointer().is_some()
    );
    for edge in input.edges() {
        println!(
            "  {:?}: {:?}, cancellation={:?}",
            edge.control(),
            edge.state(),
            edge.cancellation_reason()
        );
        if edge.is_cancelled() {
            assert!(edge.pointer().is_none());
        }
    }
    let presses = input.pressed(Action::Activate).count();
    let releases = input.released(Action::Activate).count();
    let cancellations = input
        .released(Action::Activate)
        .filter(|edge| edge.is_cancelled())
        .count();
    counts.presses += presses;
    counts.ordinary_releases += releases - cancellations;
    counts.cancellations += cancellations;
    let expected = [(2, 2, false), (2, 1, true), (1, 2, false), (1, 1, false)];
    assert_eq!(
        (presses, releases, input.held(Action::Activate)),
        expected[counts.frames - 1]
    );
    let expected_reason = match counts.frames {
        2 => Some(InputCancellationReason::PointerLeft),
        3 => Some(InputCancellationReason::FocusLost),
        _ => None,
    };
    for edge in input.released(Action::Activate) {
        assert_eq!(edge.cancellation_reason(), expected_reason);
        if counts.frames == 2 {
            assert_eq!(edge.control(), InputControl::MouseButton(MouseButton::Left));
            assert!(input.pointer().is_none()); // Space still holds the shared action.
        }
    }
    if counts.frames == 3 {
        let controls = input.released(Action::Activate).map(|edge| edge.control());
        assert!(controls.eq([
            InputControl::Key(PhysicalKeyCode::Space),
            InputControl::MouseButton(MouseButton::Left),
        ]));
    }
}

fn main() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_input_event_limit(32)?;
    let mut app = Application::<Action>::new(config)?;
    app.bind_key(PhysicalKeyCode::Space, Action::Activate)?;
    app.bind_mouse_button(MouseButton::Left, Action::Activate)?;
    app.add_frame_system(inspect);
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("input-cancellation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Counts::default())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let pointer = PointerSample::new(LogicalScreenPosition::new(100.0, 80.0), viewport)?;
    let key = |state| InputEvent::key(PhysicalKeyCode::Space, state);
    let mouse = |state| InputEvent::mouse_button(MouseButton::Left, state);
    let batches: &[&[InputEvent]] = &[
        &[
            InputEvent::pointer_moved(pointer),
            key(ButtonState::Pressed),
            mouse(ButtonState::Pressed),
            mouse(ButtonState::Released),
            key(ButtonState::Released),
        ],
        &[
            key(ButtonState::Pressed),
            mouse(ButtonState::Pressed),
            InputEvent::PointerLeft,
        ],
        &[
            mouse(ButtonState::Pressed),
            InputEvent::FocusLost,
            InputEvent::FocusLost,
            key(ButtonState::Released),
            mouse(ButtonState::Released),
        ],
        &[key(ButtonState::Pressed), key(ButtonState::Released)],
    ];
    for events in batches {
        let report = match runner.advance_frame(FrameRequest::new(Duration::ZERO, events, viewport))
        {
            FrameOutcome::Advanced(report) => report,
            FrameOutcome::Rejected(error) => return Err(error.into()),
        };
        assert!(report.failure().is_none(), "{:?}", report.failure());
    }
    let counts = runner.resource::<Counts>().ok_or("counters missing")?;
    assert_eq!(
        (
            counts.frames,
            counts.presses,
            counts.ordinary_releases,
            counts.cancellations
        ),
        (4, 6, 3, 3)
    );
    println!("Verified: 6 presses, 3 ordinary releases, 3 cancellations. No window or GPU.");
    Ok(())
}

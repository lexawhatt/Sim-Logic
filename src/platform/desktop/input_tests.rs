//! Keyboard/focus collection uses the exact field-based host dispatch path.
//! Winit's private KeyEvent fields are not fabricated or accessed unsafely.

use std::{error::Error, time::Duration};

use sim_engine::LogicalViewport;
use winit::event::MouseButton as PlatformMouseButton;

use super::*;
use crate::{
    app::{AppConfig, Application},
    input::{
        ALL_PHYSICAL_KEYS, ActionEdge, FrameInput, InputCancellationReason, InputControl,
        MouseButton, PointerSample,
    },
    resources::AppResMut,
    system::Stage,
    visual::ActiveCamera2d,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Activate,
}

#[derive(Default, Clone)]
struct Observed {
    edges: Vec<ActionEdge<TestAction>>,
    held: bool,
    pointer: Option<PointerSample>,
}

fn observe(input: FrameInput<TestAction>, mut observed: AppResMut<Observed>) {
    observed.edges.clear();
    observed.edges.extend(input.edges());
    observed.held = input.held(TestAction::Activate);
    observed.pointer = input.pointer();
}

fn host(limit: usize) -> Result<DesktopHost<TestAction>, Box<dyn Error>> {
    let mut application = Application::new(AppConfig::default())?;
    for key in ALL_PHYSICAL_KEYS {
        application.bind_key(key, TestAction::Activate)?;
    }
    for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
        application.bind_mouse_button(button, TestAction::Activate)?;
    }
    application.register_app_resource(Observed::default())?;
    application.add_system(Stage::FrameUpdate, observe);
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = application.register_world("desktop-input-provenance", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut host = DesktopHost::new(
        application.build_headless(initial)?,
        DesktopConfig::default(),
        limit,
        frame_budget(FrameLimits::default()),
        ThreeDRenderLimits::default(),
    );
    host.pointer_geometry = DesktopPointerGeometry::new(PhysicalSize::new(800, 600), 1.0)?;
    Ok(host)
}

fn advance(host: &mut DesktopHost<TestAction>) -> Result<Observed, Box<dyn Error>> {
    assert_eq!(host.input_failure, None);
    let outcome = advance_shared_core(
        &mut host.runner,
        FrameRequest::new(
            Duration::from_millis(20),
            &host.pending_events,
            LogicalViewport::new(800.0, 600.0)?,
        ),
    );
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("desktop input frame should advance".into());
    };
    assert!(report.failure().is_none());
    host.pending_events.clear();
    host.runner
        .app_resource::<Observed>()
        .cloned()
        .ok_or_else(|| "missing input observations".into())
}

#[test]
fn focus_boundary_expansion_and_cancellation_order_belong_to_shared_core()
-> Result<(), Box<dyn Error>> {
    let mut host = host(ALL_PHYSICAL_KEYS.len() + 5)?;
    host.collect_cursor(PhysicalPosition::new(25.0, 35.0))?;
    for key in ALL_PHYSICAL_KEYS.into_iter().rev() {
        host.collect_physical_key(key, ButtonState::Pressed);
    }
    for button in [
        PlatformMouseButton::Middle,
        PlatformMouseButton::Right,
        PlatformMouseButton::Left,
    ] {
        host.collect_mouse_button(button, ElementState::Pressed);
    }
    let before_boundary = host.pending_events.len();
    host.collect_focus_loss();
    assert_eq!(host.pending_events.len(), before_boundary + 1);
    assert_eq!(host.pending_events.last(), Some(&InputEvent::FocusLost));
    assert!(host.held_keys.iter().all(|held| !held));

    let observed = advance(&mut host)?;
    assert!(!observed.held);
    assert_eq!(observed.pointer, None);
    let releases = observed
        .edges
        .iter()
        .filter(|edge| edge.state() == ButtonState::Released)
        .collect::<Vec<_>>();
    let expected = ALL_PHYSICAL_KEYS
        .into_iter()
        .map(InputControl::Key)
        .chain(
            [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
                .map(InputControl::MouseButton),
        )
        .collect::<Vec<_>>();
    assert_eq!(
        releases
            .iter()
            .map(|edge| edge.control())
            .collect::<Vec<_>>(),
        expected
    );
    for edge in releases {
        assert!(edge.is_cancelled());
        assert_eq!(
            edge.cancellation_reason(),
            Some(InputCancellationReason::FocusLost)
        );
        assert_eq!(edge.pointer(), None);
    }

    host.collect_focus_loss();
    let observed = advance(&mut host)?;
    assert!(observed.edges.is_empty());
    assert!(!observed.held);
    Ok(())
}

#[test]
fn synthetic_release_before_focus_loss_does_not_hide_cancellation() -> Result<(), Box<dyn Error>> {
    let mut host = host(2)?;
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Released,
        true,
        false,
    );
    assert_eq!(host.pending_events.len(), 1);
    assert!(host.held_keys[physical_key_index(PhysicalKeyCode::KeyW)]);
    host.collect_focus_loss();

    let observed = advance(&mut host)?;
    assert_eq!(observed.edges.len(), 2);
    assert_eq!(observed.edges[0].state(), ButtonState::Pressed);
    assert!(!observed.edges[0].is_cancelled());
    assert_eq!(observed.edges[1].state(), ButtonState::Released);
    assert_eq!(
        observed.edges[1].cancellation_reason(),
        Some(InputCancellationReason::FocusLost)
    );
    assert!(!observed.held);
    Ok(())
}

#[test]
fn focus_return_ignores_restoration_and_repeat_until_a_fresh_press() -> Result<(), Box<dyn Error>> {
    let mut host = host(4)?;
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    host.collect_focus_loss();
    assert!(!advance(&mut host)?.held);

    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        true,
        false,
    );
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        true,
    );
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        true,
        true,
    );
    assert!(host.pending_events.is_empty());
    assert!(!host.held_keys[physical_key_index(PhysicalKeyCode::KeyW)]);
    assert!(advance(&mut host)?.edges.is_empty());

    // Releasing the original physically held key after return does not emit a
    // second release. The next genuine non-repeat press starts a new action.
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Released,
        false,
        false,
    );
    assert!(advance(&mut host)?.edges.is_empty());
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    let observed = advance(&mut host)?;
    assert!(observed.held);
    assert_eq!(observed.edges.len(), 1);
    assert_eq!(
        observed.edges[0].control(),
        InputControl::Key(PhysicalKeyCode::KeyW)
    );
    assert!(!observed.edges[0].is_cancelled());
    Ok(())
}

#[test]
fn synthetic_events_repeated_presses_and_unmapped_keys_use_no_queue_capacity()
-> Result<(), Box<dyn Error>> {
    let mut host = host(0)?;
    for state in [ElementState::Pressed, ElementState::Released] {
        for repeat in [false, true] {
            host.collect_key(PhysicalKey::Code(KeyCode::KeyW), state, true, repeat);
        }
    }
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        true,
    );
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyQ),
        ElementState::Pressed,
        false,
        false,
    );
    assert!(host.pending_events.is_empty());
    assert_eq!(host.input_failure, None);
    assert!(host.held_keys.iter().all(|held| !held));
    Ok(())
}

#[test]
fn repeat_flag_does_not_suppress_an_ordinary_key_release() -> Result<(), Box<dyn Error>> {
    let mut host = host(2)?;
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    host.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Released,
        false,
        true,
    );
    let observed = advance(&mut host)?;
    assert_eq!(observed.edges.len(), 2);
    assert_eq!(observed.edges[1].state(), ButtonState::Released);
    assert!(!observed.edges[1].is_cancelled());
    assert!(!observed.held);
    Ok(())
}

#[test]
fn focus_clears_stationary_pointer_before_resize_and_scale_updates() -> Result<(), Box<dyn Error>> {
    let mut host = host(4)?;
    host.collect_cursor(PhysicalPosition::new(200.0, 100.0))?;
    host.collect_focus_loss();
    let boundary_count = host.pending_events.len();
    host.collect_resize(PhysicalSize::new(1200, 800))?;
    host.collect_scale_factor(2.0)?;
    assert_eq!(host.pending_events.len(), boundary_count);
    assert_eq!(advance(&mut host)?.pointer, None);
    host.collect_cursor(PhysicalPosition::new(300.0, 200.0))?;
    let observed = advance(&mut host)?;
    let pointer = observed
        .pointer
        .ok_or("fresh motion should restore a pointer sample")?;
    assert_eq!(
        pointer.position(),
        sim_engine::LogicalScreenPosition::new(150.0, 100.0)
    );
    Ok(())
}

#[test]
fn rejected_focus_boundary_preserves_local_state_and_overflow_remains_sticky()
-> Result<(), Box<dyn Error>> {
    let mut host = host(3)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Pressed);
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    let keys = host.held_keys;
    let geometry = host.pointer_geometry;
    let pending = host.pending_events.clone();
    host.collect_focus_loss();
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    assert_eq!(host.held_keys, keys);
    assert_eq!(host.pointer_geometry, geometry);
    assert_eq!(host.pending_events, pending);

    // Even apparent room cannot recover a queue that lost an earlier event.
    host.pending_events.pop();
    let pending = host.pending_events.clone();
    for failure in [InputBufferFailure::Limit, InputBufferFailure::Allocation] {
        host.input_failure = Some(failure);
        host.collect_focus_loss();
        host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Released);
        host.collect_pointer_left();
        host.collect_cursor(PhysicalPosition::new(70.0, 80.0))?;
        host.collect_resize(PhysicalSize::new(1000, 700))?;
        host.collect_scale_factor(2.0)?;
        assert_eq!(host.input_failure, Some(failure));
        assert_eq!(host.held_keys, keys);
        assert_eq!(host.pointer_geometry, geometry);
        assert_eq!(host.pending_events, pending);
    }
    Ok(())
}

#[test]
fn rejected_pointer_events_do_not_publish_proposed_geometry() -> Result<(), Box<dyn Error>> {
    for change in 0..4 {
        let mut host = host(2)?;
        host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
        host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Pressed);
        let geometry = host.pointer_geometry;
        let pending = host.pending_events.clone();
        match change {
            0 => host.collect_cursor(PhysicalPosition::new(70.0, 80.0))?,
            1 => host.collect_pointer_left(),
            2 => host.collect_resize(PhysicalSize::new(1000, 700))?,
            _ => host.collect_scale_factor(2.0)?,
        }
        assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
        assert_eq!(host.pointer_geometry, geometry);
        assert_eq!(host.pending_events, pending);
    }
    Ok(())
}

#[test]
fn rejected_key_press_or_release_preserves_adapter_held_state() -> Result<(), Box<dyn Error>> {
    let mut pressed = host(0)?;
    pressed.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    assert!(!pressed.held_keys[physical_key_index(PhysicalKeyCode::KeyW)]);
    assert_eq!(pressed.input_failure, Some(InputBufferFailure::Limit));

    let mut released = host(1)?;
    released.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Pressed,
        false,
        false,
    );
    released.collect_key(
        PhysicalKey::Code(KeyCode::KeyW),
        ElementState::Released,
        false,
        false,
    );
    assert!(released.held_keys[physical_key_index(PhysicalKeyCode::KeyW)]);
    assert_eq!(released.input_failure, Some(InputBufferFailure::Limit));
    Ok(())
}

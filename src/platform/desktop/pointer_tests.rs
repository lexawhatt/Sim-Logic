use std::{error::Error, time::Duration};

use sim_engine::{Camera2d, LogicalScreenPosition, LogicalViewport, Vec2};
use winit::event::MouseButton as PlatformMouseButton;

use super::*;
use crate::{
    app::AppConfig,
    input::{FrameInput, FrameInputEvent, MouseButton, PointerSample},
    resources::AppResMut,
    system::Stage,
    visual::ActiveCamera2d,
};

#[path = "scroll_tests.rs"]
mod scroll_tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Key,
    Left,
    Right,
    Middle,
}

#[derive(Default)]
struct Observed {
    events: Vec<FrameInputEvent<TestAction>>,
    pointer: Option<PointerSample>,
    presses: Vec<(TestAction, Option<PointerSample>)>,
    releases: Vec<(TestAction, Option<PointerSample>)>,
}

fn observe(input: FrameInput<TestAction>, mut observed: AppResMut<Observed>) {
    observed.events.extend(input.events());
    observed.pointer = input.pointer();
    for action in [
        TestAction::Key,
        TestAction::Left,
        TestAction::Right,
        TestAction::Middle,
    ] {
        observed
            .presses
            .extend(input.pressed(action).map(|edge| (action, edge.pointer())));
        observed
            .releases
            .extend(input.released(action).map(|edge| (action, edge.pointer())));
    }
}

fn host(limit: usize) -> Result<DesktopHost<TestAction>, Box<dyn Error>> {
    let mut application = Application::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::KeyW, TestAction::Key)?;
    application.bind_mouse_button(MouseButton::Left, TestAction::Left)?;
    application.bind_mouse_button(MouseButton::Right, TestAction::Right)?;
    application.bind_mouse_button(MouseButton::Middle, TestAction::Middle)?;
    application.register_app_resource(Observed::default())?;
    application.add_system(Stage::FrameUpdate, observe);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let initial = application.register_world("desktop-pointer", move |world| {
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

fn advance(host: &mut DesktopHost<TestAction>) -> Result<&Observed, Box<dyn Error>> {
    assert_eq!(host.input_failure, None);
    let outcome = advance_shared_core(
        &mut host.runner,
        FrameRequest::new(
            Duration::ZERO,
            &host.pending_events,
            LogicalViewport::new(800.0, 600.0)?,
        ),
    );
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("pointer frame should advance".into());
    };
    assert!(report.failure().is_none());
    host.pending_events.clear();
    host.runner
        .app_resource::<Observed>()
        .ok_or_else(|| "missing pointer observations".into())
}

fn sample(x: f32, y: f32, width: f32, height: f32) -> Result<PointerSample, Box<dyn Error>> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(width, height)?,
    )?)
}

#[test]
fn trailing_motion_replaces_at_capacity_without_crossing_a_button() -> Result<(), Box<dyn Error>> {
    let mut host = host(3)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    host.collect_cursor(PhysicalPosition::new(30.0, 40.0))?;
    let allocation = host.pending_events.as_ptr();
    let capacity = host.pending_events.capacity();
    for x in 50..100 {
        host.collect_cursor(PhysicalPosition::new(f64::from(x), 60.0))?;
    }
    assert_eq!(host.pending_events.len(), 3);
    assert_eq!(host.pending_events.capacity(), capacity);
    assert_eq!(host.pending_events.as_ptr(), allocation);
    assert_eq!(host.input_failure, None);
    let observed = advance(&mut host)?;
    assert_eq!(
        observed.presses,
        [(TestAction::Left, Some(sample(10.0, 20.0, 800.0, 600.0)?))]
    );
    assert_eq!(observed.pointer, Some(sample(99.0, 60.0, 800.0, 600.0)?));
    Ok(())
}

#[test]
fn key_and_leave_events_are_motion_coalescing_barriers() -> Result<(), Box<dyn Error>> {
    let mut host = host(5)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Pressed);
    host.collect_cursor(PhysicalPosition::new(30.0, 40.0))?;
    host.collect_pointer_left();
    host.collect_cursor(PhysicalPosition::new(50.0, 60.0))?;
    assert_eq!(
        host.pending_events,
        [
            InputEvent::pointer_moved(sample(10.0, 20.0, 800.0, 600.0)?),
            InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
            InputEvent::pointer_moved(sample(30.0, 40.0, 800.0, 600.0)?),
            InputEvent::PointerLeft,
            InputEvent::pointer_moved(sample(50.0, 60.0, 800.0, 600.0)?),
        ]
    );
    Ok(())
}

#[test]
fn motion_cannot_cross_a_full_key_barrier_and_failures_stay_sticky() -> Result<(), Box<dyn Error>> {
    let mut host = host(2)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Pressed);
    let before = host.pending_events.clone();
    host.collect_cursor(PhysicalPosition::new(30.0, 40.0))?;
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    assert_eq!(host.pending_events, before);

    // Once a physical event failed, even a later replaceable motion must not
    // conceal the missing event by changing the retained queue.
    host.pending_events.pop();
    let retained = host.pending_events.clone();
    host.collect_cursor(PhysicalPosition::new(50.0, 60.0))?;
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    assert_eq!(host.pending_events, retained);
    host.input_failure = Some(InputBufferFailure::Allocation);
    host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Released);
    assert_eq!(host.input_failure, Some(InputBufferFailure::Allocation));
    assert_eq!(host.pending_events, retained);
    Ok(())
}

#[test]
fn zero_capacity_fails_even_for_the_first_motion() -> Result<(), Box<dyn Error>> {
    let mut host = host(0)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    assert!(host.pending_events.is_empty());
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    Ok(())
}

#[test]
fn stationary_resize_and_dpi_refresh_only_later_click_geometry() -> Result<(), Box<dyn Error>> {
    let mut host = host(16)?;
    host.collect_cursor(PhysicalPosition::new(200.0, 100.0))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Released);
    host.collect_resize(PhysicalSize::new(1200, 800))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Released);
    host.collect_scale_factor(2.0)?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Released);
    host.collect_resize(PhysicalSize::new(1600, 1000))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    let observed = advance(&mut host)?;
    assert_eq!(
        observed.presses,
        [
            (TestAction::Left, Some(sample(200.0, 100.0, 800.0, 600.0)?)),
            (TestAction::Left, Some(sample(200.0, 100.0, 1200.0, 800.0)?)),
            (TestAction::Left, Some(sample(100.0, 50.0, 600.0, 400.0)?)),
            (TestAction::Left, Some(sample(100.0, 50.0, 800.0, 500.0)?)),
        ]
    );
    assert_eq!(observed.pointer, Some(sample(100.0, 50.0, 800.0, 500.0)?));
    Ok(())
}

#[test]
fn focus_loss_releases_keyboard_before_pointer_and_clears_stationary_position()
-> Result<(), Box<dyn Error>> {
    let mut host = host(16)?;
    host.collect_cursor(PhysicalPosition::new(200.0, 100.0))?;
    host.collect_physical_key(PhysicalKeyCode::KeyW, ButtonState::Pressed);
    for button in [
        PlatformMouseButton::Middle,
        PlatformMouseButton::Right,
        PlatformMouseButton::Left,
    ] {
        host.collect_mouse_button(button, ElementState::Pressed);
    }
    host.collect_focus_loss();
    assert_eq!(&host.pending_events[5..], [InputEvent::FocusLost]);
    host.collect_resize(PhysicalSize::new(1200, 800))?;
    host.collect_scale_factor(2.0)?;
    assert_eq!(host.pending_events.len(), 6);
    let observed = advance(&mut host)?;
    assert_eq!(observed.pointer, None);
    assert_eq!(
        observed.releases,
        [
            (TestAction::Key, None),
            (TestAction::Left, None),
            (TestAction::Right, None),
            (TestAction::Middle, None),
        ]
    );
    host.collect_focus_loss();
    assert_eq!(host.pending_events, [InputEvent::FocusLost]);
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    let observed = advance(&mut host)?;
    assert_eq!(observed.presses.last(), Some(&(TestAction::Left, None)));
    assert_eq!(observed.releases.len(), 4);
    Ok(())
}

#[test]
fn zero_size_and_leave_require_a_new_motion_after_restoration() -> Result<(), Box<dyn Error>> {
    for cleared_by_zero_size in [false, true] {
        let mut host = host(16)?;
        host.collect_cursor(PhysicalPosition::new(-20.0, 900.0))?;
        host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
        if cleared_by_zero_size {
            host.collect_resize(PhysicalSize::new(0, 600))?;
            host.collect_cursor(PhysicalPosition::new(200.0, 300.0))?;
        } else {
            host.collect_pointer_left();
        }
        host.collect_resize(PhysicalSize::new(800, 600))?;
        host.collect_scale_factor(2.0)?;
        host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
        host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Released);
        host.collect_cursor(PhysicalPosition::new(-20.0, 900.0))?;
        host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
        let observed = advance(&mut host)?;
        assert_eq!(
            observed.presses,
            [
                (TestAction::Left, Some(sample(-20.0, 900.0, 800.0, 600.0)?)),
                (TestAction::Left, None),
                (TestAction::Left, Some(sample(-10.0, 450.0, 400.0, 300.0)?)),
            ]
        );
        assert_eq!(observed.releases[0], (TestAction::Left, None));
    }
    Ok(())
}

#[test]
fn unsupported_mouse_buttons_do_not_consume_capacity() -> Result<(), Box<dyn Error>> {
    let mut host = host(0)?;
    for button in [
        PlatformMouseButton::Back,
        PlatformMouseButton::Forward,
        PlatformMouseButton::Other(42),
    ] {
        host.collect_mouse_button(button, ElementState::Pressed);
    }
    assert!(host.pending_events.is_empty());
    assert_eq!(host.input_failure, None);
    Ok(())
}

#[test]
fn invalid_pointer_conversions_are_typed_and_preserve_geometry_and_queue()
-> Result<(), Box<dyn Error>> {
    let mut host = host(16)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    let geometry = host.pointer_geometry;
    let pending = host.pending_events.clone();
    for position in [
        PhysicalPosition::new(f64::NAN, 20.0),
        PhysicalPosition::new(10.0, f64::INFINITY),
        PhysicalPosition::new(f64::MAX, 20.0),
    ] {
        let error = host
            .collect_cursor(position)
            .err()
            .ok_or("invalid cursor must fail")?;
        assert!(matches!(error, DesktopPointerError::InvalidPosition { .. }));
        assert!(DesktopRunError::Pointer(error).source().is_some());
        assert_eq!(host.pointer_geometry, geometry);
        assert_eq!(host.pending_events, pending);
    }
    for scale in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::MAX,
        f64::MIN_POSITIVE,
    ] {
        assert!(matches!(
            host.collect_scale_factor(scale),
            Err(DesktopPointerError::InvalidScaleFactor { .. })
        ));
        assert_eq!(host.pointer_geometry, geometry);
        assert_eq!(host.pending_events, pending);
        assert!(matches!(
            DesktopPointerGeometry::new(PhysicalSize::new(800, 600), scale),
            Err(DesktopPointerError::InvalidScaleFactor { .. })
        ));
    }
    Ok(())
}

#[test]
fn failed_stationary_scale_conversion_preserves_the_previous_sample() -> Result<(), Box<dyn Error>>
{
    let mut host = host(16)?;
    host.collect_cursor(PhysicalPosition::new(f64::from(f32::MAX), 20.0))?;
    let geometry = host.pointer_geometry;
    let pending = host.pending_events.clone();
    assert!(matches!(
        host.collect_scale_factor(0.5),
        Err(DesktopPointerError::InvalidPosition { .. })
    ));
    assert_eq!(host.pointer_geometry, geometry);
    assert_eq!(host.pending_events, pending);
    Ok(())
}

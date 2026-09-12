use super::*;
use crate::input::{ScrollDelta, ScrollDeltaError};

#[test]
fn wheel_keeps_event_time_dpi_pointer_units_and_click_order() -> Result<(), Box<dyn Error>> {
    let mut host = host(16)?;
    host.collect_cursor(PhysicalPosition::new(200.0, 100.0))?;
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.25, -0.5))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Pressed);
    host.collect_scale_factor(2.0)?;
    host.collect_mouse_wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
        8.0, -4.0,
    )))?;
    host.collect_cursor(PhysicalPosition::new(400.0, 300.0))?;
    host.collect_mouse_button(PlatformMouseButton::Left, ElementState::Released);
    let observed = advance(&mut host)?;
    let [
        FrameInputEvent::Scroll(first),
        FrameInputEvent::Action(press),
        FrameInputEvent::Scroll(second),
        FrameInputEvent::Action(release),
    ] = observed.events.as_slice()
    else {
        return Err("wheel and mouse edges must remain interleaved".into());
    };
    assert_eq!(first.delta(), ScrollDelta::lines(0.25, -0.5)?);
    assert_eq!(first.pointer(), Some(sample(200.0, 100.0, 800.0, 600.0)?));
    assert_eq!(press.pointer(), first.pointer());
    assert_eq!(second.delta(), ScrollDelta::pixels(4.0, -2.0)?);
    assert_eq!(second.pointer(), Some(sample(100.0, 50.0, 400.0, 300.0)?));
    assert_eq!(release.pointer(), Some(sample(200.0, 150.0, 400.0, 300.0)?));
    Ok(())
}

#[test]
fn wheel_is_a_motion_barrier_and_is_never_coalesced_at_capacity() -> Result<(), Box<dyn Error>> {
    let mut host = host(3)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 1.0))?;
    host.collect_cursor(PhysicalPosition::new(30.0, 40.0))?;
    host.collect_cursor(PhysicalPosition::new(50.0, 60.0))?;
    assert_eq!(host.pending_events.len(), 3);
    let before = host.pending_events.clone();
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.0, -1.0))?;
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    assert_eq!(host.pending_events, before);

    let mut host = super::host(2)?;
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 0.25))?;
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 0.25))?;
    assert_eq!(host.pending_events.len(), 2);
    host.collect_mouse_wheel(MouseScrollDelta::LineDelta(0.0, 0.25))?;
    assert_eq!(host.input_failure, Some(InputBufferFailure::Limit));
    Ok(())
}

#[test]
fn invalid_wheel_conversion_preserves_queue_and_geometry() -> Result<(), Box<dyn Error>> {
    let mut host = host(8)?;
    host.collect_cursor(PhysicalPosition::new(10.0, 20.0))?;
    let before = host.pending_events.clone();
    let geometry = host.pointer_geometry;
    assert_eq!(
        host.collect_mouse_wheel(MouseScrollDelta::LineDelta(f32::NAN, 1.0)),
        Err(DesktopPointerError::Scroll(ScrollDeltaError::NonFinite))
    );
    assert_eq!(host.pending_events, before);
    assert_eq!(host.pointer_geometry, geometry);
    host.collect_scale_factor(0.5)?;
    assert_eq!(
        host.collect_mouse_wheel(MouseScrollDelta::PixelDelta(PhysicalPosition::new(
            ScrollDelta::MAX_DISPLACEMENT,
            0.0,
        ))),
        Err(DesktopPointerError::Scroll(ScrollDeltaError::OutOfRange))
    );
    Ok(())
}

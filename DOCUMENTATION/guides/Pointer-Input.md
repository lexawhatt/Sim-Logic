# Pointer input and click-to-place

[Documentation](../README.md) / Guides

Sim;Logic accepts a cursor position and left, right, or middle mouse buttons
through the same headless input path as keyboard events. Bind a mouse button
to an action with `Application::bind_mouse_button`. Binding a physical button
twice returns `DuplicateMouseBinding` and preserves its original binding.

Try the complete [click_to_place example](../../examples/click_to_place/main.rs):

```bash
cargo run --release --example click_to_place
```

Left-click places a circle at the next fixed update. Right-click clears all
placed circles; if placement and clear arrive in the same fixed tick, clear
takes priority. Escape exits. The scene permits at most 128 placed circles,
and further placement presses do nothing until space is freed. A press without
a known cursor position does nothing. Creation and clearing use deferred
Commands, so the structural changes appear at the fixed stage barrier.

## Position at the click

`FrameInput::pointer()` and `FixedInput::pointer()` return the latest accepted
`Option<PointerSample>`. `None` means that no position is known. Movement does
not need a button binding, and the position persists across frames and ticks.

An `ActionEdge` returned by `pressed` or `released` has its own `pointer()`.
`FrameInput::edges()` exposes presses and releases together in physical event
order. Use it for dragging: a complete press/move/release can fit inside one
frame, and the release edge's pointer remains the endpoint even if later
motion changes the frame's latest pointer.
For a mouse edge, this is the last sample accepted before that particular
button event. Use the edge's sample for clicking: the pointer may move again
before a System reads the frame. Keyboard edges and synthetic releases caused
by leaving the window have no pointer sample. A mouse edge with no known
position is still a valid action occurrence and carries `None`.

For example, these events produce a press at A and a latest position of B:

```rust
use sim_logic::prelude::*;

fn main() -> LogicResult {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let a = PointerSample::new(LogicalScreenPosition::new(200.0, 150.0), viewport)?;
    let b = PointerSample::new(LogicalScreenPosition::new(600.0, 450.0), viewport)?;
    let events = [
        InputEvent::pointer_moved(a),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
        InputEvent::pointer_moved(b),
    ];
    assert_eq!(events.len(), 3);
    Ok(())
}
```

Pass that slice in a `FrameRequest` to drive a headless runner. A frame's
current viewport does not replace the viewport already recorded in a sample.
When a headless host changes geometry around a stationary pointer, it submits
a refreshed `InputEvent::pointer_moved` sample explicitly.
The [public pointer tests](../../tests/pointer_input/runtime.rs) exercise event
order, delayed fixed delivery, pause, and World replacement without a window.

## Coordinates and the camera

Each immutable `PointerSample` contains a `LogicalScreenPosition` and the
`LogicalViewport` that was current when the sample was collected. Both use
logical pixels. The origin is the top-left of the window content, X increases
rightward, and Y increases downward. Logical pixels account for the window's
scale factor; they are not necessarily physical display pixels.

`PointerSample::new(position, viewport)` rejects a non-finite position with
`PointerSampleError`.
Negative and outside positions are valid and are never clamped.
`position()` and `viewport()` return the stored values.

`sample.world_position(camera)` calls Sim;Engine's depth-zero
`Camera2d::screen_to_world` using the sample's stored geometry and returns its
`Result<Vec2, Camera2dError>`, including projection errors. The application
explicitly chooses the camera. The click-to-place example reads the canonical
`ActiveCamera2d`, which it keeps static, and converts each edge separately:

```rust,ignore
for edge in input.pressed(DemoAction::Place) {
    let Some(pointer) = edge.pointer() else {
        continue;
    };
    let position = pointer.world_position(camera.camera())?;
    // Apply the example's object limit before queuing a spawn at position.
}
```

This helper converts a screen point onto the camera's depth-zero plane. It
does not find a rendered entity or choose an interpolated camera for you.
If the camera moves between event collection and consumption, the camera
passed by the application determines the result. Object hit testing and the
choice of camera history belong to application code.

## Stages, pause, and World replacement

Mouse and keyboard controls can share an action. `held(action)` remains true
as long as any bound physical control for it is down. Every separate physical
press or release still creates its own edge and transition intent. Repeating
a press while that same control is already down creates no additional edge.

Frame edges belong to the current application frame. Fixed edges wait through
frames that run no fixed tick, then appear in the first consuming fixed tick
only. All Systems in that tick can inspect the same immutable edges. The
latest continuous pointer remains available in later catch-up ticks even
though those ticks receive no repeated edges.

Input parameters are stage-specific: `FrameInput` exposes no pointer or
actions in FixedUpdate; `FixedInput` exposes none in FrameUpdate. Startup has
no input resources, so input System parameters cannot be used there.
Ordinary stage cleanup clears delivered edges and keeps the application's
continuous pointer state.

Pause continues to collect pointer movement and frame edges. As with keyboard
input, fixed edges are discarded on entering pause and are not queued while
paused. Resuming keeps the current position and held controls without replaying
old clicks.
World replacement likewise clears old causal edges while preserving the
application-owned pointer and held state. The target's Startup has no input
resources; its later update stages see the preserved continuous state.

## Leaving, resizing, and desktop delivery

`InputEvent::PointerLeft` clears the known position and releases held bound
mouse buttons in Left, Right, Middle order. Each synthesized release has no
pointer sample. A repeated leave creates no additional edges. In the core,
this event does not release keyboard keys.

The desktop adapter handles leaving the window, losing focus, and a zero-size
viewport by clearing its cursor and queuing `PointerLeft`. Focus loss also
releases keyboard keys first. After re-entry or restoration, a new cursor
movement is needed before the adapter knows a position again.

The adapter records physical cursor coordinates, extent, and scale in event
order. A delivered resize or scale change refreshes a known stationary
pointer before later button events. Historical click geometry is not rebuilt
from the Window's later state. Invalid coordinates or geometry return a typed
desktop error.

Consecutive queued movements may replace only the final queued movement,
including when the queue is full. A key event, button event, or leave separates
movement samples and prevents coalescing across that boundary.

## Bounds and compatibility

`AppConfig::set_input_event_limit` sets three separate bounds with the same
value: physical events in a frame, logical edges generated for that frame,
and logical edges retained for the next fixed delivery. A single leave can
generate up to three release edges, so physical event count alone is not
sufficient. Frame edge limits still apply while paused. A rejected collection
leaves held controls, the pointer, occurrence identities, and delivery queues
unchanged. A desktop buffer error remains a failure even if later movement
could otherwise be coalesced.

`InputEvent` and `ActionEdge` retain lawful `Eq` and `Hash`, including samples
whose coordinates differ only in the sign of zero. Adding `PointerMoved`,
`MouseButton`, and `PointerLeft` does change source compatibility for
downstream exhaustive matches on `InputEvent`; update those matches for the
new variants. Exhaustive error matches also need
`InputCollectionError::FrameEdgeLimitExceeded` and, with desktop enabled,
`DesktopRunError::Pointer`.

This slice has no pointer capture, wheel, touch, pen, text entry, UI focus, or
built-in hit testing. Drawing a `ScreenRectangleVisual` does not make it
capture mouse input.

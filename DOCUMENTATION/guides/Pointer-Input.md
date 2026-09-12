# Pointer input, control origins, and cancellation

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
before a System reads the frame. Keyboard edges and runtime-generated
cancellation releases have no pointer sample. A mouse edge with no known
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

## Which control, and why did it release?

Several physical controls can share one action. For example, bind both Space
and the left mouse button to `Activate`. `edge.control()` distinguishes
`InputControl::Key(PhysicalKeyCode::Space)` from
`InputControl::MouseButton(MouseButton::Left)` without guessing from whether
the edge has a pointer position. A mouse press can legitimately have no known
position, so `pointer().is_some()` is not a substitute for control identity.

An ordinary release and a cancelled interaction both appear in
`input.released(action)`. Check `edge.cancellation_reason()` or
`edge.is_cancelled()` before treating a release as confirmation:

| Release | `cancellation_reason()` | Meaning |
| --- | --- | --- |
| Actual key/button release | `None` | The physical control was released normally. |
| Pointer leaves or becomes unavailable | `Some(InputCancellationReason::PointerLeft)` | End the held mouse interaction without confirming it. Keyboard holds are unaffected. |
| Window loses focus | `Some(InputCancellationReason::FocusLost)` | End held mapped keyboard and mouse interactions without confirming them. |

For a release-to-confirm interaction, handle cancellation first:

```rust,ignore
for edge in input.released(Action::Activate) {
    if edge.is_cancelled() {
        // Cancel the interaction belonging to edge.control().
        continue;
    }
    // Finish the ordinary interaction using this edge's event-time sample.
}
```

The application still owns what "cancel" and "finish" mean. This is not
a button widget, drag manager, pointer capture, focus tree, or event consumption
API. All eligible Systems still see the same immutable edge sequence.
An ordinary release is not by itself a valid click: keep track of the matching
eligible press and its target before confirming an interaction.

Cancellation does not erase earlier presses. If a batch contains a press and
then focus loss, `pressed(action)` still contains that press and
`released(action)` contains the cancellation. Earlier queued fixed edges are
also not retrospectively rewritten. Use `FrameInput::edges()` when the order of starting
and cancelling matters; processing every press first and every release later
can change the meaning of several interactions in one batch. A cancelled
release gets its own occurrence identity and transition token like any other
edge; it does not replace or reuse the earlier press token.

Try the scripted [input_cancellation example](../../examples/input_cancellation/main.rs):

```bash
cargo run --no-default-features --example input_cancellation
```

It creates no window, renderer, or font. Four headless frames print each
control, press/release, and cancellation reason. They check ordinary releases,
pointer leave while Space keeps the shared action held, focus loss cancelling
both controls, duplicate cleanup events, and a fresh keyboard press afterward.
The final counters are six presses, three ordinary releases, and three
cancellations. This is a repeatable input diagnostic, not a test of physical
keyboard delivery on a particular desktop.

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
mouse buttons in Left, Right, Middle order. Each generated release carries
`PointerLeft` as its cancellation reason and has no pointer sample. A repeated
leave creates no additional edges. This event does not release keyboard keys.

`InputEvent::FocusLost` clears the position and releases held mapped keyboard
controls in the portable key catalog's order, followed by mouse buttons in
Left, Right, Middle order. This is not binding order or the order in which
controls were pressed. All generated releases carry the `FocusLost` reason
and no pointer sample. Repeated focus loss, or an ordinary release after a
control has already been cancelled, creates no duplicate edge.

Focus loss is one event, not a persistent core focus flag. Later events in
the same headless batch are still processed: a subsequent new press can hold
the action again. The core does not invent a focus-gained event or decide which
application owns future input; the host controls what it submits.

The desktop adapter queues `PointerLeft` when the cursor leaves or the viewport
becomes zero-sized, and `FocusLost` when the window loses focus. After re-entry
or restoration, a new cursor movement is needed before a position is known.
It ignores all winit synthetic keyboard events and repeated key presses.
Refocusing therefore does not replay a synthetic held-key press; a fresh
physical press is required to hold that action again. These discarded platform
events are different from the logical cancellation releases generated by the
core, which remain visible to Systems.

The adapter records physical cursor coordinates, extent, and scale in event
order. A delivered resize or scale change refreshes a known stationary
pointer before later button events. Historical click geometry is not rebuilt
from the Window's later state. Invalid coordinates or geometry return a typed
desktop error.

Consecutive queued movements may replace only the final queued movement,
including when the queue is full. A key event, button event, pointer leave, or
focus loss separates movement samples and prevents coalescing across that
boundary.

## Bounds and compatibility

`AppConfig::set_input_event_limit` sets three separate bounds with the same
value: physical events in a frame, logical edges generated for that frame,
and logical edges retained for the next fixed delivery. A single pointer leave
can generate up to three release edges; a focus-loss event can additionally
release every held mapped key. Physical event count alone is therefore not
sufficient. The runtime preflights the complete batch before changing held
state or publishing edges. Frame edge limits still apply while paused. A
rejected collection leaves held controls, the pointer, occurrence identities,
and delivery queues unchanged; it does not perform half of a cancellation.
The desktop runner reports such a rejected frame as an error and stops; it does
not silently drop releases or retry with a larger limit.
A desktop buffer error remains a failure even if later movement could
otherwise be coalesced.

`InputEvent` and `ActionEdge` retain lawful `Eq` and `Hash`, including samples
whose coordinates differ only in the sign of zero. Edge equality includes its
physical control and cancellation reason. Downstream exhaustive matches on
`InputEvent` must now handle `FocusLost` as well as `PointerMoved`,
`MouseButton`, and `PointerLeft`. Exhaustive error matches also need
`InputCollectionError::FrameEdgeLimitExceeded` and, with desktop enabled,
`DesktopRunError::Pointer`.

For explicit press/release ownership, use the small
[pointer-button helper](../rendering/Screen-HUD.md#buttons-and-local-input-ownership).
It uses these source/cancellation fields and caller-selected rectangle hits.
It does not consume these snapshots or add an OS pointer grab. Wheel, touch,
pen, text entry and UI keyboard focus are not implemented. Drawing a
`ScreenRectangleVisual` alone still does not capture input.

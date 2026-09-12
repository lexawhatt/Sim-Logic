# Screen-fixed panels

[Documentation](../README.md) / Rendering

A heads-up display, or HUD, is information drawn over the scene without moving
with its camera. `ScreenRectangleVisual` is the first small building block for
that: a filled rectangle for a status panel, progress bar, or pause indicator.
It is not a general UI toolkit.

Try the complete [screen_hud example](../../examples/screen_hud/main.rs):

```bash
cargo run --release --example screen_hud
```

The camera follows a moving body while the panel stays on screen. Space
pauses or resumes motion; the status strip changes from green to amber. The
blue bar tracks simulation progress, while the small light keeps pulsing
during pause. Resize the window to see the panel reposition. Escape exits.

## Coordinates and ownership

Screen coordinates use logical pixels with the origin at the top-left of the
window content. X increases rightward and Y downward. The position is the
rectangle's top-left corner, not its center. Logical pixels account for the
window's scale factor; they are not necessarily one physical display pixel.

The rectangle owns its position, size, color, layer, and draw-order depth. It
does not require a `Transform2d`, follow the World camera, or interpolate
between fixed ticks. FrameUpdate may change it directly, including while
fixed simulation is paused. An entity can explicitly own both screen and
World visuals, but their geometry remains independent.

## Lay out a panel during pause

This complete headless example positions a panel 16 logical pixels from the
right edge. It intentionally pauses fixed simulation before the frame to
verify that presentation layout still runs:

```rust
use std::time::Duration;
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

#[derive(Component)]
struct Panel;

fn place_panel(
    viewport: FrameViewport,
    panel: Single<&mut ScreenRectangleVisual, With<Panel>>,
) -> LogicResult {
    let mut panel = panel.into_inner();
    let x = viewport.logical().width() - panel.size().to_vec2().x() - 16.0;
    panel.set_position(LogicalScreenPosition::new(x, 16.0))?;
    Ok(())
}

fn main() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.approve_component::<Panel>()?;
    let camera = ActiveCamera2d::centered(32.0)?;
    let panel = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(16.0, 16.0),
        LogicalScreenVector::new(220.0, 24.0),
        Color::rgb8(68, 144, 255),
    )?;
    let initial = app.register_world("panel", move |world| {
        world.spawn(camera)?;
        world.spawn((Panel, panel))?;
        Ok(())
    })?;
    app.add_fallible_frame_system(place_panel);

    let mut runner = app.build_headless(initial)?;
    runner.set_paused(true);
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let report = match runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport)) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    assert_eq!(report.fixed_ticks_attempted(), 0);
    let frame = runner.extracted_frame().ok_or("snapshot is missing")?;
    let rectangle = frame.resolved_screen_rectangles().first().ok_or("panel is missing")?;
    assert_eq!(rectangle.position(), LogicalScreenPosition::new(564.0, 16.0));
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    Ok(())
}
```

`ScreenRectangleVisual` is automatically approved. Only the custom `Panel`
marker needs an approval call. `FrameViewport` is deliberately available to
FrameUpdate rather than FixedUpdate: resizing the window should not silently
change simulation rules. The active camera is still required because the
runtime validates the World scene even when it has no World-space shapes.

For a desktop application, replace the explicit headless driver with
`app.run(initial)?;` and enable the default features. The window loop supplies
the current viewport each frame.

## Updating and hiding it

`set_position`, `set_size`, `set_geometry`, `set_color`, and
`set_draw_order_depth` validate before changing the component. An error keeps
the old value intact. `set_geometry` changes position and size together, so
there is no intermediate geometry. `set_layer` is infallible.

Sizes must be positive and finite. Position plus size must produce finite,
strictly larger far bounds in both axes; floating-point precision can make an
extremely tiny size invalid at an extremely large position. Negative or
offscreen positions are allowed and are not clamped by the component.

A zero-width progress bar is therefore not a valid rectangle. Hide an empty
bar by removing its visual through Commands, or disable a dedicated bar entity
and enable it later. Disabling affects the whole entity, including any other
components it owns. Commands follow the ordinary stage barrier rules.

## Drawing order and limits

All screen rectangles draw above all World visuals, even if a World primitive
has a higher layer. Within the screen scene, layer, draw-order depth, and
stable source identity determine order. Without screen images, an empty screen
scene adds no extra render pass; a nonempty one follows the World pass.
[Images](Screen-Images.md) share this ordering and can split rectangles into
several contiguous runs, with an image source between runs.

The default limit is 256 screen rectangles. `RenderLimits` exposes
`with_max_screen_rectangles` and `with_screen_scene_budget` for changing the
source count and screen-scene work limits before startup. Desktop
`FrameLimits` also bound the combined presentation. Increasing only one limit
does not bypass the others.

World and screen extraction publish together. If either fails, no half-new
snapshot is published. Headless code can inspect
`ExtractedFrame::resolved_screen_rectangles()` and should check its World
generation as it would for any other snapshot.

## Buttons and local input ownership

`ScreenRectangleVisual::contains_pointer(sample)` checks its rectangle against
the sample's stored viewport. Left/top edges are included, right/bottom edges
excluded. An offscreen rectangle can be hit only in the visible part of that
viewport. This is geometry only: even a transparent rectangle can pass this
check. Your application chooses which entities are enabled, eligible and on
top. The rectangle is its current layout, not a saved event-time layout.

`PointerButton<T>` in `sim_logic::ui` tracks one physical mouse button and one
pressed target. `T` can be a small application enum or a generation-qualified
`LogicEntity`. Construct it with `PointerButton::new(MouseButton::Left)` and
feed the complete `FrameInput::edges()` stream once, in order:

```rust,ignore
for edge in input.edges() {
    // Pick the top eligible target at edge.pointer(), not input.pointer().
    let hit = pick_eligible_button(edge.pointer());
    let result = button.process(edge, hit);
    if let Some(PointerButtonEvent::Clicked { target, intent, .. }) = result.event() {
        // Apply the target's action; a World transition can use this release token.
    }
    if !result.claimed() {
        // This central router may forward the edge to application/game controls.
    }
}
```

A known press inside captures the target. An ordinary release over that same
eligible target produces `Clicked`; release elsewhere, unknown coordinates or
focus/pointer cancellation never confirms it. Pressing outside and releasing
inside does not click. Keyboard aliases and other mouse buttons leave capture
alone. `captured()` exposes the pressed target for styling; it is not hover.

Call `cancel()` as soon as a captured target is removed, disabled, or hidden
behind a modal. It returns that target for your cleanup and claims the rest of
the hold until release, so an invalidated UI gesture cannot become a game
gesture. Such suppressed edges can be claimed without producing an event.
Reintroducing the same target value does not restore its old capture.

This helper does not consume the shared input snapshots. Route competing
actions together, and do not separately process the same physical action in
another game System. FixedUpdate runs before FrameUpdate: UI decisions made in
FrameUpdate cannot undo earlier game actions. For fixed simulation, queue only
the accepted game intents for a later tick; do not feed this controller both
frame and fixed copies. Replaying a complete pair can produce another click.
Those queued intents are your application's ordinary data or commands. Saving
a `PointerButtonEvent` does not extend its transition token's runtime lifetime.

Keep controllers in World-local Resources that are rebuilt on replacement.
An application-owned controller needs explicit cancellation when targets change
worlds, generation-qualified IDs, and continued delivery of release events.
Saving a controller inside an inactive game region can miss releases. Persist
game data between regions, not unfinished UI interactions.

Run the [ui_buttons example](../../examples/ui_buttons/main.rs):

```bash
cargo run --release --features text --example ui_buttons
```

It routes UI and background input together in FrameUpdate. COUNT increments
an application-owned counter; PAUSE stops fixed updates, not the UI; ENABLE/
DISABLE COUNT changes eligibility; NEXT WORLD performs a real replacement and
keeps shared counters. Press D while holding COUNT to test invalidation. Click
the lower play area to place marks; a UI press never also places a mark.
The example uses the optional managed text feature and the existing licensed
font fixture. Labels change only when their displayed value changes.
It draws 7 screen rectangles and 28 text labels, with two shared fonts; fixed
time is displayed as whole seconds. The layout is fixed at logical 1000x700
and clips on smaller windows. After replacement, inherited counter labels show
`...` until the first following FrameUpdate: isolated factories cannot read
live Application Resources. The counters themselves survive immediately.

## What this does not provide

There are no rounded screen rectangles, automatic widget layouts, keyboard
focus navigation, text editing, OS pointer grabs, or clipping trees here.
[Optional font-backed labels](Text.md) are a separate drawing capability.
The button helper owns a local press/release sequence, not drag-motion sampling
or automatic global input routing. Drawing a rectangle alone does not make it
capture input. It belongs to the active
World and disappears on World replacement; it is not an application-owned
error overlay.

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

`SceneBudget`, `SceneBudgetResource`, `SceneError`, `Stroke` and the stroke style
argument types are exported by `sim_logic` and its prelude. No direct Engine
dependency is needed to construct a budget. The independent downstream test
at `tests/consumers/budget_api` admits 10,001 commands and verifies atomic
rejection of the next command.

## Native retention and recovery

The desktop host defaults to `DesktopScreenMode::Prepared`. It retains each
contiguous screen geometry run and compares exact source identities and values.
An unchanged run performs no new tessellation or geometry upload. Editing one
run rebuilds that run, not other runs. Changes that split/reorder runs may
invalidate more than one.

CPU extraction also keeps exact source proofs. Once both atomic publication
buffers are warm, identical source identities/values skip collection, sorting
and Engine scene reconstruction. Only moving/recolouring labels or images can
reuse the aggregate geometry scene and unchanged mixed geometry runs. Editing
geometry rebuilds the aggregate validation scene and only changed mixed runs.
This still scans current sources; it is not an O(1) ECS change index, and a changed
large curve may require comparing/validating its points. No hash collision or
user-supplied version can bless stale content. Source limits, registered assets,
World generation and publication failure atomicity remain enforced.

`ExtractedFrame::screen_extraction_updates()` reports compared sources, whole
snapshot/scene reuse, rebuilt/reused mixed runs and retained comparison metadata.
The byte counter covers one publication buffer's keys, scratch and run records;
it excludes shared payloads and Engine scenes, not total application memory.
`DesktopRunReport::last_screen_extraction()` retains these counters from the last
successful extraction, which may precede the last rejection or exit and is not
itself proof of a present. For same-workload CPU comparisons, disable only this
cache with `RenderLimits::with_screen_scene_reuse(false)`; native Prepared/Compact
selection remains independent. A rejected extraction invalidates its spare
proof and is retried normally without replacing the last published snapshot.

`DesktopConfig::set_screen_mode` also accepts `Streaming` for paired baseline
measurements and opt-in `Compact`. Compact uses Engine's analytic filled circles,
round lines and square rectangles. It splits at incompatible styles and every
256 records, preserving order; unsupported parts use ordinary prepared geometry
without forcing their simple neighbours down that route. Split parts consume
additional frame items, so the composed pass limit also needs to fit them.
It changes polygonal round boundaries to analytic
coverage and has Engine's stricter destination-dependent precision envelope.
Construction rejection can select the ordinary route, but a later target/DPI
precision rejection remains an error, not permission to weaken validation.
Use Prepared when that additional compact contract is unsuitable.

Layers, clips, alpha and interleaving with images/text keep their original
ordering. Screen geometry is already in logical pixels, so world-camera motion
alone does not invalidate it. Host-reprojected pan/zoom does change its source
values. Viewport/DPI changes are revalidated by Engine. World replacement clears
the cache; device replacement rebuilds it from the current CPU snapshot. The
cache mirrors current runs rather than keeping a history of edits. Retained
work is bounded by screen admission limits; replacement can temporarily retain
both old and new resources. Frame preflight still conservatively estimates
ordinary vertex/draw work, even with Compact selected.

`DesktopRunReport::last_screen_updates()` reports geometry preparation CPU time,
uploads and reuse separately from surface-frame metrics. Resource uploads can
precede a skipped surface frame. `renderer_description()` records the final
adapter, backend, PCI address, driver, size, DPI and present mode. Neither field
turns a CPU-only measurement into a native FPS claim.

Register `PresentationFeedback::default()` with `register_app_resource` to opt
into recovery from typed screen/world SceneBudget and composed FrameBudget
rejections. Read `AppRes<PresentationFeedback>` on the next update to reduce
presentation work, close the document or exit. No quotas are silently increased,
and no partial scene is displayed. Canonical updates continue, without rollback;
the application decides whether to pause. Feedback clears after an actual
successful presentation, not after an occluded/skipped attempt.

This does not recover invalid geometry, allocation failure, failing systems,
per-kind source-count limits, or an invalid initial World. Initial construction
must first produce a valid snapshot. Without the resource, desktop retains its
fail-fast policy. After rejection the compositor may keep the previous image,
but that image is not guaranteed through resize/loss and is never replayed as
if it belonged to a replacement World.

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

## Rounded cards, animated vectors, and clips

`ScreenRectangleVisual::rounded(position, size, color, radius)` adds logical-pixel
corners. Zero radius means square corners; large radii are clamped to half the
shorter side when drawn. `set_corner_radius` and `set_stroke(Some(Stroke::new(
width, color)))` change the shape without recreating an entity. Outlines are
centered; pointer hits test the rounded fill, not the decorative outside stroke.

`ScreenLineVisual::new(start, end, width, color)` and
`ScreenCircleVisual::new(center, radius, color)` are screen-owned components.
Change endpoints/center/radius directly in FrameUpdate. No `Transform2d`, fixed
tick, or World camera is involved; animation continues during simulation pause.
Lines have round caps. Circles also accept an optional outline.

All screen visuals, including images and text, accept `set_clip(ScreenClip)`.
Clips use the same top-left/downward logical pixels as hit testing. They stay
fixed when content moves or images rotate. To nest scopes, explicitly intersect
them and assign the result to their content:

```rust
use sim_logic::prelude::*;
fn content_clip() -> LogicResult<ScreenClip> {
    let panel = ScreenClip::new(
        LogicalScreenPosition::new(20.0, 20.0), LogicalScreenVector::new(400.0, 300.0),
    )?;
    let scroll_area = ScreenClip::new(
        LogicalScreenPosition::new(30.0, 70.0), LogicalScreenVector::new(380.0, 240.0),
    )?;
    Ok(panel.intersection(scroll_area))
}
```

Disjoint scopes produce `ScreenClip::Empty`, never an accidental unclipped
draw. Empty-clipped sources still count toward extraction limits. They emit no
draw geometry; image/text preparation may still retain their bounded resources.
There is no implicit clip tree or parent traversal. Recompute the assigned clip
when your layout changes. Pointer helpers respect each visual's explicit clip;
text metrics remain typographic metrics, not a widget hit box.

Contiguous rectangles, lines, circles and paths form one geometry run; images and
labels split it only where painter order requires it. Exhaustive `ScreenDraw`
matches need the new `Primitives` variant. Inspect either geometry-run variant
with `screen_primitive_run_records(run)`; the old rectangle accessor returns
only its rectangles. Source limits and the shared screen `SceneBudget` both
apply: a 1,000-line decoration needs an explicitly larger line count and scene
command/vertex/byte budget, not 1,000 separate desktop draw calls by design.

Set `RenderLimits::with_max_screen_lines` / `with_max_screen_circles` for large
decorative scenes (each defaults to 256). Rectangles, lines, and circles also
share `with_screen_scene_budget`; rounded/stroked shapes need more vertices
than square fills. Images/text split contiguous geometry runs, not one draw
per vector. `screen_primitives()` exposes the sorted sampled geometry.
`ScreenDraw::Primitives` is the new mixed-vector run variant; rectangle-only
applications retain the existing `Rectangles` variant. Same-source ties draw
rectangle, line, circle, path, image, then text. All remain above World content.

## Whole curves and outline-only circles

Use one `ScreenPolylineVisual` per open curve instead of spawning one line per
segment. Engine joins the segments and continues dash phase through the path;
only its two endpoints receive caps. A two-point path is also a styled line:

```rust
use sim_logic::prelude::*;

fn curve() -> LogicResult<ScreenPolylineVisual> {
    let points = [
        LogicalScreenPosition::new(10.25, 20.5),
        LogicalScreenPosition::new(50.25, 20.5),
        LogicalScreenPosition::new(70.25, 40.5),
    ];
    let style = StrokeStyle2d::new(2.0, Color::WHITE)
        .with_cap(StrokeCap2d::Butt)
        .with_join(StrokeJoin2d::Round);
    Ok(ScreenPolylineVisual::new(&points, style)?)
}
```

The component is approved automatically and needs no `Transform2d`. These are
logical screen positions: a Math editor projects its document points into this
space when its pan/zoom changes. Paths do not follow the World camera implicitly.
`set_points` and `set_style` validate through Engine before committing an edit.
Clones share immutable points; changing one does not mutate older snapshots.
Unchanged points skip copying. Width must use logical pixels, not world units.
Stroke colors, caps, joins, miter limits, dashes and markers use the exported
`StrokeStyle2d` types and Engine's validation rules.

`hit_test_centerline(pointer, radius)` is a selection helper, not a pixel test.
Its explicit logical radius includes dash gaps and endpoint disks, ignores
stroke width/markers/alpha, and respects the assigned clip and pointer viewport.
The application chooses which eligible path wins. Fractional coordinates need
no DPI multiplication: the desktop renderer handles the physical scale.

Defaults allow 256 path sources and 65,536 aggregate source points, including
empty-clipped paths. Disabled entities are excluded. Set
`with_max_screen_polylines` and `with_max_screen_polyline_points` on `RenderLimits`
for different workloads; `with_point_limit` sets a separate per-path ceiling.
The shared screen `SceneBudget` and composed `FrameLimits` still apply. A
10,000-point plot needs explicit vertex/byte/upload budgets in addition to its
point allowance; raising a point limit alone does not reserve or authorize those
resources. Shared paths count points per source occurrence for work limits.

One path is one Engine scene command, not a promise of one GPU draw for any
style. Prepared desktop runs reuse unchanged tessellation/uploads; Compact
routes paths and circle outlines through ordinary prepared geometry. CPU
extraction still rebuilds Engine scene commands each frame. Incremental CPU
extraction has **not** been implemented by adding this component.

`ScreenCircleVisual::outlined(center, radius, width, color)` creates a true
stroke-only circle, with no transparent fill disk. Its pointer test uses the
centered annulus. `fill_color()` returns `None`; `set_color` enables a fill.
Removing the only outline with `set_stroke(None)` is rejected until a fill is
enabled. Existing filled-circle constructors keep their behavior.

Closed arbitrary paths remain unsupported in Engine 0.4.2. Repeating the first
point at the end would produce two endpoint caps, not a joined seam, so Logic
rejects that spelling with `ClosedPathUnsupported`. Use the circle/rectangle
outline APIs for those shapes. This is not a polygon fill or closed-path API.

Migration: `ResolvedScreenPrimitive` now includes `Polyline` and is `Clone`,
not `Copy`, because paths share owned point storage. Add the variant to
exhaustive matches and use `.cloned()` instead of `.copied()` on record iterators.
Its ordering/source getters borrow the record; ordinary calls are unchanged.

## Keyboard focus

`KeyboardFocus<Target, Scope>::new(max_targets)` is an optional, allocation-free
controller. Supply the active modal scope and an ordered slice of unique,
eligible target IDs to `process(scope, eligible, FocusCommand::Next)` (or
Previous/First/Last/Activate/Clear). Activation is returned in `FocusOutcome`;
the helper never invokes a widget or reads/consumes input on its own.

Omit disabled/hidden targets. A new scope or removed target clears old focus;
Activate does not silently select a replacement. `set_focused` handles pointer
selection. Invalid/duplicate/over-limit orders preserve previous state. Call
`synchronize` after eligibility changes even when no navigation occurred.
Use generation-qualified IDs or recreate the helper on World replacement.

Run the combined proving UI:

```bash
cargo run --release --features text --example interface_lab
```

It exercises rounded cards, outlines, vector animation during pause, clips,
rotating images, shared-font sizes, pointer/keyboard activation, native hover
cursors, and F11. `-- --frames 180 --exercise-window` performs a bounded run
and checks actual presented-frame counts; it is not a pixel or FPS oracle.

## What this does not provide

There is no automatic widget layout, text editing, or implicit clipping tree.
Native pointer capture is a separate optional [input service](../guides/Pointer-Input.md).
[Optional font-backed labels](Text.md) are a separate drawing capability.
The button helper owns a local press/release sequence, not drag-motion sampling
or automatic global input routing. Drawing a rectangle alone does not make it
capture input. It belongs to the active
World and disappears on World replacement; it is not an application-owned
error overlay.

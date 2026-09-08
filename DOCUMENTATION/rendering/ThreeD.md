# Bounded geometric 3D

[Documentation](../README.md) / Rendering

`CuboidVisual3d` draws a solid box through Sim;Engine's retained mesh and
hardware depth-buffer path. A `View3d` World resource supplies its camera and
opaque background. This is real geometric 3D, not projected screen rectangles.
The initial bridge supports colored cuboids and optional outer-edge outlines,
not imported meshes, textures, lights, shadows, transparency, or 3D picking.

The components, camera values, and extracted records work without a window or
GPU. Desktop rendering requires the `desktop` feature. See the
[piano example](../../examples/piano_roll) for an application that switches
between a score view and a geometric instrument using shared application state.

## One cuboid

3D limits default to zero. Enable both the independent 3D limits and enough
final-frame capacity to compose its target:

```rust
use sim_logic::prelude::*;

fn main() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_frame_limits(
        FrameLimits::new(2, 1, 6, 4096, 32 * 1024 * 1024, 1),
    ));
    config.set_three_d_render_limits(ThreeDRenderLimits::new(
        1,             // visible cuboids
        12,            // filled triangles
        2048 * 2048,   // physical target pixels
    ));
    let mut app = Application::<u8>::new(config)?;
    let world_camera = ActiveCamera2d::centered(1.0)?;
    let mut view = View3d::new(Vec3::new(4.0, 3.0, 7.0)?, Vec3::ZERO)?;
    view.set_background(Color::rgb(0.02, 0.03, 0.05))?;
    let mut cuboid = CuboidVisual3d::new(
        Vec3::ZERO,
        Vec3::new(2.0, 1.0, 3.0)?,
        Color::rgb(0.25, 0.65, 0.5),
    )?;
    cuboid.set_wireframe(Some(WireframeStyle3d::visible(
        Color::BLACK,
        LogicalPixels::new(1.0)?,
    )?));
    let initial = app.register_world("cuboid", move |world| {
        world.spawn(world_camera)?;
        world.spawn(cuboid)?;
        world.insert_resource(view)?;
        Ok(())
    })?;

    let runner = app.build_headless(initial)?;
    let frame = runner.extracted_frame().ok_or("missing snapshot")?;
    let snapshot = frame.three_d().ok_or("missing 3D view")?;
    assert_eq!(snapshot.cuboids().len(), 1);
    assert_eq!(snapshot.cuboids()[0].visual(), cuboid);
    Ok(())
}
```

For desktop presentation, replace the headless build and inspection with
`app.run(initial)?;`. The two frame sources are the empty 2D World scene and
the composed 3D target. Its single composition draw is separate from the
cuboid's surface and wireframe draws. Larger windows, high-DPI displays, or
additional overlays may require larger explicit budgets.

The `ActiveCamera2d` is still required by World-scene extraction, even when
that scene is empty. Cuboids themselves require no `Transform2d` and are
standard approved components; no extra component approval is needed.

## Geometry, camera, and view switching

`CuboidVisual3d::new(center, size, color)` uses caller-defined world units.
Size is the full extent along each local axis. Every extent must be positive;
the transform must preserve finite, noncollapsed triangles. Surface colors
are normalized linear RGB with alpha exactly one. Invalid geometry or color
updates leave the previous component unchanged.

Use `set_transform(Transform3d::new(translation, rotation, scale)?)` to move,
rotate, or resize the unit cube. Scale gives its full local extents; the
transform applies scale, then rotation, then translation. `set_color` changes
its surface, and `set_wireframe` selects an already validated Engine edge
style or removes it with `None`.

FrameUpdate may modify cuboids and `View3d`. Extraction samples their current
values directly: there are no previous transforms or fixed-step interpolation
endpoints. Keep authoritative simulation or playback state separately and
derive presentation values from it.

`View3d::new(position, target)` uses positive-y up, a 60-degree vertical field
of view, and near/far distances of 0.1 and 1000 world units. `set_pose`,
`set_perspective(fov_radians, near, far)`, and `set_background` validate updates
atomically. Near and far use `WorldLength`; the camera requires a nondegenerate
look-at direction that is not parallel to its up axis.

`view.camera(logical_viewport)` derives the perspective aspect ratio from the
current viewport. This does not automatically move the camera to fit objects.
Use `FrameViewport` in FrameUpdate for application-specific resize decisions.

`view.set_enabled(false)` suppresses the whole 3D snapshot without deleting
cuboids. A missing view behaves the same way. Neither case visits cuboid
sources or submits a 3D prepass. `cuboid.set_visible(false)` suppresses just
that object; ECS `Disabled` also excludes the entity. Hidden and disabled
objects consume no extracted cuboid or triangle allowance. An enabled view
with no visible cuboids still renders its background and needs target capacity.

## Extraction and composition

`ExtractedFrame::three_d()` returns an optional borrowed `ThreeDSnapshot`.
Its `view()` and `cuboids()` expose the validated CPU presentation state.
`ExtractedFrame::resolved_cuboids()` is empty when 3D is suppressed. Each
`ResolvedCuboid3d` retains its managed `source()`, `visual()`, `transform()`,
and `color()`. Source identities must belong to the extracted World generation.

The complete 3D snapshot is staged with the 2D and screen buffers before any
new frame is published. Cuboids are submitted in stable managed-identity
order, but actual depth determines surface visibility. Do not rely on exact
coplanar surfaces having a useful visual winner.

Desktop composition is: 2D World, then the opaque full-viewport 3D target,
then ordered screen rectangles and images. The target replaces the World
pixels beneath it; 2D World objects do not share its depth buffer. Screen
overlays retain their existing mixed layer/depth/entity ordering and remain
above all 3D geometry.

The adapter retains one immutable cube mesh, bounded reusable scene slots,
and one color/depth target. Ordinary transform and style changes reuse mesh
topology. Target dimensions follow the renderer's physical extent and logical
viewport. Resize or scale changes release the obsolete target before creating
its replacement. A changed background rebuilds scene metadata because the
pinned Engine scene background is immutable. Unchanged backgrounds reuse it.
The adapter caches each slot's last applied transform, style, and visibility,
so unchanged fields do not call Engine setters. Changed fields still use
Engine's linear object-ID lookup; this does not promise constant-time scene
updates when many objects change together.

## Two independent budgets

| Limit | Meaning |
| --- | --- |
| `ThreeDRenderLimits::max_cuboids()` | Visible extracted cuboids and the retained scene-slot count. |
| `ThreeDRenderLimits::max_triangles()` | Twelve filled triangles for every visible cuboid. |
| `ThreeDRenderLimits::max_target_pixels()` | Physical width times height of one color/depth target pair. |
| `FrameLimits` | Final composition, including the 3D color target and other World/screen sources. |

Set 3D limits with `AppConfig::set_three_d_render_limits`, or with
`RenderLimits::with_three_d_render_limits`. `RenderLimits::three_d()` reads
them. These values are frozen before the application starts. The AppConfig
setter preserves other rendering limits; subsequently replacing all
`RenderLimits` replaces its 3D limits too.

The separate offscreen prepass uploads retained mesh/instance state and draws
the cuboids with depth testing. Each cuboid has twelve surface triangles and
twelve outer edges; selecting both visible and hidden wireframe modes can
submit twenty-four edge segments. This work is not included in the final
frame's composition budget or timings.

Composing the resulting target adds one pass, one command, six vertices, one
draw, a compositor uniform upload, and the color target's texture bytes.
Color bytes depend on the renderer's surface format. The depth attachment
adds four bytes per physical pixel outside that frame texture accounting.
Retained mesh buffers, recovery descriptions, and scene metadata are also
separate from the color texture allowance. These limits are not a total
process-memory cap.

The adapter checks known composition counts and exact color-target bytes
before target preparation. Engine still checks its private uniform costs,
geometry, device capacities, and final composition. Headless extraction has
no physical target or GPU and cannot validate those desktop costs.

`DesktopRunReport::last_three_d_frame()` exposes the successful
`Mesh3dRenderReport` retained for the latest presentation attempt, separately
from `last_render_frame()`. It includes object, triangle, edge, pass, and draw
counts plus upload and encode/submit timings. A successful prepass can precede
a skipped surface frame; an attempt without an enabled 3D view records no
prepass report. These reports do not establish allocation-free GPU frames.

## Failures, recovery, and camera boundaries

Invalid values return `CuboidVisualError` or `View3dError`. Extraction failures
are wrapped in `ExtractionError::ThreeD`; the old complete published CPU
snapshot remains intact. A candidate World failing extraction never becomes
active. As with other extraction failures, the desktop host does not present
that stale snapshot: it attempts a clear-only diagnostic frame and stops.

`DesktopRunError::ThreeDPreparation` preserves the concrete preparation or
prepass error and the logical frame that already completed. Object insertion,
transform, and style failures retain their managed source. Engine's whole-scene
portability failures do not currently identify an individual object.
Composition failures remain presentation errors. Cache uploads, allocations,
or a successful prepass may have happened before a later failure; no partially
composed surface frame is presented, and canonical state is not rolled back.

After successful renderer recovery or renderer replacement, the adapter
invalidates its mesh, scene, and target cache. The next enabled 3D presentation
recreates them from the built-in topology and current CPU snapshot. Recovery
does not require replacing the World or replaying gameplay/audio events.

The pinned Sim;Engine 0.2 filled-mesh path has a deliberate limitation: each
surface triangle must be provably entirely inside the camera frustum, or
entirely outside one common clipping plane. A triangle crossing a plane, or
one whose projected orientation is numerically ambiguous, can fail with
`Mesh3dRenderError::UnportableSurfaceTopology`. Explicit display edges have a
separate clipper, but enabling outlines does not remove the filled surfaces
or their restriction.

Keep visible solid cuboids comfortably inside every frustum plane, including
near and far, and use an oblique camera that avoids nearly edge-on filled
faces. Check resized, very tall, and very wide viewports. `cuboid.corners()`
and `view.camera(viewport)?.project_world(corner, viewport)` support CPU fit
checks; their `inside_view()` results are useful but do not prove Engine's
stricter portable-shader validation. Verify the intended desktop camera and
viewport range with the actual renderer as well.

The [headless acceptance tests](../../tests/three_d.rs) cover visibility, shared
identity across view switches, current-value transforms, atomic failure, and
World replacement. They do not claim GPU clipping support or rendered output.

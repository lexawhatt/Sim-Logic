# Bounded geometric 3D

[Documentation](../README.md) / Rendering

`CuboidVisual3d` draws a solid box through Sim;Engine's retained mesh and
hardware depth-buffer path. A `View3d` World resource supplies its camera and
background. This is real geometric 3D, not projected screen rectangles.
The bridge pins the official Engine `0.4.0` release from crates.io.
Cuboids keep their simple opaque/outline API.
Host-built meshes support vertex colors, UVs, normals, Opaque/Mask/Blend
materials, textures and explicit lighting/fog. Mesh import, normal generation,
shadows, point lights and 3D picking remain outside this bridge.

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
        12,            // total filled triangles, including clipped geometry
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

## Host-built meshes and editable chunks

`MeshAsset3d` wraps Engine's immutable, shared CPU `Mesh3d`. `MeshVisual3d`
adds one transform, surface policy, optional texture, and visibility. Both work headlessly; no
renderer handle belongs in an ECS component. `MeshVisual3d` is already approved.

```rust
use sim_logic::prelude::*;
use sim_engine::Mesh3d;

fn triangle() -> LogicResult<MeshVisual3d> {
    let topology = Mesh3d::new(
        vec![Vec3::ZERO, Vec3::new(1.0, 0.0, 0.0)?, Vec3::new(0.0, 1.0, 0.0)?],
        vec![0, 1, 2],
    )?;
    let asset = MeshAsset3d::new(topology)?;
    Ok(MeshVisual3d::new(asset, Transform3d::IDENTITY, Color::WHITE)?)
}
```

Enable meshes explicitly, for example:
`ThreeDRenderLimits::new(8, 100_000, 2048 * 2048).with_mesh_limits(256, 16 * 1024 * 1024)`.
That allows eight cuboids, 256 mesh instances, and 16 MiB of source topology
capacity summed per visible mesh instance. The triangle allowance is shared
by cubes and meshes, including clipping headroom.

Asset clones share Engine's existing allocation; unchanged assets reuse one
desktop upload even when several entities use them. Rebuilding identical
contents independently creates a different asset identity. `shares_storage`
tests sharing without comparing all vertices. Construct a new asset only for
changed geometry, then call `visual.set_asset(new_asset)?`. Transform and asset
setters validate transformed geometry before changing the component; color
changes do not scan topology. Extraction clones shared handles, not vertex
arrays. Engine `Mesh3d::with_attributes` supplies UVs, vertex colors and normals;
Logic preserves them without copying. Edge-only topology returns
`UnsupportedTopology`; display edges on a surface mesh are retained but not
drawn by this surface bridge.

For a voxel application, generate only exposed faces and group geometry by
chunk and surface color. Terrain rules, dirty-chunk selection, meshing, picking,
and saves belong to the application. The bridge does not introduce voxel rules
or a second renderer. An empty chunk needs no mesh component/entity: Engine
topology requires nonempty geometry.

The desktop cache retains CPU identities, not extra GPU clones. The scene owns
its GPU meshes. A changed asset updates the existing scene object; a first
divergent edit detaches shared buffers, and later fitting edits can reuse them.
Removal releases obsolete references; unchanged fields avoid Engine setters.
Mesh slots follow full managed source identity, not a snapshot index;
removing an early chunk does not reinsert the surviving suffix.
Disabling/removing the whole `View3d` releases the retained 3D scene,
assets, and target, so enabling it again requires uploads. In-flight GPU work
may outlive released host references. Published and staging CPU snapshots may
each retain one bounded source set; assets held separately by application code
are not included in these rendering allowances.

## Materials, lighting and textures

`MeshVisual3d::new(asset, transform, color)` retains the old Opaque, TwoSided,
Unlit, no-fog defaults. Use `with_surface(asset, transform, surface)` or
`set_surface(surface)` to select Engine's validated `SurfaceStyle3d`:

- `opaque(color)` ignores vertex/texture alpha and writes depth.
- `mask(color, cutoff)` discards fragments below the combined-alpha threshold;
  surviving fragments write opaque color and depth. Equality survives.
- `blend(color)` tests existing depth, does not write depth, and draws after
  opaque/masked surfaces. Engine sorts whole objects back-to-front, not
  triangles within one object or intersecting surfaces.

`surface.with_sidedness(SurfaceSidedness3d::FrontOnly)` keeps projected-CCW
front faces; the default renders both sides. Looking from inside a one-sided
box can therefore hide its faces. `MeshVisual3d::set_color` preserves the
selected alpha mode, mask threshold, lighting, sidedness and fog flag.

`surface.with_lighting(SurfaceLighting3d::Lambert)` requires host-supplied
model normals. Logic rejects missing normals at construction or a setter,
without changing the old component. Engine owns inverse-transpose transport,
nonuniform-scale validation, back-face normal reversal and fragment shading.
It does not infer face normals from triangle winding. Set `view.set_lighting`
with Engine's `Lighting3d`: ambient plus at most one directional light.
Unlit surfaces ignore that illumination.

`view.set_fog(Some(Fog3d::new(color, start, density)?))` sets distance fog, but
each participating surface must also use `with_fog(true)`. Fog follows
camera-forward world distance in either projection, not radial distance. It
changes RGB after illumination, never alpha, depth or logical visibility.

`TextureAsset3d::rgba8(width, height, pixels, max_source_bytes)` accepts owned
tightly packed top-left-origin sRGB RGBA8 pixels with straight alpha. The
inclusive limit checks retained Vec capacity, not only its length. Clones
share an immutable CPU snapshot. Device limits remain Engine's responsibility.

```rust
use sim_engine::{TextureAddressMode3d, TextureUvTransform3d};
use sim_logic::{prelude::*, screen::ImageFilter, three_d::{TextureAsset3d, TextureVisual3d}};

fn block_texture() -> LogicResult<TextureVisual3d> {
    let pixels = vec![255; 4 * 4 * 4];
    let asset = TextureAsset3d::rgba8(4, 4, pixels, 64)?;
    Ok(TextureVisual3d::new(asset)
        .with_mipmaps(true)
        .with_filter(ImageFilter::Nearest)
        .with_address_mode(TextureAddressMode3d::Repeat)
        .with_uv_transform(TextureUvTransform3d::new(
            Vec2::new(4.0, 2.0), Vec2::ZERO,
        )?))
}
```

Attach with `visual.set_texture(Some(texture))?`. Meshes must already carry
one UV per vertex; invalid attachment or replacement leaves the old component
unchanged. Removing a texture does not remove source UVs. Texture tint, surface
tint, vertex color and sampled texture color multiply in linear color space.
Engine controls alpha coverage according to the surface mode.

Mipmaps are generated by Engine, not Logic. For atlas assets, isolate a tile
into its own image before repeating it or generating mipmaps. A packed atlas
image is not automatically interpreted as isolated tiles. Ordinary mip
averaging does not preserve Mask coverage and adds no anisotropic filtering.

`asset.with_region_update(region, row_stride, pixels, max_source_bytes)?`
returns a new complete CPU snapshot. Region coordinates use the existing
`ImageRegion` texel type; the byte slice has exactly
`(height - 1) * row_stride + width * 4` bytes, without final-row padding.
The method allocates a new bounded base-level image and preserves every old
alias. The limit applies to the new snapshot, not all snapshots the application
still holds. Only immediate-parent identity and the dirty rectangle are kept;
there is no unbounded edit-history chain. Install an edited image with
`texture.with_asset(revised_asset)` and `visual.set_texture`.

`TextureVisual3d::texel_bytes()` reports nominal RGBA8 bytes for the complete
mip chain. GPU region updates upload mip zero's dirty rectangle but regenerate
and upload lower levels in full; a small patch is not necessarily a small
total upload. Revision identity is process-local, never a persistent asset key.

The bridge patches only a direct parent revision with the same mip policy;
skipped revisions use a full replacement. Engine's final scene budgets and
finite old/new overlap ceilings apply independently. Removing or rebinding a
material retains geometry and attributes, without a geometry upload. A new
texture can still need its own allocation/upload. Plain and textured instances
of the same asset can share geometry independently of their materials.
Changing `View3d`'s background updates only the clear color, preserving object
IDs, mesh capacity and texture revisions.

## Desktop measurements and recovery

`DesktopConfig::set_gpu_timing(true)` opts into Engine's bounded asynchronous
GPU queries. It is off by default. After the loop, `report.gpu_timings()` exposes
availability, losses, pending queries and the latest completed samples. Samples
retain source, query ID, logical frame and device generation; completion order
is not frame order. No waiting or unbounded history is added to the game loop.

`report.three_d_updates()` totals successful scene-owned mesh revisions and
texture patches, with their upload bytes and new GPU allocations. It excludes
initial creation, material-only replacements and composition uploads. The
last preparation is available separately through `last_three_d_updates()`.
These counters are not whole-frame allocation or universal FPS measurements.

Device recovery restores whole scenes, keeping object IDs, reserved geometry
capacity, texture revisions and material settings. Targets are recreated for
the new device. Pending timing associations are cleared, not reassigned to
new-device frames. Engine 0.4.0 includes the fix for the earlier standalone
textured `restore_mesh3d` material regression. The manual consumer test passes
on NVIDIA/Vulkan with the original assertions; this is not Intel qualification.
The bridge keeps whole-scene recovery for shared-resource deduplication.

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
`set_perspective(fov_radians, near, far)`,
`set_orthographic(vertical_span, near, far)`, and `set_background` validate updates
atomically. Near and far use `WorldLength`; the camera requires a nondegenerate
look-at direction that is not parallel to its up axis.

The background accepts normalized straight-linear RGBA, including transparent.
Engine stores premultiplied color in its offscreen target; do not premultiply
the input color a second time. `orthographic_span()` returns None in perspective
mode. Both projections derive aspect from the current logical viewport.

`view.set_surface_policy(ThreeDSurfacePolicy::Native)` explicitly selects
Engine's hardware-filled-surface path for ordinary free-camera games. The
default `StrictPortable` retains conservative cross-backend clipping and
orientation proofs. Mathematical display edges retain independent strict
validation under either policy.

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
Its `view()`, `cuboids()`, and `meshes()` expose the validated CPU presentation
state. `source_triangle_count()` counts original triangles before clipping.
`ExtractedFrame::resolved_cuboids()` and `resolved_meshes()` are empty when 3D
is suppressed. Each
`ResolvedCuboid3d` retains its managed `source()`, `visual()`, `transform()`,
and `color()`. Source identities must belong to the extracted World generation.

The complete 3D snapshot is staged with the 2D and screen buffers before any
new frame is published. Each CPU record group uses stable managed-identity
order, but actual depth determines surface visibility. Retained Engine object
order may differ after mesh edits or new insertions. Do not rely on exact
coplanar surfaces having a useful visual winner.

Desktop composition is: 2D World, then the full-viewport 3D target,
then ordered screen rectangles and images. Opaque target pixels cover the
World beneath them; transparent target pixels compose over it. 2D World objects
do not share its depth buffer. Screen
overlays retain their existing mixed layer/depth/entity ordering and remain
above all 3D geometry.

The adapter retains one immutable cube mesh, current custom mesh assets,
bounded scene slots, and one color/depth target. Ordinary transform and style changes reuse mesh
topology. Target dimensions follow the renderer's physical extent and logical
viewport. Resize or scale changes clear Engine's cached composition bindings
and release the obsolete target before creating its replacement. This drops
the host's retained references; it does not force the GPU driver to retire
already submitted work immediately. Background changes update the retained
scene in place without rebuilding its metadata or resources.

The adapter caches each slot's last applied transform, style, and visibility,
so unchanged fields do not call Engine setters. Engine 0.3 uses indexed object
IDs for changed fields. This removes the former linear ID search, but does
not claim that an entire scene update or GPU frame is constant-time. A slot's
managed source is refreshed even when a different entity has identical geometry.

## Independent rendering and cache budgets

| Limit | Meaning |
| --- | --- |
| `ThreeDRenderLimits::max_cuboids()` | Visible extracted cuboids and the retained scene-slot count. |
| `ThreeDRenderLimits::max_meshes()` | Visible custom-mesh instances; zero until explicitly enabled. |
| `ThreeDRenderLimits::max_mesh_source_bytes()` | Engine topology capacities summed per visible mesh instance, including shared copies. |
| `ThreeDRenderLimits::max_texture_source_bytes()` | Base-level pixel capacities summed per visible textured mesh, including shared copies. |
| `ThreeDRenderLimits::max_texture_gpu_bytes()` | Nominal complete-chain RGBA8 texture bytes summed per visible textured mesh. |
| `ThreeDRenderLimits::max_triangles()` | Source triangles at extraction and total submitted surface triangles at presentation. Each source cuboid starts with twelve. |
| `ThreeDRenderLimits::max_target_pixels()` | Physical width times height of one color/depth target pair. |
| `FrameLimits` | Final composition, including the 3D color target and other World/screen sources. |
| `DesktopConfig::frame_cache_budget()` | Engine's independently bounded idle composition storage and retained bindings. |

Set 3D limits with `AppConfig::set_three_d_render_limits`, or with
`RenderLimits::with_three_d_render_limits`. `RenderLimits::three_d()` reads
them. These values are frozen before the application starts. The AppConfig
setter preserves other rendering limits; subsequently replacing all
`RenderLimits` replaces its 3D limits too.

Engine's strict path can clip filled triangles at all six camera boundaries,
creating more triangles than the source contained. The adapter passes the
combined surface-triangle ceiling directly to Engine's authoritative preflight;
it no longer estimates headroom from the smallest object. Leave explicit
triangle headroom for strict moving cameras. Native submits source triangles
to hardware clipping and reports CPU clipped/discarded counts as None, not
zero. Submission counts do not reveal hardware visibility. No duplicate
clipping validator lives in Logic.

The retained Engine scene receives the sum of cuboid and mesh ceilings and
Engine's finite default scene-storage and mesh-byte ceilings. Custom uploads
are bounded by remaining aggregate scene CPU/GPU mesh capacity before upload;
Engine's separate finite staging limit also applies. Enable textures with
`limits.with_texture_limits(base_source_bytes, full_mip_texel_bytes)`; both
defaults are zero. Hidden/disabled meshes do not consume these allowances.
Engine's separate CPU mip-storage and transient-update limits still apply.
Engine's default generated
vertex, triangle, and upload-byte ceilings remain upper bounds. Author limits
do not bypass these Engine or device limits. Zero cuboid and mesh limits still
permit only a background, despite Engine requiring a nonzero internal scene
object budget.

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

Use `DesktopConfig::set_frame_cache_budget(FrameCacheBudget)` to set the
separate Engine idle-cache limits; `frame_cache_budget()` reads them. Defaults
are Engine's defaults. This controls reusable composition storage and bindings,
not the cube mesh, scene, depth target, or canonical application state. Idle
cache limits do not enlarge `FrameLimits` or the 3D allowances. Steady-state
frames keep their cache; target replacement and renderer recovery invalidate
the affected retained resources.

The adapter checks known composition counts and exact color-target bytes
before target preparation. Engine still checks its private uniform costs,
geometry, device capacities, and final composition. Headless extraction has
no physical target or GPU and cannot validate those desktop costs.

`DesktopRunReport::last_three_d_frame()` exposes the successful
`Mesh3dRenderReport` retained for the latest presentation attempt, separately
from `last_render_frame()`. It includes object, triangle, edge, pass, and draw
counts plus upload and encode/submit timings. Its `preflight()` report includes
generated triangles, vertices, upload bytes, and clipped/discarded source
triangle counts. A successful prepass can precede a skipped surface frame;
an attempt without an enabled 3D view records no
prepass report. These reports do not establish allocation-free GPU frames.

## Failures, recovery, and camera boundaries

Invalid values return `CuboidVisualError`, `MeshVisualError`, or `View3dError`. Extraction failures
are wrapped in `ExtractionError::ThreeD`; the old complete published CPU
snapshot remains intact. A candidate World failing extraction never becomes
active. As with other extraction failures, the desktop host does not present
that stale snapshot: it attempts a clear-only diagnostic frame and stops.

`DesktopRunError::ThreeDPreparation` preserves the concrete preparation or
prepass error and the logical frame that already completed. Object insertion,
transform, and style failures retain their managed source. Instance-local
Engine preflight failures become `DesktopThreeDError::ObjectRender`, preserving
both the current `LogicEntity` and the complete Engine object error. Reusing a
scene slot after deletion or World replacement does not retain an old entity's
attribution. Camera, target, resource-ownership, and aggregate-capacity failures
remain `DesktopThreeDError::Render` without an invented source. An unrecognized
Engine object handle also preserves its original error without guessing.
Composition failures remain presentation errors. Cache uploads, allocations,
or a successful prepass may have happened before a later failure; no partially
composed surface frame is presented, and canonical state is not rolled back.

Successful device recovery restores the retained scene and its mesh resources;
the target is recreated on the next enabled 3D presentation. A newly created
desktop renderer starts with empty caches. Neither route replaces the World or
replays gameplay/audio events.

In StrictPortable mode, numerically ambiguous grazing or nearly edge-on cases
can still return an object-attributed portability error. Native avoids that
strict filled-surface orientation requirement, but still rejects unsupported
shader arithmetic. Neither mode disables Engine's independent edge proofs.
Object-local failures now include the original triangle or vertex index and
the detailed reason, when applicable.

Check resized, very tall, and very wide viewports, and allow triangle headroom
for clipped surfaces. An oblique camera can avoid nearly edge-on filled faces.
`cuboid.corners()` and `view.camera(viewport)?.project_world(corner, viewport)` support CPU fit
checks; their `inside_view()` results are useful but do not prove Engine's
stricter portable-shader validation. Verify the intended desktop camera and
viewport range with the actual renderer as well.

The [0.4 CPU tests](../../tests/dev4_visuals.rs) cover surface defaults,
atomic attribute requirements, immutable strided texture revisions, mip byte
counts and exact extraction limits. They do not claim rendered pixel evidence.
The [mesh tests](../../tests/mesh_three_d.rs) cover immutable sharing, geometry
rejection, capacity-aware budgets, revisions and World replacement.
The [headless acceptance tests](../../tests/three_d.rs) cover visibility, shared
identity across view switches, current-value transforms, atomic failure, and
World replacement. The [migration unit tests](../../src/platform/desktop/three_d/migration_tests.rs)
cover budget arithmetic, source refresh, scene-wide errors, and target-cache
invalidation policy without a GPU. These tests do not establish rendered
clipping output or a performance improvement. Native/GPU integration results
must be measured independently on the exact pinned candidate.

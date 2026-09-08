# Screen images and immutable assets

Register an image once, then place it in one or more Worlds with
`ScreenImageVisual`. The Application owns its pixels across World replacement.
Each visual holds a small image handle, destination geometry, optional source
region, tint, and sampling choice. This works headlessly without a window or
GPU; the desktop host prepares the GPU image when it is first needed.

This API accepts already decoded pixels. File loading, PNG/JPEG decoding,
runtime asset replacement, text layout, font loading, and audio are not part
of this slice.

The optional [Ferris helper](Easter-Eggs.md) decodes only its built-in PNG;
it does not add a general-purpose file loader to this API.

Try the desktop board with `cargo run --release --example image_board`.
WASD or arrows move the partially offscreen image, Space toggles its source
region, Enter opens an alternate World, and Escape exits. Both Worlds use the
same registered pixels. The [example source](../examples/image_board/game.rs)
is also exercised by [headless acceptance tests](../tests/screen_images/example.rs).

## A complete headless image

Screen-image extraction is disabled by default. Enable its source count and
set the presentation budget explicitly; registering pixels alone does not
change those limits.

This program registers a two-by-two image, selects its right column, and
checks the extracted result without creating a renderer:

```rust
use std::time::Duration;
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

fn main() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_image_asset_limits(ImageAssetLimits::new(1, 2, 2, 16));
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(1)
            .with_frame_limits(FrameLimits::new(2, 1, 12, 4096, 16, 1)),
    );
    let mut app = Application::<Action>::new(config)?;
    let pixels = [
        255, 0, 0, 255,   0, 255, 0, 255,
        0, 0, 255, 255,   255, 255, 255, 128,
    ];
    let image = app.register_image_rgba8(2, 2, &pixels)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let mut visual = ScreenImageVisual::new(
        image,
        LogicalScreenPosition::new(-8.0, 24.0),
        LogicalScreenVector::new(64.0, 128.0),
    )?;
    visual.set_source_region(Some(ImageRegion::new(1, 0, 1, 2)?))?;
    visual.set_tint(Color::rgba(1.0, 1.0, 1.0, 0.75))?;

    let initial = app.register_world("image", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let report = match runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport)) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    let frame = runner.extracted_frame().ok_or("snapshot is missing")?;
    let drawn = frame.resolved_screen_images().first().ok_or("image is missing")?;
    assert_eq!(drawn.image(), image);
    assert_eq!(drawn.source_region(), Some(ImageRegion::new(1, 0, 1, 2)?));
    assert_eq!(runner.image_asset_count(), 1);
    assert_eq!(runner.image_asset(image).ok_or("asset is missing")?.pixels(), pixels);
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    Ok(())
}
```

Use the [getting-started dependency setup](Getting-Started.md) to run this in
a separate project with `default-features = false`. For desktop presentation,
enable the default feature and replace the headless driver with
`app.run(initial)?;`. The camera is still required to validate the World scene,
even when all visible content uses screen coordinates.

The sample's `FrameLimits` allows two sources (the World scene and one image),
one image command/draw, twelve vertices, 4096 upload bytes, and sixteen
referenced texture bytes. These limits describe this tiny example. Add space
for additional content rather than assuming the defaults will grow with it.

## Pixels, ownership, and source regions

`register_image_rgba8(width, height, pixels)` takes exactly
`width * height * 4` bytes. Rows run from top to bottom, and each row runs from
left to right. RGB bytes use sRGB encoding; alpha is straight, so RGB has not
already been multiplied by alpha. The registration copies the slice into
Application-owned storage. Later changes to the caller's buffer have no effect.

Images are immutable after registration. Equal registrations produce distinct
handles; reusing one `ImageAssetId` shares the same asset among visuals and
World factories. Handles belong to their issuing Application and are not file
paths or persistent save identifiers. An enabled visual using another
Application's image fails candidate preparation. A live enabled invalid image
reference causes extraction to keep the last complete World-plus-screen
snapshot. Disabled entities are not inspected; enabling one makes its image
subject to validation on the next extraction.

`ImageRegion::new(x, y, width, height)` selects exact source texels. Its extents
must be positive and its bounds must fit the registered image.
`set_source_region(None)` restores the full image. Changing the source does
not change destination size. `set_image` keeps the current region and rejects
a smaller replacement if that region would no longer fit; the old visual
remains intact on failure.

Sampling defaults to `ImageFilter::Nearest`. Use `set_filter(ImageFilter::Linear)`
to blend neighboring texels while scaling. Tint defaults to white and uses
normalized straight-linear `Color`, multiplying the sampled color. The image
bytes and tint therefore use different RGB encodings intentionally. Invalid
geometry, tint, region, or draw-order depth leaves the previous value intact.

## Placement and ordering

Position is the destination's top-left corner in logical screen pixels, with
x increasing rightward and y downward. Size is a positive logical width and
height. Finite negative and offscreen positions are valid; desktop drawing
clips them while retaining the correct source mapping. World camera motion
does not move the image, and no `Transform2d` is required.

FrameUpdate may change image geometry directly and use `FrameViewport` for
layout during resize or pause, just like [screen panels](Screen-HUD.md).
The visual supplies no automatic hit testing, input capture, or layout tree.
Disabled entities do not contribute extracted images.

Images and rectangles share layer, finite draw-order depth, and stable entity
identity ordering. Lower layer/depth draws first; a rectangle precedes an
image when both belong to the same entity with exactly equal ordering values.
All screen content follows World content. The desktop host preserves mixed
rectangle/image order instead of grouping all images at the end.

Each image placement currently uses one compositor source. Contiguous
rectangle groups between images use their own sources. This is a bounded
image-placement API, not a large sprite or text batching system. Sharing an
asset avoids duplicate image storage but does not combine its placements
into one draw. Applications with no images keep the existing screen-scene path.

## Limits and memory

| Setting | What it bounds |
| --- | --- |
| `ImageAssetLimits` | Registered image count, each image's dimensions, and total retained CPU pixel capacity. |
| `RenderLimits::with_max_screen_images` | Enabled image visuals inspected during extraction; the default is zero. |
| `FrameLimits` | Composed sources, commands, vertices, uploads, referenced texture bytes, and draw calls. |

Default asset limits allow 64 images, each at most 4096 by 4096 texels, with
64 MiB total retained pixel capacity. Zero limits are valid. Registration
checks dimensions, overflow, exact byte length, count, and retained capacity
before publishing a new entry. Registry metadata is separately bounded by
entry count. The caller's spare Vec capacity is not retained or charged.

`HeadlessRunner::image_pixel_bytes()` reports retained registry pixel capacity;
`image_asset_count()` includes registered images that are not currently drawn.
`image_asset(id)` provides read-only dimensions and pixels, or `None` for a
foreign handle.

Desktop preparation retains an additional Engine CPU recovery copy and a GPU
texture for each prepared image. The registry's pixel limit is therefore not
a limit on total process memory or GPU memory. Frame texture accounting covers
the full registered images referenced by that frame, even when a visual draws
only a source region. Scene budgets still apply to their individual scenes.

Mixed ordering retains one aggregate rectangle scene for validation and
separate scenes for its contiguous rectangle runs. Run storage is reused while
the partition lengths stay unchanged. Changed lengths replace old storage,
and a failed composition drops private run storage before retrying. This
keeps historical run peaks from accumulating across changed orders; it does
not promise allocation-free repartitioning or an exact Vec capacity.

The desktop cache reuses unchanged images and is cleared when its renderer is
replaced or recovered; the Application's CPU asset remains available for
preparation again. An upload failure is reported with its source image.
`DesktopRunError::ImagePreparation` includes that error and the completed
logic-frame report. Composition-budget errors remain presentation errors.
Earlier successful cache uploads may remain if a later presentation step
fails, but the host does not present a partially composed frame.

Headless extraction checks CPU state and ordering. Screenshots can show a
particular rendered result; neither a screenshot nor a headless test proves
allocation-free GPU presentation or a frame-rate guarantee.

The [CPU allocation fixture](../benches/headless_runtime/images.rs) measures
64 images sharing one asset and 128 rectangles in stable mixed order. It
changes positions and colors, checks the result every frame, and requires
zero allocator calls across 100 frames after warm-up. Run it with
`cargo bench --no-default-features --bench headless_runtime -- --images-only`.
This measures the headless path, not uploads, rendering, or changed partitions.

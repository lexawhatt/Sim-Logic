# Screen text

[Documentation](../README.md) / Rendering

Register a font once, then put a `ScreenTextVisual` on an entity. Sim;Logic
owns its application identity, label values, ordering, and desktop cache.
Sim;Engine shapes the text, rasterizes glyphs, and renders it. There is no
second font renderer hidden in Logic.

Enable the optional `text` feature:

```bash
cargo run --release --features text --example text_labels
```

The [example](../../examples/text_labels/main.rs) includes a licensed DejaVu
font. The library does not discover, download, or bundle a default font into
your application. Supply trusted TTF/OTF bytes and follow that font's license.

## A complete headless label

This program reads a font supplied by the caller, prepares a Cyrillic label,
and inspects the extracted result without creating a window or GPU. Run it
with `--no-default-features --features text` to omit the desktop host.
The label `text` feature includes GPU atlas configuration types, so this
headless label build still compiles GPU support; it does not initialize it.
For CPU font loading/shaping/rasterization primitives alone, use
`--no-default-features --features fonts`. That separate feature does not
enable labels, desktop hosting or GPU dependencies.

```rust
use std::time::Duration;
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

fn main() -> LogicResult {
    let font_path = std::env::args().nth(1).ok_or("pass a trusted TTF/OTF path")?;
    let mut config = AppConfig::default();
    config.set_text_limits(TextLimits::new(1, 8 * 1024 * 1024));
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_texts(1)
            .with_max_screen_text_bytes(256)
            .with_max_screen_text_glyphs(64)
            .with_frame_limits(FrameLimits::new(
                2, 64, 64 * 6, 64 * 1024, 4 * 1024 * 1024, 2,
            )),
    );
    let mut app = Application::<Action>::new(config)?;
    let font = app.register_font(std::fs::read(font_path)?, TextSettings::new(24.0)?)?;
    let mut label = ScreenTextVisual::new(
        font,
        "Привет, мир!",
        LogicalScreenPosition::new(400.0, 64.0),
    )?;
    label.set_alignment(TextAlignment::Center)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("text", move |world| {
        world.spawn(camera)?;
        world.spawn(label.clone())?;
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
    let text = frame.resolved_screen_texts().first().ok_or("label is missing")?;
    assert_eq!(text.text(), "Привет, мир!");
    assert!(text.metrics().advance() > 0.0);
    assert_eq!(runner.text_font_count(), 1);
    Ok(())
}
```

For a desktop application, retain the default feature and use
`app.run(initial)?` instead of the headless driver. The World camera is still
required even when all visible content is in screen coordinates. Text sources
are disabled by default; registering a font does not silently enlarge source
or frame limits.

## Coordinates, changes, and ordering

`position` is the label's **baseline anchor**, not its top-left corner. Screen
x increases to the right and y increases downward, in logical pixels. Moving
the World camera does not move the label. Negative/offscreen coordinates are
allowed if their arithmetic remains representable; the renderer clips them.

Alignment uses typographic advance, including trailing spaces. `Left` starts
at the anchor, `Center` places half the advance on each side, and `Right` ends
at the anchor. `baseline_origin()` reports the resulting drawing origin.
Metrics describe the font's advance, ascent, descent, and line spacing, not
exact visible-pixel bounds. A glyph can extend beyond its advance.

Use `set_text`, `set_font`, `set_position`, `set_alignment`, `set_tint`,
`set_layer`, and `set_draw_order_depth` to change a label. Tint starts white and
uses normalized straight-linear RGBA. Invalid text, budgets, or geometry leave
the entire previous visual unchanged. Equal UTF-8 text is a no-op. A clone
shares immutable storage; changing one clone does not change another.

Text joins [panels](Screen-HUD.md) and [images](Screen-Images.md) in the same
screen draw plan. Lower layer/depth draws first; stable entity identity breaks
ties. On the same entity with identical layer and depth, the order is
rectangle, image, then text. Other entities still follow normal entity order.
All screen content follows World content. Disabled entities contribute no
text source. Visibility does not capture input or provide hit testing.

## Reusing shaping state

For several changing labels sharing one font, borrow a shaping session from
that registration. It keeps the parsed shaping face and at most one cached
plan instead of rebuilding them for every label. The session lives with the
caller; it is not a global cache or a self-referencing ECS component.

```rust
use sim_logic::prelude::*;

fn update_labels(font: &TextFont) -> LogicResult {
    let mut session = font.shaping_session()?;
    let mut label = ScreenTextVisual::new_with_session(
        &mut session,
        "Blocks: 100",
        LogicalScreenPosition::new(20.0, 40.0),
    )?;
    let previous = label.clone();
    label.set_text_with_session(&mut session, "Blocks: 101")?;
    assert_eq!(previous.text(), "Blocks: 100");
    assert_eq!(label.text(), "Blocks: 101");
    session.clear_scratch(); // Existing labels remain valid.
    Ok(())
}
```

`set_text_with_session` checks the exact font registration even when text is
unchanged. The session's size, direction and budgets are fixed by that font.
Failed updates preserve the old label and all published clones. Changed lines
still own new UTF-8/glyph storage: retaining immutable snapshots is not the same
as updating an exclusive Engine `ShapedLine` in place. This helper therefore
does not promise zero allocations for changing labels.

## Fonts and limits

`TextFont` is an opaque application registration, not a filename or save-file
key. Clones share parsed font bytes. Equal registrations remain distinct, and
handles survive World replacement within the issuing Application. Extraction
rejects a handle from another Application. Keeping a handle after application
shutdown retains its shared font bytes until the last clone is dropped.

One registration has a fixed logical em size and shaping direction. Register
another configuration for a second font size. `TextSettings` accepts Engine's
`FontBudget`, `TextLayoutBudget`, and `TextAtlasBudget`; these are ordinary
configuration values, not GPU resources.

| Setting | What it bounds |
| --- | --- |
| `TextLimits` | Font registration count and sum of source-font Vec capacities. Defaults: 8 registrations and 32 MiB. |
| `TextSettings::font_budget()` | Source capacity and declared font glyph count for one registration. |
| `TextSettings::layout_budget()` | Each label's UTF-8 bytes and shaped glyph count; raster work per glyph. Defaults: 4096 bytes and 1024 glyphs per label. |
| `TextSettings::atlas_budget()` | Fixed physical atlas dimensions, cached glyph count, and retained run limits. Default atlas: 1024 by 1024 RGBA, 4 MiB CPU plus 4 MiB GPU. |
| `RenderLimits::with_max_screen_texts` | Active extracted text sources, including empty labels. Default: zero. |
| `with_max_screen_text_bytes` / `with_max_screen_text_glyphs` | Aggregate UTF-8 input bytes and shaped glyph work per snapshot. Each placement counts separately. Defaults: zero. |
| `FrameLimits` | Composed sources, commands, vertices, uploads, referenced textures, and draw calls. |

Whitespace keeps logical advance and counts toward conservative glyph work,
even when it produces no visible quad. `retained_text_bytes()` reports actual
UTF-8 plus shaped-glyph capacity as a conservative upper bound on text storage;
use the more precise name `retained_layout_bytes()` for that same measurement.
Unlike the old 0.3 integration, it is not String capacity alone: Engine now owns
the string inside its retained layout and exposes the combined allocation.
The snapshot UTF-8 allowance still counts text length. CPU source bytes, parser
metadata, prepared strings, renderer recovery copies, and GPU storage are
different quantities. These
limits are not a total-process memory or untrusted-font execution guarantee.

The headless snippet's frame budget allows one World source, one text source,
and one default atlas. Add room for other labels, interleaved rectangle runs,
images, and 3D targets. Registering more fonts does not automatically enlarge
referenced texture limits.

## Caching, failures, and DPI

Constructing a label or changing its text/font runs Engine's CPU shaper.
The label now retains the complete `ShapedLine`, including its UTF-8 string,
glyph positions and exact font/style provenance, without a duplicate Logic
string. `shaped_line()` exposes it by immutable reference. Extraction clones
shared prepared values; it does not shape every frame.
Moving, aligning, tinting, and ordering reuse the same text storage. There is
no cache of every string the application has ever displayed.

Desktop presentation keeps a retained Engine run per live visible entity and
an atlas per used font registration. Unchanged lines reuse their buffers;
changed lines use Engine's capacity-preserving `update_from_shaped`. Placement
and tint are per-draw values and do not rewrite shared glyph layout. At display
scale 1.0, `prepare_from_shaped` and `update_from_shaped` borrow the exact line
already prepared by Logic, so GPU preparation does not repeat shaping.

There is one explicit DPI boundary: canonical labels prepare at scale 1.0,
while Engine checks the exact DPI of a prepared line against its atlas. At
other scales the desktop bridge prepares a temporary matching-DPI CPU line
before handing it to the atlas; it never rewrites provenance to bypass the
check. This fallback occurs only on changed text/font or resource rebuild.
Unchanged runs skip it completely. Eliminating this additional shaping at
non-unit DPI would require a separate Engine layout/raster-style contract;
it is not claimed by this integration.

Font registrations bound the number of retained atlases. The frozen active
label limit and each font's run budget bound retained label buffers, including
their reusable capacity. Retention can exceed the current string lengths;
the snapshot UTF-8/glyph allowances do not describe the whole desktop cache.
Empty strings allocate no atlas or run. Unused atlases may remain for reuse
until DPI/device invalidation or application shutdown.

CPU extraction publishes the complete World-plus-screen snapshot only after
validation. A rejected candidate or failed extraction does not publish a
partial text overlay. Rasterization and GPU preparation can still fail after
CPU validation: for example, a glyph may not fit the atlas at the current DPI.
The desktop runner returns a contextual text preparation error and presents
no partially composed frame. Earlier cache warming or successful individual
run uploads are not rolled back. This is not a recoverable UI error dialogue.

Changing DPI rebuilds desktop text resources for the new physical glyph size;
logical metrics and canonical application state do not change. Renderer
replacement invalidates GPU caches and rebuilds from retained CPU sources.
Old DPI variants and retired entity runs do not accumulate. The fixed atlas
does not grow or evict glyphs automatically; atlas exhaustion is an explicit
Engine error. Reused CPU layout is not a claim that every GPU frame allocates
zero memory.

## Current boundaries

This is single-line, single-font text. Engine supplies kerning, ligatures,
combining offsets, and one directional shaping run. It does not provide
mixed-direction paragraph layout, automatic wrapping, fallback fonts, rich
text, text selection, editing, buttons, or a layout tree. Newlines and control
characters are rejected. Missing glyphs report an error instead of silently
changing fonts. Labels currently have no independent user-specified clip;
normal target clipping still applies.

See [CPU tests](../../src/text/tests.rs),
[prepared-label/session tests](../../tests/text_preparation.rs), and
[headless runtime tests](../../tests/screen_text.rs) for executable contracts.

The CPU benchmark separates allocation counting from timing:

```bash
cargo bench --no-default-features --features text --bench screen_text
```

It covers 0, 1, 100 and 1000 labels, unchanged text, movement/tint, and changed
strings. This measures frame systems and CPU extraction, not GPU presentation.
The warmed unchanged/movement cases assert zero CPU allocations. Changed text
still allocates during shaping and preparation; it is measured, not exempted
from the result. To compare Engine shaping with Logic label preparation alone:

```bash
cargo bench --no-default-features --features text --bench screen_text -- --shaping-probe
```

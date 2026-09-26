# Finding your way around the code

[Documentation](../README.md) / Internals

The source tree groups files by responsibility. Public Rust paths are defined
in [lib.rs](../../src/lib.rs), so a physical directory name is not necessarily a
public module path. For example, input lives under `src/application/` and is
available to users as `sim_logic::input`.

| Directory | What lives there |
| --- | --- |
| [application](../../src/application) | Application setup, input, time, resources, transitions, and the shared runner. |
| [assets](../../src/assets) | Immutable CPU image registrations, opaque handles, and storage limits. |
| [audio](../../src/audio) | Optional single-source device output, buffer policy, and diagnostics. |
| [text](../../src/text) | Optional font registrations, single-line text values, metrics, and limits. |
| [ui](../../src/ui) | Caller-driven pointer-button ownership; no renderer or global input consumption. |
| [ecs](../../src/ecs) | Entity identity, queries, System registration, World construction, events, and Commands. |
| [logic](../../src/logic) | Generic overlap geometry and opt-in movement helpers. |
| [rendering](../../src/rendering) | Visual components, CPU extraction, and render limits. |
| [platform](../../src/platform) | Window events and the desktop renderer adapter. |
| [examples](../../examples) | Complete applications and their own gameplay or presentation rules. |
| [tests](../../tests) | Acceptance tests using the public API. |

## Build storage

This checkout uses `debug = "line-tables-only"` and `incremental = false` in
the development profile. Tests inherit those settings. Backtraces keep file
and line information, but the default artifacts do not contain full debugger
variable/type information or the incremental compiler cache. Debug assertions
and integer-overflow checks remain enabled; release settings are unchanged.
See [Cargo profiles](https://doc.rust-lang.org/cargo/reference/profiles.html)
for the profile and inheritance rules. These checkout settings do not override
the workspace profile of an application that depends on Sim;Logic.

For a full debugger session, opt in temporarily:

```bash
CARGO_PROFILE_DEV_DEBUG=2 cargo build --example moving_ball
```

Different feature combinations and profile overrides still retain different
artifacts. Check disk use before building a large matrix; the profile reduces
growth but is not a hard disk quota. Once no Cargo build is using this checkout,
remove its rebuildable development/test artifacts while keeping release builds:

```bash
du -sh target
cargo clean -p sim-logic --profile dev
```

This removes this package's development/test products, not shared dependency
artifacts or release binaries. Nothing cleans automatically during compilation
or application startup.
Keep one feature combination while iterating, then run broader checks before
delivery. Use `cargo check` when linking/running a binary is not needed.

## The shared application runner

[headless.rs](../../src/application/headless.rs) is the small public facade for
`HeadlessRunner`: it declares ownership, provides read-only inspection and time
controls, and re-exports frame diagnostics. Despite the name, the desktop host
uses this same runner to advance application state.

Its private [runtime directory](../../src/application/runtime) separates the
implementation into focused files:

| File | Start here for |
| --- | --- |
| [mod.rs](../../src/application/runtime/mod.rs) | The active World's private state and owned query caches. |
| [reports.rs](../../src/application/runtime/reports.rs) | Frame requests, outcomes, errors, and lifecycle records exposed through the public facade. |
| [lifecycle.rs](../../src/application/runtime/lifecycle.rs) | Building the runner, preparing isolated candidate Worlds, and recording lifecycle history. |
| [frame.rs](../../src/application/runtime/frame.rs) | `advance_frame`, stage order, fixed catch-up, pause, and frame failure handling. |
| [transitions.rs](../../src/application/runtime/transitions.rs) | Validating transition intents, resolving competing requests, and installing a replacement World. |
| [rendering.rs](../../src/application/runtime/rendering.rs) | Cached queries, fixed interpolation history, and staging World state for extraction. |
| [barriers.rs](../../src/application/runtime/barriers.rs) | Running a stage, ending its event lifetime, and applying or discarding its queued Commands. |
| [snapshots.rs](../../src/application/runtime/snapshots.rs) | Publishing the right input snapshot for each stage while reusing its storage. |
| [tests](../../src/application/runtime/tests) | Internal regressions and deliberate invalid-state checks grouped by topic. |

The public paths remain `sim_logic::headless` and `sim_logic::prelude`.
Applications do not import the private runtime files. Splitting their source
does not create a second runner or a separate desktop simulation loop.

For a frame-order question, begin with `frame.rs`, then follow its stage,
transition, or extraction call into the matching file. The
[runtime guide](../guides/Runtime.md) explains the observable rules without requiring
you to read those internals.

## Game code and runtime code

The [image board](../../examples/image_board/game.rs) shares its setup with
headless tests. Image registration lives in `assets`, the managed component
in `rendering/screen`, and mixed draw-plan extraction in `rendering/extraction`.
Only `platform/desktop/images.rs` owns the Engine GPU cache and presentation.
Neither World factories nor the CPU registry need a renderer.

The [text labels](../../examples/text_labels/scene.rs) use the same separation.
`text` owns shared CPU font registrations and prepared labels;
`rendering/extraction/text.rs` checks their provenance and publishes snapshots.
`platform/desktop/text.rs` owns Engine atlases and retained GPU runs. The mixed
screen compositor in `platform/desktop/images.rs` orders text, images, and
geometry runs together. `headless-text` compiles the CPU half without GPU/window
dependencies; `text` adds the retained GPU path. Shaping and rasterization remain
Engine responsibilities.

The [interface lab](../../examples/interface_lab/main.rs) exercises the stock
host's optional `application/window.rs` requests through
`platform/desktop/window.rs`. Portable key catalogs and native mappings have
their own `input/keyboard.rs` and `desktop/keyboard.rs` modules. Screen rounded
rectangles, vectors and clip values live in `rendering/screen`; extraction
batches them without introducing another renderer. `ui/focus.rs` is a standalone
typed helper, not a second input dispatcher or a layout system.

The optional [`draw_crab`](../rendering/Screen-Images.md#ferris-easter-egg) helper and its small embedded PNG
live in `rendering/easter_eggs`. It registers a normal image during setup and
adds no drawing path of its own.

[Iron Maze](../examples/Iron-Maze.md) shows the same separation at application scale:
`game.rs` updates the game's state, `level.rs` owns its map geometry, and
`render.rs` turns that state into screen rectangles. Its shared `mod.rs`
connects these functions to Sim;Logic; `main.rs` supplies the window entry
point. Its public integration tests reuse that setup headlessly.

Keep application-specific rules with the application. A reusable runtime
helper belongs in the library when it removes repeated infrastructure or
enforces a clear invariant, with its behavior covered through the public API.

The [piano roll](../examples/Piano-Roll.md) keeps notes, synthesis, editing and its 3D
instrument in `examples/piano_roll`. `music.rs` owns sample-clock state;
`control.rs` connects it to the editor through a bounded queue. `view.rs` and
`model3d.rs` read that shared state. Only `main.rs` owns a real audio device.
The generic cuboid bridge lives in `rendering/three_d`; its retained GPU
resources live in `platform/desktop/three_d.rs`. Original bitmap glyphs shared
by the two larger examples live in `examples/support/bitmap_font.rs`.

## Checks and measurements

[Release checks](Releasing.md) describe the test, feature, documentation and
package gates. For focused CPU measurements, use the repository's benchmarks:

```bash
cargo bench --no-default-features --bench headless_runtime
cargo bench --no-default-features --bench headless_runtime -- --motion-only
cargo bench --no-default-features --features text --bench screen_text
```

The headless benchmark separates timing from allocator counting. Its focused
selectors are defined in [the benchmark entry point](../../benches/headless_runtime.rs):

| Area | Selectors |
| --- | --- |
| Motion | `--motion-only`, `--acceleration-motion-only`, `--digital-motion-only` |
| Overlap | `--overlap-only`, `--rectangle-overlap-only`, `--mixed-overlap-only` |
| Structural Commands | `--insert-only`, `--remove-only`, `--enablement-only`, `--exit-only` |
| Input/UI | `--pointer-only`, `--input-cancellation-only`, `--buttons-only` |
| Extraction | `--visual-only`, `--images-only` |

Warmed zero-allocation gates apply only to their named workloads and fixed
capacities, not arbitrary application Systems. The insert gate deliberately
counts the boxed payload allocation per queued insert. Overlap measurements
include repeated linear scans; they are not evidence of a spatial broad phase.
Timing is diagnostic, without a machine-independent frame-rate threshold.

The [text guide](../rendering/Text.md), [audio guide](../guides/Audio-Output.md)
and example guides describe their own benchmarks. CPU headless results exclude
native event delivery, surface acquisition and GPU work. Compare desktop runs
on the same source, workload, adapter, surface format and presentation mode;
keep acquisition and GPU diagnostics separate from CPU extraction time.

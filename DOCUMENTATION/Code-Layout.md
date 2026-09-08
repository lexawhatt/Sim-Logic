# Finding your way around the code

The source tree groups files by responsibility. Public Rust paths are defined
in [lib.rs](../src/lib.rs), so a physical directory name is not necessarily a
public module path. For example, input lives under `src/application/` and is
available to users as `sim_logic::input`.

| Directory | What lives there |
| --- | --- |
| [application](../src/application/) | Application setup, input, time, resources, transitions, and the shared runner. |
| [assets](../src/assets/) | Immutable CPU image registrations, opaque handles, and storage limits. |
| [ecs](../src/ecs/) | Entity identity, queries, System registration, World construction, events, and Commands. |
| [logic](../src/logic/) | Generic overlap geometry and opt-in movement helpers. |
| [rendering](../src/rendering/) | Visual components, CPU extraction, and render limits. |
| [platform](../src/platform/) | Window events and the desktop renderer adapter. |
| [examples](../examples/) | Complete applications and their own gameplay or presentation rules. |
| [tests](../tests/) | Acceptance tests using the public API. |

## The shared application runner

[headless.rs](../src/application/headless.rs) is the small public facade for
`HeadlessRunner`: it declares ownership, provides read-only inspection and time
controls, and re-exports frame diagnostics. Despite the name, the desktop host
uses this same runner to advance application state.

Its private [runtime directory](../src/application/runtime/) separates the
implementation into focused files:

| File | Start here for |
| --- | --- |
| [mod.rs](../src/application/runtime/mod.rs) | The active World's private state and owned query caches. |
| [reports.rs](../src/application/runtime/reports.rs) | Frame requests, outcomes, errors, and lifecycle records exposed through the public facade. |
| [lifecycle.rs](../src/application/runtime/lifecycle.rs) | Building the runner, preparing isolated candidate Worlds, and recording lifecycle history. |
| [frame.rs](../src/application/runtime/frame.rs) | `advance_frame`, stage order, fixed catch-up, pause, and frame failure handling. |
| [transitions.rs](../src/application/runtime/transitions.rs) | Validating transition intents, resolving competing requests, and installing a replacement World. |
| [rendering.rs](../src/application/runtime/rendering.rs) | Cached queries, fixed interpolation history, and staging World state for extraction. |
| [barriers.rs](../src/application/runtime/barriers.rs) | Running a stage, ending its event lifetime, and applying or discarding its queued Commands. |
| [snapshots.rs](../src/application/runtime/snapshots.rs) | Publishing the right input snapshot for each stage while reusing its storage. |
| [tests](../src/application/runtime/tests/) | Internal regressions and deliberate invalid-state checks grouped by topic. |

The public paths remain `sim_logic::headless` and `sim_logic::prelude`.
Applications do not import the private runtime files. Splitting their source
does not create a second runner or a separate desktop simulation loop.

For a frame-order question, begin with `frame.rs`, then follow its stage,
transition, or extraction call into the matching file. The
[runtime guide](Runtime.md) explains the observable rules without requiring
you to read those internals.

## Game code and runtime code

The [image board](../examples/image_board/game.rs) shares its setup with
headless tests. Image registration lives in `assets`, the managed component
in `rendering/screen`, and mixed draw-plan extraction in `rendering/extraction`.
Only `platform/desktop/images.rs` owns the Engine GPU cache and presentation.
Neither World factories nor the CPU registry need a renderer.

[Iron Maze](Iron-Maze.md) shows the same separation at application scale:
`game.rs` updates the game's state, `level.rs` owns its map geometry, and
`render.rs` turns that state into screen rectangles. Its shared `mod.rs`
connects these functions to Sim;Logic; `main.rs` supplies the window entry
point. Its public integration tests reuse that setup headlessly.

Keep application-specific rules with the application. A reusable runtime
helper belongs in the library when it removes repeated infrastructure or
enforces a clear invariant, with its behavior covered through the public API.

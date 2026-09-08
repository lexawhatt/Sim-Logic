# How the runtime works

## Objects, data, and functions

An entity is an object identity. Components are ordinary Rust values attached
to it: position, appearance, velocity, or your own data. A System is a function
that reads or changes matching data. This separation is often called ECS:
Entity, Component, System.

`Query<&mut Transform2d, With<Player>>` means "give me the positions of enabled
managed entities carrying `Player`, and let me change them". `Single` instead
requires exactly one match; zero or multiple matches fail before the System
body runs. Use `Query::single()` when your System should handle those cases.

Use the types from `sim_logic::prelude`, not raw Bevy queries or commands.
Sim;Logic checks that handles belong to the active World and limits structural
work. A saved `LogicEntity` is not a permanent save-file ID: replacing its World
invalidates the handle.

A resource is one shared value rather than one value per entity:

| Access | Owner | Survives World replacement? |
| --- | --- | --- |
| `Res<T>` / `ResMut<T>` | The current World | No |
| `AppRes<T>` / `AppResMut<T>` | The Application | Yes, without cloning |

Insert World Resources through `WorldBuilder::insert_resource`. Register
Application Resources once with `Application::register_app_resource` before
starting. Factories and Startup cannot read or change live Application
Resources.

## Update order

Systems in each stage run sequentially in registration order. Adding a
standard helper System places it at that exact position too; no separate
hidden movement schedule runs afterward.

1. Build an isolated World through its registered factory, run `Startup`, and
   validate that it can produce a render snapshot.
2. For each application frame, collect input and calculate the fixed work due.
3. Run zero or more `FixedUpdate` ticks, applying queued Commands after each
   completed tick.
4. Run `FrameUpdate` once, then apply its Commands.
5. Extract visual state and, on desktop, give it to Sim;Engine for drawing.

This is the normal path. A failed stage, fixed-stage replacement request, or
application exit can stop the remaining stages. Startup belongs to each World
candidate, not just the first World in the application.

For example, register acceleration before velocity when position should use
the newly updated velocity. Register camera following after movement when the
camera should follow the result of that tick. Adding a helper twice runs it
twice; components such as `LinearVelocity2d` are inert without their helper or
your own System.

## Immediate changes and deferred Commands

Changing a borrowed component or resource is immediate. Later Systems in the
same stage see the new value. Creating an entity, changing which components
it has, disabling it, or despawning it instead goes through `Commands`.

A stage barrier is simply the point after all Systems in one stage invocation
have finished. Commands become visible there, not after each individual
System. A projectile spawned during one fixed tick cannot move through an
ordinary query until a later tick. A despawned target remains queryable until
the barrier; collision code must account for multiple hits in the same tick.

The complete structural batch is checked before it changes the World. A
failed System or rejected batch discards queued structural and application
control changes. Do not confuse this with rolling back the whole stage:
component edits, resource edits, and external effects already performed by
Systems are not undone.

`Commands::insert` can add or replace approved components. `remove::<T>`
removes the named component without deleting the entity. `disable` keeps all
data but excludes the entity from ordinary queries, overlap checks, and
drawing. Keep its handle if you need to enable it later. Disabled entities
still count toward the configured entity limit.

Command and entity limits are explicit in `AppConfig`. Propagate enqueue
errors with `?` and inspect frame reports; accepting an enqueue does not yet
mean its batch will commit. `LogicResult` is optional shorthand for combining
several error types, not a requirement for your Systems.

## Fixed time and smooth drawing

`FixedUpdate` receives the same configured duration on each tick through
`FixedTime`. The default step is approximately 1/60 second, with at most eight
catch-up ticks per application frame. Catch-up means doing several fixed
updates after a slow frame; excess whole ticks are dropped and reported so
one late frame cannot request unlimited work.

`FrameUpdate` receives the frame's unscaled wall duration through `FrameTime`.
Use it for application controls and presentation changes that should not wait
for the next simulation tick. Canonical `Transform2d` and `ActiveCamera2d`
mutation belongs to fixed updates; requesting mutable access to either in
FrameUpdate is rejected. Structural replacement through Commands remains
available.

For smooth drawing, Sim;Logic stores the previous and current fixed positions
and camera centers. Extraction draws between them using the unused fraction
of a fixed step. It never writes that smoothed value back into simulation
state. Collision and model calculations should read the canonical components,
not the rendered positions.

Use `Transform2d::teleport` and `ActiveCamera2d::teleport_center` for deliberate
jumps that should not be interpolated. Teleporting an object does not
automatically teleport its camera.

## Input and typed events

Physical keys and left, right, and middle mouse buttons are mapped to your
action enum. `digital_axis` returns the raw two-dimensional direction;
`normalized_digital_axis` gives a constant-speed
direction and safely returns zero for idle input. Binding WASD and arrows to
one action axis does not run movement by itself.

Held state answers "is any bound control down?". An edge records a particular
press or release. `has_press_occurrence` checks for at least one press without
consuming it; `pressed` iterates all occurrences. Two physical controls mapped
to the same action can produce two edges, even while the action remains held
throughout.

Frame input lasts for its application frame. Fixed edges wait through
zero-tick frames and are delivered to the first consuming fixed tick, not
repeated for every catch-up tick. Multiple Systems in that tick can read the
same snapshot. Application-controlled pause has the special rules below.

Both input parameters expose the latest pointer sample through `pointer()`.
Mouse edges also retain their own sample from the time of that button event,
including its logical viewport. Use the edge's sample when a click should
refer to its original position. The [pointer guide](Pointer-Input.md) explains
coordinate conversion, leave events, stage isolation, and the input bounds.

Typed events are small `Copy` values sent with `EventWriter<E>`. Later Systems
in the same stage invocation see them through `EventReader<E>`; reads do not
consume the records. They do not automatically cross into another stage or
tick. A reader registered before a writer will not be called again to see the
new event. A failed send stops that stage's successful commit even if its
returned error was ignored.

## Pause and exit

`Commands::set_paused(true)` pauses fixed simulation at the successful stage
barrier. Remaining catch-up ticks stop, but FrameUpdate, input collection,
extraction, and drawing continue. This is why a pause-toggle System normally
belongs in FrameUpdate: it must still run to resume the application.

`FrameTime::is_paused()` is the value at FrameUpdate entry. All Systems in that
invocation see the same value, even if an earlier one queued a change. Within
one batch the last requested pause value wins, and each call uses one command
slot. Failed batches discard the change; Startup cannot request it.

Entering pause snaps interpolation to the current state. Previously
accumulated fixed time is kept, but paused wall time is not added. Resuming in
FrameUpdate makes retained ticks eligible on the next frame, not retroactively
in the current one. Press and release edges collected while paused remain
FrameInput-only; they do not replay into FixedInput after resume. Held state
still persists.

Pause is Application-owned and survives World replacement. Replacement keeps
its own stronger time-cleanup rules. A replacement failure does not undo a
pause change from an otherwise committed batch.

`Commands::request_exit()` also takes effect at the barrier. It stops remaining
fixed work, skips FrameUpdate when issued in a fixed tick, and produces no new
extraction or presentation. Exit takes precedence over replacement and
suppresses pause changes in that same batch. The desktop loop returns; a
headless host observes `report.exit_requested()` and stops its own loop. Exit
is not a cleanup hook and does not emit a `WorldExit` lifecycle record.

## Replacing the World

Register a repeatable construction function with `register_world`. The
returned `WorldFactoryId` identifies that registration, not its display name.
For a simple route, insert `WorldReplacementOnPress` into the source World and
register `add_world_replacement_on_press_system`. It uses FixedUpdate; for
conditional routing or a transition while paused, write a FrameUpdate System.

The direct path is `commands.replace_world(intent, target)`. Use the originating
input edge's `intent()` or issue one with `commands.new_transition_intent()`
and share it with other Systems handling that same cause. Do not invent or
persist these runtime-issued tokens.

Requests from one successful stage batch are compared before a target is
built:

| Requests | Result |
| --- | --- |
| Same intent, same target | One replacement. |
| Same intent, different targets | Rejected as a malformed intent. |
| Different intents, same target | One replacement with a convergence warning. |
| Different intents, different targets | Rejected as a conflict. |

The target is built in isolation, including Startup and render validation.
If preparation fails, the old World stays active. The factory must finish
synchronously, change only its candidate, and avoid I/O, workers, or shared
external mutations: those effects cannot be undone by dropping a candidate.

After validation, the runner records the old World's exit, retires its
generation, installs the candidate, transfers Application Resources, and
records entry. This is irreversible. These lifecycle records are diagnostics,
not user-defined `WorldExit` or `WorldEnter` hooks. The new World can be
extracted in the same frame; its ordinary update Systems begin on a later
frame. A fixed-stage replacement attempt skips the remaining fixed ticks and
FrameUpdate even if the transition is rejected or preparation fails.

The public path currently has no transition payload store, asynchronous
preparation, fallible activation hooks, or `Faulted` recovery schedule. Panic,
abort, and failures outside Rust's typed error path are not covered by a
rollback or resumable-runner promise.

## Drawing and diagnosing failures

Extraction means reading managed visual components and preparing a bounded
CPU snapshot for Sim;Engine. It requires exactly one enabled active camera,
even headlessly. Circles, axis-aligned rectangles, and anchored lines are
supported; colliders are separate optional data and never draw by themselves.

`ScreenRectangleVisual` draws a separate, screen-fixed rectangle above the
World. It uses top-left logical pixel coordinates, requires no `Transform2d`,
and can be changed directly in FrameUpdate. It is a building block for status
panels, not a clickable widget. See [screen-fixed panels](Screen-HUD.md).

`ScreenImageVisual` adds immutable registered images with source regions,
tint, and filtering. Images and rectangles share a screen draw order. Their
pixels survive World replacement, and the desktop host caches prepared Engine
images. See [screen images](Screen-Images.md) for explicit limits and ownership.

Extraction rebuilds draw plans using reusable buffers; it does not patch only
changed components. Immutable image resources are retained separately from
those per-frame plans. Some warmed workloads have allocation tests, but
arbitrary application Systems are not promised to allocate nothing.

`FrameOutcome::Rejected` means frame inputs were refused before the frame
changed live state. `FrameOutcome::Advanced` means it started; inspect its
report for System, command, or extraction failures and for transition results.
A previously published snapshot can remain after failure. Headless consumers
must compare its World generation with the active generation before treating
it as current visual state.

For executable usage patterns, return to [getting started](Getting-Started.md)
or the [example applications](../examples).

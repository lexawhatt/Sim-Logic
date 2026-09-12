# Sim;Logic

Sim;Logic is a code-first application layer for
[Sim;Engine](https://crates.io/crates/sim-engine), currently pinned to 0.3.0.
It keeps the program in ordinary Rust, but supplies the pieces that otherwise
have to be rebuilt for every interactive simulation: entities and components, ordered systems, typed
input, fixed updates, interpolation, World replacement, and a window/render
loop.

The intended feel is closer to a small game engine than to a collection of
unrelated rendering calls. There is no editor and no hidden object hierarchy.
State remains explicit Rust data, and behavior remains explicit Rust systems.

Start with the [documentation](DOCUMENTATION/README.md) for a runnable
headless example and a plain-language explanation of the runtime.

## Play Frontier

Conquer a seeded island against six bots in an offline territory game:

```bash
cargo run --release --example territory_wars
```

Click land to start, then click unclaimed land or a bordering rival to send
troops. Adjust the attack percentage with the slider; Space expands, P pauses,
and R restarts. F3 opens the mechanics/AI inspector. F4 arms the optional cheats;
using them marks the run as assisted. The [Frontier guide](DOCUMENTATION/examples/Territory-Wars.md)
explains the controls and the separate income, combat, and bot modules.

## Play the piano

Draw a short melody in a 2D piano roll, play it with synthesized sound, and
switch to a depth-tested 3D upright piano without restarting the music:

```bash
cargo run --release --features audio --example piano_roll
```

Left click adds/selects notes, dragging changes their length, and right click
removes them. Space plays/pauses; Enter switches 2D/3D. Click the bottom keys
or hold A/S/D/W to play C/D/E/F. The [piano guide](DOCUMENTATION/examples/Piano-Roll.md)
explains all controls, silent mode, timing, and the current limits.

## Play Iron Maze

Iron Maze is a small original 2.5D shooter built from ordinary Sim;Logic
Systems and managed screen rectangles:

```bash
cargo run --release --example iron_maze
```

Use WASD to move and strafe, Left/Right arrows to turn, and Space or the left
mouse button to fire. Defeat six sentries, collect health and ammunition, and
reach the exit beacon. Enter restarts the level; Escape exits.
The example's raycaster and gameplay stay in its own code, without a new
general 3D renderer or external game assets. See the
[Iron Maze guide](DOCUMENTATION/examples/Iron-Maze.md) for controls, limits, source
layout, and the shared headless tests.

## Current slice

Optional real-font labels use Engine's shaping and retained rendering:

```bash
cargo run --release --features text --example text_labels
```

The [text guide](DOCUMENTATION/rendering/Text.md) covers font registration,
baseline alignment, changing strings, headless inspection and bounded caches.

The first experimental slice can:

- create one desktop window or run the same lifecycle headlessly;
- build Worlds from registered synchronous factories;
- run `Startup`, `FixedUpdate`, and `FrameUpdate` systems in registration order;
- let systems return `Result` and stop their stage with a reported error;
- pass bounded typed events between ordered systems in one stage invocation;
- keep explicitly registered typed Application Resources across World replacement;
- map W/A/S/D, P/R/N, digits 1-5, arrows, Space, Enter, Escape, and
  F3/F4/F5/F6/F8/F9 to application-defined actions;
- map left, right, and middle mouse buttons to actions with click-time pointer
  samples and explicit screen-to-world coordinate conversion;
- keep each input edge's physical key or mouse button, and distinguish an
  ordinary release from cancellation on pointer leave or focus loss;
- find live same-shape overlaps among typed circular colliders and among
  axis-aligned rectangular colliders;
- optionally integrate finite `LinearVelocity2d` components during fixed
  updates at an explicit point in system order;
- optionally integrate finite `LinearAcceleration2d` into velocity at another
  explicit point in fixed-system order;
- optionally drive `DigitalMovement2d` components from per-entity typed axes
  and speeds at an explicit point in fixed-system order;
- optionally follow one enabled `CameraFollowTarget2d` from the active camera
  at another explicit point in fixed-system order;
- optionally map one World-local fixed input action to an ordinary deferred
  World replacement without writing a routing System;
- supply a default origin `Transform2d` when a standard visual or collider is
  spawned without one;
- queue bounded spawn, component insert/replace/removal, despawn, no-payload
  World-replacement, entity enable/disable, fixed-pause, and application-exit commands;
- queue many copies of one `Copy` bundle while retaining one erased spawn
  template and preserving the per-entity command bound;
- interpolate `Transform2d` and the active camera center between fixed ticks;
- extract a camera, filled circles, axis-aligned rectangles, and anchored lines
  into one bounded Sim;Engine scene;
- draw screen-fixed rectangle panels above that scene, with logical-pixel
  layout that keeps updating while simulation is paused;
- register bounded immutable RGBA8 assets and draw screen images with source
  regions, tint, filtering, and shared rectangle/image ordering;
- register fonts and draw optional single-line screen text with alignment,
  changing content, tint, and shared text/image/rectangle ordering;
- extract opt-in colored cuboids and a current-value 3D view, with retained
  desktop geometry, bounded camera-plane clipping, a depth target, and screen
  overlays; object-local preflight errors preserve the current managed entity;
- optionally send one application-owned mono PCM source to an audio device,
  with explicit buffer policy, lifetime and diagnostics;
- commit A-to-B World replacement and extract B in the same logical frame.

This is not a finished general-purpose engine. Generic transition payloads,
asynchronous loading, `Faulted` recovery, Behavior sugar, a full UI toolkit,
arbitrary 3D assets, a World-scoped audio service, and parallel
schedules are deliberately not public yet. The [audio output](DOCUMENTATION/guides/Audio-Output.md)
adapter is optional, immediate and independent of World transactions; it is
not a full sound engine. The [3D bridge](DOCUMENTATION/rendering/ThreeD.md) currently
supports cuboids, not arbitrary meshes, lighting or materials. Engine 0.3's
additional mesh and texture capabilities are not yet managed Logic components.

Inspect input sources and cancellation without a window using
`cargo run --no-default-features --example input_cancellation`. The
[input guide](DOCUMENTATION/guides/Pointer-Input.md) explains why a cancelled
release must not confirm a click or drag.

Try the image path with `cargo run --release --example image_board`.
WASD/arrows move an image, Space changes its source region, Enter opens an
alternate World sharing the same pixels, and Escape exits. The
[screen-image guide](DOCUMENTATION/rendering/Screen-Images.md) explains registration,
explicit rendering limits, memory, and headless inspection.

A small optional Easter egg: enable `easter-eggs` to use the library's
[`draw_crab` helper](DOCUMENTATION/rendering/Screen-Images.md#ferris-easter-egg). It places the bundled
Ferris artwork through the same screen-image renderer.

## Small example

```rust,no_run
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action { Up, Down, Left, Right, Exit }

const MOVEMENT: DigitalAxis2d<Action> = DigitalAxis2d::new(
    Action::Left,
    Action::Right,
    Action::Down,
    Action::Up,
);

#[derive(Component)]
struct Ball;
# fn main() -> LogicResult {
let mut app = Application::<Action>::new(AppConfig::default())?;
app.approve_component::<Ball>()?;
app.bind_wasd_and_arrows(MOVEMENT)?;

let camera = ActiveCamera2d::centered(32.0)?;
let circle = CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?;
let movement = DigitalMovement2d::new(MOVEMENT, 8.0)?;
let initial = app.register_world("main", move |world| {
    world.spawn(camera)?;
    world.spawn((Ball, circle, movement))?;
    Ok(())
})?;

app.add_digital_movement2d_system();
app.run(initial)?;
# Ok(())
# }
```

For a small fixed group created by a World factory, `spawn_array` removes the
repeated setup loop while keeping the number of returned handles in the type:

```rust,ignore
let enemy_handles = world.spawn_array([
    (Enemy, Transform2d::from_xy(-4.0, 0.0)?, enemy_visual),
    (Enemy, Transform2d::from_xy(0.0, 0.0)?, enemy_visual),
    (Enemy, Transform2d::from_xy(4.0, 0.0)?, enemy_visual),
])?;
```

The bundle type and capacity for the complete array are checked before the
first entity is created, so a typed error never leaves a spawned prefix.
Handles correspond to input indices. This is an immediate `WorldBuilder`
convenience for small fixed arrays, not a dynamic batch-insertion speed promise.
Runtime structural changes still go through deferred, bounded `Commands` at a
stage barrier.

When runtime code needs several identical entities, `spawn_copies` avoids a
loop of separately erased spawn payloads:

```rust,ignore
let projectile = (Projectile, transform, visual, collider, velocity);
commands.spawn_copies(projectile, shots)?;
```

The bundle must be `Copy`. A nonzero count creates distinct managed entities
consecutively at the stage barrier and is charged that many slots against the
stage command limit; one compact queue entry cannot bypass the work bound. The
complete entity count and component approval are checked before any copy is
spawned. A zero count is a true no-op and does not validate the bundle. Like
singular deferred spawn, the method returns no handles because the entities do
not exist until the barrier. It reduces retained erased templates, but still
performs one ECS spawn per entity and is not a pooling or prefab system.

`LogicResult<T = ()>` is optional shorthand for application and fallible-System
code that combines unrelated typed errors with `?`. Sim;Logic APIs still
return their concrete error types, so code with one useful error type can keep
returning it directly. The shorthand uses `Box<dyn std::error::Error>`; define
a project-specific result when errors must be `Send + Sync` or represented by
an enum.

`DigitalAxis2d` combines logical actions, not physical keys. `digital_axis`
returns raw components in `-1.0..=1.0`; opposite actions cancel. Choose
`normalized_digital_axis` for a constant-speed direction or keep
`digital_axis` for raw diagonal magnitude and grid movement. The same
descriptor works with `FrameInput` and `FixedInput`, and neither method
consumes press or release edges.
`bind_wasd_and_arrows` maps all eight physical keys to the matching slots
atomically. The narrower `bind_wasd` and `bind_arrows` presets remain available
when a project wants only one key family. Escape is an ordinary key with no
hidden quit or pause behavior; bind it to an application action with `bind_key`.
`add_fixed_system` and `add_frame_system` register ordinary update Systems;
their `add_fallible_*` forms let a returned error stop that stage. The general
`add_system(Stage, ...)` API remains available for Startup and for code that
chooses a stage as data. Short and general calls share one registration order.

## Application exit

Exit is explicit application logic rather than a hidden Escape-key behavior:

```rust,ignore
fn exit_on_escape(
    input: FrameInput<Action>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    if input.has_press_occurrence(Action::Exit) {
        commands.request_exit()?;
    }
    Ok(())
}

app.bind_key(PhysicalKeyCode::Escape, Action::Exit)?;
app.add_fallible_frame_system(exit_on_escape);
```

`has_press_occurrence` and `has_release_occurrence` are repeatable checks for
at least one physical occurrence in the current input snapshot; they do not
consume it. Use the `pressed` and `released` iterators when code needs every
occurrence, its order, or its transition-intent token. With multiple physical
keys mapped to one action, an edge can exist while that action remains held by
another key. These checks deliberately describe physical occurrences, not an
aggregate transition of the logical action.

The request takes effect after every System in that stage has run and only if
the complete command batch succeeds. From FixedUpdate it stops the remaining
catch-up ticks and skips FrameUpdate. It always skips new extraction and
desktop presentation; the desktop loop then returns `Ok(DesktopRunReport)`
with `DesktopExitReason::ApplicationRequested`. Repeated requests coalesce in
the result, though every call still occupies one slot in the bounded command
batch. Application exit takes precedence over replacement requests in that
same committed batch. It does not emit `WorldExit`, run a cleanup hook, or
present one final frame. A headless host reads
`LogicFrameReport::exit_requested()` and decides when to stop its own loop. If
it deliberately continues, pending input edges do not replay and interpolation
resumes from the current canonical state.

## Pausing fixed simulation

Pause can be controlled from an ordinary FrameUpdate System, including in a
desktop application whose runner is owned by the window loop:

```rust,no_run
use sim_logic::prelude::*;
# #[derive(Clone, Copy, PartialEq, Eq, Hash)]
# enum Action { Pause }

fn toggle_pause(
    input: FrameInput<Action>,
    time: FrameTime,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(Action::Pause) {
        commands.set_paused(!time.is_paused())?;
    }
    Ok(())
}

# fn main() -> LogicResult {
# let mut app = Application::<Action>::new(AppConfig::default())?;
app.bind_key(PhysicalKeyCode::Space, Action::Pause)?;
app.add_fallible_frame_system(toggle_pause);
# Ok(())
# }
```

`FrameTime::is_paused()` is a stage-entry snapshot: later Systems in the same
FrameUpdate still see the old value. The change takes effect at the successful
command barrier; failed Systems or rejected batches discard it. Multiple
requests in one stage are last-write-wins, and every call uses one command
slot. Startup cannot change application pause.

Pause stops FixedUpdate, including remaining catch-up ticks after a fixed-stage
request. FrameUpdate, input collection, extraction, and presentation continue.
Entering pause snaps interpolation to current state. Accumulated fixed time is
preserved, but paused wall time is not added. A FrameUpdate resume makes retained
work eligible on the next frame, with safe extraction in the resume frame.
Input edges collected while paused are FrameInput-only and do not replay into
FixedInput; held actions remain held.

Pause belongs to the Application and survives World replacement. Replacement
retains its own time-cleanup and stage-skipping rules; even an unsuccessful
transition does not undo a committed pause. Same-batch application exit
suppresses the pause change. Headless hosts can also call the immediate
`HeadlessRunner::set_paused` between frames and inspect `is_paused()`.

The Moving Ball example uses Space to pause/resume. Its Enter replacement runs
in FixedUpdate, so it is inactive while paused; a fixed replacement also skips
that frame's Space policy. No physical key or toggle policy is built in.

## Screen-fixed panels

`ScreenRectangleVisual` draws a filled rectangle above the World. Its top-left
position and size use logical screen pixels: x goes right and y goes down.
Moving, zooming, or rotating the World camera does not move the panel. It needs
no `Transform2d`; FrameUpdate can change it directly and use `FrameViewport`
for resize-aware layout, even while simulation is paused.

```rust,ignore
let panel = ScreenRectangleVisual::new(
    LogicalScreenPosition::new(24.0, 24.0),
    LogicalScreenVector::new(240.0, 40.0),
    Color::rgb8(22, 31, 49),
)?;
world.spawn(panel)?;
```

The default allows 256 screen rectangles. World and screen scenes publish as
one snapshot: a failure in either cannot publish half a new frame. Screen
layers always follow World layers. Without images or text, nonempty screen rectangles
use one extra pass and empty screen content uses none. Images and text can split that
rectangle pass into ordered runs. These are presentation primitives, not text,
buttons, mouse handling, or a layout tree.

Run `cargo run --release --example screen_hud` to see a moving World with a
fixed status panel and progress bar. Space pauses/resumes; the activity
indicator continues animating during pause; Escape exits. See the
[screen panel guide](DOCUMENTATION/rendering/Screen-HUD.md) for a runnable headless
example, coordinate rules, ordering, and limits.

## World replacement on a press

For the common one-route case, a World can own the action and target as data:

```rust,ignore
let results = app.register_world("results", move |world| {
    world.spawn(results_camera)?;
    Ok(())
})?;

let play = app.register_world("play", move |world| {
    world.spawn(play_camera)?;
    world.insert_resource(WorldReplacementOnPress::new(
        Action::ShowResults,
        results,
    ))?;
    Ok(())
})?;

app.bind_key(PhysicalKeyCode::Enter, Action::ShowResults)?;
app.add_world_replacement_on_press_system();
```

The adapter is an ordinary fallible `FixedUpdate` System at its registration
position. A World without `WorldReplacementOnPress` is a successful no-op, so
the same application-level schedule works in the destination. Every matching
press occurrence forwards its original transition-intent token and consumes
one command slot; retained fixed edges therefore survive zero-tick frames, and
two physical presses mapped to the same action keep the existing convergent
transition diagnostic. Later Systems in that fixed tick still run before the
barrier, after which the normal replacement state machine decides whether to
commit, reject, or preserve the old World.

The Resource allows one unconditional action-target route per World. Replace
the whole `Copy` value through `ResMut` in an earlier System to reroute it
atomically. Use an ordinary System for several destinations, conditions,
payloads, FrameUpdate timing, or application exit. The helper binds no key and
does not change transition arbitration, lifecycle, preparation failure, or
recovery behavior.

## Deferred component changes

`Commands::insert` attaches approved components to an existing managed entity,
and `Commands::remove::<B>` removes the explicitly named component types,
without changing its `LogicEntity` handle or unrelated state:

```rust,ignore
fn reveal_ball(
    ball: Single<LogicEntityRef, With<Ball>>,
    mut commands: Commands,
) -> LogicResult {
    commands.insert(
        ball.handle(),
        CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?,
    )?;
    Ok(())
}

fn hide_ball(
    ball: Single<LogicEntityRef, With<Ball>>,
    mut commands: Commands,
) -> LogicResult {
    commands.remove::<CircleVisual>(ball.handle())?;
    Ok(())
}
```

`Single<D, F>` is for an exactly-one System precondition: zero or multiple
enabled managed matches stop the stage before the function body runs. Use
`Query::single()` or `single_mut()` instead when those cardinalities are
expected branches that the System should handle itself.

The bundle becomes visible after the current stage barrier. Existing explicit
component types are replaced; required components are created only when they
are absent. Multiple inserts apply in call order, so the last explicit value
of one type wins. An explicitly inserted Transform or camera starts with snapped
interpolation, while attaching a visual or collider to an entity that already
has a moving Transform preserves that Transform's history. Inserting and
despawning the same entity in one stage is a batch error regardless of call
order, so neither structural change is applied. Removal does not cascade into
required components: removing a visual leaves its Transform available for
later reuse. The complete batch is rejected before structural mutation if its
final composition would retain a component without one of its requirements.
Insert and remove otherwise share one call order, so a later insert can
restore a removed component. Removing an absent approved component is a no-op.
The named [`Commands::disable`](#enable-and-disable-entities) and
`Commands::enable` operations should be used for whole-entity activity rather
than spelling it as generic component insertion/removal.

## Enable and disable entities

Disabling keeps an entity and all its data, but excludes it from ordinary
managed queries, overlap searches, and drawing after the current stage
barrier. Enabling makes it active again:

```rust,ignore
#[derive(Resource)]
struct SavedEnemy(LogicEntity);

fn hide_enemy(
    enemy: Res<SavedEnemy>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    commands.disable(enemy.0)
}

fn show_enemy(
    enemy: Res<SavedEnemy>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    commands.enable(enemy.0)
}
```

`Disabled` is a standard automatically approved component, so these commands
need no setup call or Bevy import. Save the `LogicEntity` handle before
disabling: a normal World `Query` or `Single` cannot rediscover an inactive
entity. The entity still counts toward the World entity limit and retains its
identity, components, and interpolation history. Repeated calls are safe;
mixed enable/disable calls follow ordinary command order, and every accepted
call still occupies one bounded Command slot even if its final state is
idempotent. Toggling and despawning the same entity in one stage rejects the
complete structural batch. This controls the whole entity; it is not a
visual-only hide or application pause.

The complete two-World example is
[`examples/moving_ball.rs`](examples/moving_ball.rs). Run it on the currently
supported Linux/Vulkan desktop path with:

```bash
cargo run --release --example moving_ball
```

For a more game-like example, `projectile_arena` combines WASD/arrow movement,
edge-triggered Space input, deferred projectile creation, lifetime and
collision logic, despawning, and a score Resource:

```bash
cargo run --release --example projectile_arena
```

The example deliberately keeps all behavior in ordinary Rust Systems. A
headless integration test drives the same setup through movement, firing, and
a hit without opening a window. Projectile motion uses the standard opt-in
linear-velocity System described below; firing and collision remain ordinary
game-specific Systems.

`coin_pickup` shows a smaller chain built around typed events: movement runs
first, collision detection queues a coin despawn and sends `CoinCollected`,
then a later system reads that event and updates the score.

```bash
cargo run --release --example coin_pickup
```

`click_to_place` places a circle for each left-button press with a known
pointer position, clears with right-click, and exits with Escape. It keeps a
static camera and at most 128 placed circles. Each press retains its own
logical position and viewport through delayed fixed updates. See the
[pointer input guide](DOCUMENTATION/guides/Pointer-Input.md) for the public API,
desktop geometry handling, and limits.

```bash
cargo run --release --example click_to_place
```

`LineVisual` draws a world-axis vector from an entity's interpolated
`Transform2d`. Its width is in logical screen pixels; the line uses butt caps,
so the body stops at the exact mathematical endpoints. A zero vector is valid
and draws nothing; the component can stay attached while a force, acceleration,
or velocity is idle:

```rust,ignore
let body = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?;
let vector = LineVisual::new(Vec2::ZERO, 3.0, Color::rgb8(255, 176, 70))?;
world.spawn((Body, LinearAcceleration2d::default(), body, vector))?;

fn show_acceleration(
    body: Single<(&LinearAcceleration2d, &mut LineVisual), With<Body>>,
) -> LogicResult {
    let (acceleration, mut line) = body.into_inner();
    line.set_vector(acceleration.acceleration())?;
    Ok(())
}
```

The line does not automatically follow a force, acceleration, or velocity
component; an ordinary ordered System performs that copy explicitly. It has no collider.
On an exact same-entity ordering tie, rectangles draw first, circles second,
and lines last. The complete `acceleration_vectors` example samples WASD/arrow input,
uses the standard acceleration-then-velocity composition, and presents its
current acceleration:

```bash
cargo run --release --example acceleration_vectors
```

Standard world-space visuals and colliders automatically add
`Transform2d::default()` when their bundle does not contain a transform. A
transform supplied explicitly keeps its canonical translation; normal spawn
snapping can set its previous interpolation endpoint to the current value.
This makes an omitted position mean "at the world origin", not "silently
inactive". To keep an entity inactive from its first frame, import the
automatically approved `sim_logic::bevy_ecs::entity_disabling::Disabled` and
include it in the spawn bundle. To change activity later, use
`Commands::disable` and `Commands::enable`.
Alternatively, do not attach the visual or collider component yet and keep its
value as template data, then attach it later with `Commands::insert`. A visual
or collider can likewise be detached with `Commands::remove::<T>` while
retaining the entity and its Transform.

## Opt-in movement

`DigitalMovement2d<A>` is the short path for ordinary constant-speed keyboard
movement. Each component owns its typed four-action axis and finite
nonnegative speed, so different entities can use different controls:

```rust,ignore
let movement = DigitalMovement2d::new(MOVEMENT, 8.0)?;
world.spawn((Player, movement, player_visual))?;

// Physical bindings and System order stay explicit.
app.bind_wasd_and_arrows(MOVEMENT)?;
app.add_digital_movement2d_system();
```

Input direction is normalized, so cardinal and diagonal movement use the same
speed. The component is inert until the adapter is registered. Registering it
twice moves matching entities twice; `Disabled` entities are excluded; and a
component inserted or spawned through `Commands` first moves on a later fixed
tick. The adapter does not perform collision response, facing, acceleration,
analog input, or hidden key binding.

## Opt-in linear acceleration

`LinearAcceleration2d` stores finite world-space velocity change per second.
Like velocity, it is inert until its standard FixedUpdate adapter is registered:

```rust,ignore
let acceleration = LinearAcceleration2d::new(Vec2::new(0.0, -9.81))?;
world.spawn((FallingBody, acceleration, body_visual))?;

// Registration order is the integration policy.
app.add_linear_acceleration2d_system();
app.add_linear_velocity2d_system();
```

The component supplies a zero `LinearVelocity2d`, which in turn supplies an
origin `Transform2d`, when those values are omitted. Acceleration-before-
velocity gives semi-implicit Euler: velocity changes first and position then
uses the new velocity. Reversing the calls deliberately moves with the old
velocity and updates velocity for the next tick. Duplicate registration applies
the corresponding step twice. Exact zero acceleration is a no-op; Disabled and
Commands-barrier behavior matches the other standard motion adapter.

This is acceleration in world units per second squared, not force. There is no
mass, gravity field, drag, collision response, angular motion, substep solver,
or automatic LineVisual synchronization.

## Opt-in linear velocity

`LinearVelocity2d` stores a finite world-space velocity in units per second.
It supplies a default origin `Transform2d` when needed, but deliberately does
nothing until its fixed System is registered:

```rust,ignore
let velocity = LinearVelocity2d::new(Vec2::new(10.0, 0.0))?;
world.spawn((Projectile, velocity, projectile_visual))?;

// Place this exactly where motion belongs among the other fixed Systems.
app.add_linear_velocity2d_system();
```

Each registration appends one ordinary fallible FixedUpdate System. Earlier
Systems may change velocity or position; later Systems see the integrated
translation in that same tick. Registering it twice moves matching entities
twice. A velocity inserted or spawned through `Commands` crosses the normal
stage barrier and first moves on a later fixed tick. Disabled entities do not
move.

The calculation is only `translation += velocity * fixed_delta`, using the
normal interpolated `Transform2d` path. Overflow becomes a normal System
failure and does not roll back entities already processed in that System.
There is no acceleration, mass, force, collision response, swept movement,
rotation, drag, or hidden physics scheduler.

## Circle overlap

Collision geometry is independent from drawing. Add `CircleCollider2d`, then
use a typed candidate filter in a fixed-update System:

```rust,ignore
fn detect_pickups(
    player: Single<(LogicEntityRef, &Transform2d, &CircleCollider2d), With<Player>>,
    coins: CircleOverlapEntities<With<Coin>>,
    mut commands: Commands,
    mut events: EventWriter<CoinCollected>,
) -> LogicResult {
    let (player, transform, collider) = player.into_inner();
    for coin in coins.iter_overlapping(player.handle(), transform, collider)? {
        commands.despawn(coin)?;
        events.send(CoinCollected { coin, points: 1 })?;
    }
    Ok(())
}

let body = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?
    .with_matching_collider();
world.spawn((Player, body))?;
```

The geometry arguments define the source position and shape; normal code
borrows them from the source entity as above. The runtime validates the source
handle but cannot prove that caller-supplied references came from that same
entity. When canonical component values are supplied, render interpolation
does not change collision results. Disabled entities and the source itself are
excluded, while tangent circles count as overlapping.
`with_matching_collider` returns the visual beside the same one-time collider
snapshot as `from_visual`; it does not keep drawing and collision geometry
synchronized. Use `CircleCollider2d::new` for an invisible or deliberately
different hit area.
The current implementation performs an allocation-free linear scan of the
ECS rows visited after the filter's archetype pruning for each call. Movement
and collision are normally separate ordered Systems; a System that also asks
for mutable access to candidate transforms or colliders needs filters that
prove those accesses disjoint. The helper is deliberately not presented as a
spatial index or a physics solver: there is no automatic response, event,
layer mask, broad phase, or continuous collision yet.

`CameraFollowTarget2d` is a small declarative path for the common one-target
camera. Put it on the followed entity and register its fixed adapter exactly
where the camera should sample target motion:

```rust,ignore
let offset = Vec2::new(0.0, 2.0);
let camera = ActiveCamera2d::new(Camera2d::new(offset, 28.0)?);
let follow = CameraFollowTarget2d::new(offset)?;
world.spawn(camera)?;
world.spawn((follow, player_movement, player_visual))?;

// The camera observes movement from this same fixed tick.
app.add_digital_movement2d_system();
app.add_camera_follow2d_system();
```

The component supplies a default origin `Transform2d` when needed. With no
enabled follow target, the adapter is a no-op, so a menu World can share the
same application schedule. One target follows; multiple targets fail instead
of choosing by ECS order. Disabled targets are ignored. Missing or duplicate
active cameras are left to the existing extraction diagnostic and repair path.
Commands-created targets first affect a later fixed tick.

The initial camera center stays explicit because the fixed adapter has not run
during World construction. Sim;Logic retains both fixed endpoints after it
does run, so the object and camera use the same presentation alpha instead of
visibly drifting apart. Registering camera follow before movement deliberately
samples the target's earlier canonical position.

Use `ActiveCamera2d::teleport_center` for an intentional jump. Teleporting a
follow target does not infer camera intent: when both must jump, the owning
fixed System must call `Transform2d::teleport` and `teleport_center` in the same
tick.

Mutable camera access belongs to `FixedUpdate`; `FrameUpdate` may read the
canonical camera but cannot request `&mut ActiveCamera2d`. A structural camera
replacement through `Commands` is still allowed and starts snapped. The
canonical value is suitable for fixed logic and diagnostics, not exact
presentation-space picking, because it can be ahead of the rendered camera.
`ExtractedFrame::camera()` returns the exact camera for a published snapshot;
an in-World presentation-camera input is not public yet.

```bash
cargo run --release --example camera_follow
```

## Circle and rectangle collision

`CircleVisual` and `RectangleVisual` use `Transform2d` as their center.
Rectangles keep full width and height, are filled and axis-aligned; rotation,
scale, and stroke are not available in this slice. Drawing remains independent
from collision: attach the matching collider only when an entity needs a hit
area.

```rust,ignore
let body = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?
    .with_matching_collider();
world.spawn((Player, body))?;

let wall = RectangleVisual::rounded(
    Vec2::new(0.7, 12.0),
    Color::rgb8(54, 68, 92),
    0.2,
)?.with_matching_collider();
world.spawn((Wall, wall))?;
```

Circle colliders store a radius; rectangle colliders store full width and
height. Both use the supplied transform center and treat contact as overlap.
`with_matching_collider` packages the visual with the same one-time geometry
snapshot as `from_visual`; later visual and collider changes remain independent.
A rectangle collider deliberately ignores visual corner radius, so an invisible
collider or a differently sized hit area still uses `RectangleCollider2d::new`
without special cases. `overlaps` performs the same-shape test.
`CircleCollider2d::overlaps_rectangle` and
`RectangleCollider2d::overlaps_circle` use one symmetric mixed-shape
predicate. Response remains ordinary System code. There is no rotation,
sliding, swept collision, broad phase, or automatic physics.

`RectangleOverlapEntities` removes the repeated query-and-scan plumbing while
leaving the movement rule in ordinary Rust. The source geometry is explicit,
so a System can test a copied proposed transform before changing the real one:

```rust,ignore
fn move_player(
    player: Single<
        (LogicEntityRef, &mut Transform2d, &CircleCollider2d),
        (With<Player>, Without<Wall>),
    >,
    walls: RectangleOverlapEntities<(With<Wall>, Without<Player>)>,
) -> LogicResult {
    let (player, mut transform, collider) = player.into_inner();
    let proposed = transform.translated_by(Vec2::new(0.25, 0.0))?;

    let blocked = walls.has_overlap_with_circle(
        player.handle(),
        &proposed,
        collider,
    )?;
    if !blocked {
        transform.set_translation(proposed.translation())?;
    }
    Ok(())
}
```

As with the circle helper, the runtime validates the source handle, excludes
the source and disabled entities, and applies the typed filter only to
candidates. Each call is an allocation-free linear scan after ECS archetype
filtering, with no stable result order. If the same System mutably queries
source transforms, its source filter and the helper's candidate filter must
prove those archetypes disjoint.

Rectangles, circles, and lines share one deterministic order: layer, draw-order
depth, stable entity identity, then primitive kind. On an exact same-entity tie
the order is rectangle, circle, then line. Per-kind CPU limits are separate,
while the Scene command budget limits their combined emitted count. Zero-vector
lines count toward the line-source limit but emit no command. Invalid or
over-budget mixed extraction never publishes a partial snapshot. The runner
keeps one published snapshot and one private work buffer; after both have
reached a stable scene size, ordinary visual extraction reuses retained
storage.

The `rectangle_room` example moves a circular player inside four rectangular
walls. Its fixed System tests a proposed position with the mixed-shape helper
and either commits the entire movement or rejects it. This small policy can
tunnel across a wall when one unusually large step starts and ends outside,
and it does not slide; the example keeps those limits visible instead of
presenting the check as a physics solver.

```bash
cargo run --release --example rectangle_room
```

## Application and World Resources

Use `Res<T>` and `ResMut<T>` for state that belongs to one World. Use
`AppRes<T>` and `AppResMut<T>` for state such as session progress or user
settings that must remain when that World is replaced:

```rust,ignore
use sim_logic::prelude::*;

#[derive(Default)]
struct SessionScore {
    points: u32,
}

#[derive(Resource)]
struct RoomState;

fn award_point(
    room: Option<Res<RoomState>>,
    mut score: AppResMut<SessionScore>,
) {
    if room.is_some() {
        score.points = score.points.saturating_add(1);
    }
}

fn show_score(score: AppRes<SessionScore>) {
    println!("{}", score.points);
}

let mut app = Application::<Action>::new(AppConfig::default())?;
app.register_app_resource(SessionScore::default())?;
app.add_fixed_system(award_point);
app.add_frame_system(show_score);
```

The distinction is part of the type signature: `Res<RoomState>` disappears
with the old World, while the same `SessionScore` value moves into the new
installed World without requiring `Clone` or a Bevy `Resource` derive.
Application Resource types are explicitly registered before startup and their
count has a configurable finite limit.

Candidate factories and Startup systems receive no `AppRes` or `AppResMut`
access to live Application Resources. External aliases such as an `Arc` held
by both a resource and a factory remain ordinary Rust side effects and are
forbidden by the factory contract; the runtime cannot revoke aliases it did
not issue. A setting that affects candidate construction therefore cannot use
`AppRes` in the current no-payload transition slice. Direct `AppResMut` changes
in an active FixedUpdate or FrameUpdate are not transactional: like `ResMut`
changes, they remain if a later system or candidate preparation fails.

`AppRes::is_changed` and `is_added` expose the usual ECS change hints. Their
tracking belongs to an ECS World, so a value transferred into a replacement is
reported as newly installed there even though `T` itself was not cloned or
reset.

The same-frame snapshot of a committed replacement is the candidate's fully
validated, AppRes-independent visual state. Ordinary B systems begin using the
transferred resources on the next FixedUpdate or FrameUpdate. This avoids
letting provisional Startup depend on live state; a later WorldEnter slice may
add a separate post-commit activation phase. Panics, including a panic from a
World value's `Drop` during commit, are outside typed recovery; catching one
does not make the runner safe to resume.

The complete `persistent_score` example adds a point in World A, replaces it
with World B, and reads the same score there:

```bash
cargo run --release --example persistent_score
```

## World events

Event types are ordinary `Copy` values that application code should keep
compact. Sim;Logic discovers their typed channels from System parameters, so
there is no separate registration call:

```rust,ignore
#[derive(Clone, Copy)]
struct CoinCollected {
    coin: LogicEntity,
    points: u32,
}

fn detect_pickups(
    coins: Query<LogicEntityRef, With<Coin>>,
    mut commands: Commands,
    mut events: EventWriter<CoinCollected>,
) -> LogicResult {
    // Position checks are omitted here; the complete example filters collisions.
    for coin in &coins {
        commands.despawn(coin.handle())?;
        events.send(CoinCollected {
            coin: coin.handle(),
            points: 1,
        })?;
    }
    Ok(())
}

fn apply_score(events: EventReader<CoinCollected>, mut score: ResMut<Score>) {
    for event in &events {
        score.points += event.points;
        score.last_coin = Some(event.coin);
    }
}

app.add_fallible_fixed_system(detect_pickups);
app.add_fixed_system(apply_score);
```

An event is visible only to later Systems in the same `Startup`, individual
fixed tick, or `FrameUpdate` invocation. Reading is repeatable and does not
consume the event. Each event type has a configurable record limit; a failed
send stops the stage even if its `Result` was ignored, and pending `Commands`
are discarded. The limit counts records rather than bytes and does not enforce
a compact Rust type. Event values are restricted to `Copy` data; applications
should prefer small values and opaque handles.

`EventReader<E>` and `EventWriter<E>` must not be requested together in one
System because they borrow the same channel. When one System needs both
operations, use `EventWriter<E>::iter` before sending more values.

Fallible systems are an execution boundary, not an ECS transaction. An `Err`
skips later systems in that stage and discards its pending structural
`Commands`, while direct component and Resource writes already made by the
stage remain. Atomic consistency between direct writes and `Commands` is not
available in this slice: early validation can reduce partial writes inside one
system, but a later system error or stage-barrier rejection can still discard
the commands.

## Headless tests

The same `Application` setup can run without a window, surface, or GPU. Replace
the final `app.run(initial)` with an explicit frame driver:

```rust,ignore
let mut runner = app.build_headless(initial)?;
let events = [InputEvent::key(
    PhysicalKeyCode::KeyD,
    ButtonState::Pressed,
)];
let viewport = LogicalViewport::new(800.0, 600.0)?;

let outcome = runner.advance_frame(FrameRequest::new(
    std::time::Duration::from_millis(17),
    &events,
    viewport,
));

let FrameOutcome::Advanced(report) = outcome else {
    panic!("the bounded frame request was rejected");
};
assert!(report.failure().is_none());
let (ball, _) = runner
    .components::<Ball>()
    .next()
    .expect("the World should contain one ball");
assert!(runner.component::<Transform2d>(ball)?.translation().x() > 0.0);
```

Use `--no-default-features` when a consumer needs the headless dependency path
without compiling the desktop window and GPU stack.

## Performance check

The repository includes a dependency-free release benchmark with separate
full-frame, focused subsystem, and allocation-gate modes:

```bash
cargo bench --bench headless_runtime --no-default-features
cargo bench --bench headless_runtime --no-default-features -- --overlap-only
cargo bench --bench headless_runtime --no-default-features -- --rectangle-overlap-only
cargo bench --bench headless_runtime --no-default-features -- --mixed-overlap-only
cargo bench --bench headless_runtime --no-default-features -- --visual-only
cargo bench --bench headless_runtime --no-default-features -- --motion-only
cargo bench --bench headless_runtime --no-default-features -- --acceleration-motion-only
cargo bench --bench headless_runtime --no-default-features -- --digital-motion-only
cargo bench --bench headless_runtime --no-default-features -- --insert-only
cargo bench --bench headless_runtime --no-default-features -- --remove-only
cargo bench --bench headless_runtime --no-default-features -- --enablement-only
cargo bench --bench headless_runtime --no-default-features -- --exit-only
cargo bench --bench headless_runtime --no-default-features -- --pointer-only
cargo bench --bench headless_runtime --no-default-features -- --input-cancellation-only
```

The cancellation gate checks source, pointer, cause, and token identity across
frame delivery and delayed fixed ticks. After warm-up it requires zero allocator
calls across 100 frames alternating pointer leave and focus loss. This is a
headless CPU check, not a measurement of desktop event delivery or GPU work.

It reports World build and steady-state frame time for 0, 100, 1,000, and
10,000 circles, plus managed-idle scale, balanced entity churn, and typed event
throughput. It also runs matched `ResMut` and `AppResMut` counter cases through
the same complete frame path. Warmed background-only, circle-only,
rectangle-only, line-only, zero-vector line-source, two-kind mixed, and
three-kind mixed extraction cases additionally require zero allocator calls
across 100 stable-size frames. The visual-only suite times 10,000 sources and
checks exact 120-frame cardinality checksums; zero-vector lines exercise
validation and source limits while emitting no resolved record or Scene
command. The warmed one-transform benchmark World also requires zero runtime
allocator calls across 100 frames with either one or four fixed ticks per
frame; cached interpolation queries still discover archetypes introduced by
Commands. User systems remain outside that allocation-free claim. Three warmed
motion gates exercise 10,000 entities: constant velocity, explicit
acceleration-then-velocity composition, and typed input-driven movement.
The acceleration case independently replays every fixed step and checks the
exact velocity plus current and previous Transform endpoints for every entity;
all warmed motion gates require zero allocator calls across 100 frames. Four
input shapes, each exercised with W and
ArrowUp, require zero runtime allocator calls for held and alternating-edge
input across zero-, one-, and four-tick frames while exact checksums verify
that both update stages still receive the intended snapshots. A separate
pointer gate holds all 14 supported physical controls on distinct actions,
moves the pointer, and releases the mouse buttons through a leave event. Its
100 warmed cycles each run one-, zero-, and four-tick frames, require zero
allocator calls, and verify exact delivery checksums. This measures the
headless input/frame path, not desktop event collection or object spawning.
A separate
four-wall circle-against-rectangle collision gate requires zero allocator calls
across 100 warmed fixed frames and verifies both the blocked-branch checksum
and final mover position. The exit-only gate checks that a warmed payload-free
exit command allocates nothing and intentionally performs no extraction. The
overlap-only command reports one-query sparse-X scaling at 100, 1,000, and
10,000 candidates for both collider shapes, then repeated 100-by-10,000 scans
of dense, tangent, sparse, near-miss, and mixed layouts. The rectangular and
mixed-shape suites also require zero allocator calls across 100 warmed full
frames scanning 10,000 dense candidates once per frame. Mixed collision covers
circle-to-rectangle layouts plus a reciprocal rectangle-to-circle scan. This
makes the repeated-linear-scan cost and closed collision boundaries visible
without implying a broad phase.
The remove-only gate alternates
refill frames outside measurement with 1,000 actual removals per measured frame;
after the command storage, final-state planner, and Bevy archetype route are
preconditioned, 100,000 trivial component removals must allocate nothing while
leaving entity counts unchanged.
The enablement-only gate alternates 1,000 retained handles between enabled and
disabled archetypes. After both routes and command/planner storage are warm,
100,000 named toggle commands must allocate nothing, retain all entities, and
finish with every target enabled.
Timing results are informational and intentionally have no machine-specific
pass threshold.

The focused insert case replaces a non-zero-sized component on 1,000 stable
entities. After warm-up, 100 frames must perform exactly the 100,000 boxed
payload allocations implied by 100,000 queued inserts, with no extra command
or validation-storage allocations. It also compares the complete insert frame
against an otherwise matching read/checksum frame and verifies exact final
component values and unchanged spawn/despawn counts.

## License

Licensed under either Apache-2.0 or MIT, at your option.

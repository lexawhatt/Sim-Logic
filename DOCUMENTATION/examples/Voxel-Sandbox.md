# Twin Fields: a two-region voxel sandbox

[Documentation](../README.md) / Examples

Build, dig and walk between a green meadow and a sandy canyon. This is an
original finite building game, not Minecraft compatibility or a full survival
game. It exercises real World replacement while keeping the edited terrain
and inventory in application-owned Rust data.

```bash
cargo run --release --features text --example voxel_sandbox
```

Requires a working desktop GPU backend. The existing licensed DejaVu test font
is embedded; there are no downloaded game assets. Sound is not enabled.

## Renderer limitation

**This prototype is not yet a reliable free-camera release.** A native camera
traversal reproduced Engine 0.3.0's `UnportableSurfaceTopology` rejection in a
valid generated chunk. The desktop runner currently exits on that rendering
error; unsaved edits are then lost. Startup and resize passed, but that does
not establish safe navigation through every camera pose.

A small camera-coordinate offset was tested and did not fix the rejection;
it is not part of the game. The failure is inside Engine's conservative
projected-triangle orientation validation, not World persistence or voxel
collision. Fixing that rendering capability requires an Engine update. Keep
manual saves and treat this as an integration prototype until then.

## Controls

| Input | Action |
| --- | --- |
| W/A/S/D | Walk forward, left, backward, right |
| Arrow keys | Look horizontally or vertically |
| Middle mouse drag | Look without locking the OS pointer |
| Space | Jump |
| Left mouse in the play area | Break the block under the center crosshair |
| Right mouse in the play area | Place the selected block on that face |
| 1-5 or bottom buttons | Select grass, stone, wood, sand or leaves |
| N or Other world | Travel to the other region |
| P or Pause | Pause/resume; the interface remains active |
| F5 / F9 | Save / load both regions and inventory |
| Escape | Exit |

The mouse position decides whether a click belongs to the UI or play area.
The center crosshair decides which block is edited. UI clicks require a press
and release on the same eligible button; a cancelled or dragged-out gesture
does not build through the interface. Building is limited to six blocks of
reach and cannot overlap the player's body. Blocks are finite: placement
consumes one, breaking returns one, and each material slot holds at most 999.
The bottom stone layer and horizontal world boundaries cannot be crossed.

## Saving

There is no automatic load or autosave. F5 writes `voxel-sandbox.save` in the
working directory. To choose a different file:

```bash
cargo run --release --features text --example voxel_sandbox -- /path/to/my.save
```

The default save and its temporary files are ignored by this repository's Git
rules; copying the game to someone else does not automatically include your save.

The small versioned format contains both terrains, inventory, selected block
and each region's last player position and view. It contains no ECS or GPU
handles. Loading checks the exact byte count, checksum, block values, inventory
bounds and safe player positions before requesting a replacement. The checksum
detects ordinary corruption; it is not authentication.

Saving validates first, writes and syncs a newly created sibling temporary file,
then renames it over the destination. Earlier failures keep the old save. A
foreign temporary file is not overwritten or removed. The directory is not
fsynced, so this is not a full power-loss durability guarantee. File errors
appear in the HUD and do not replace the running game.

## What belongs where

- [model/](../../examples/voxel_sandbox/model/) owns seeded terrain, voxel ray
  traversal, collision, inventory, exposed-face meshing and the save format.
- [app.rs](../../examples/voxel_sandbox/app.rs) routes input and fixed movement.
- [scene.rs](../../examples/voxel_sandbox/scene.rs) constructs valid loading Worlds.
- [projection.rs](../../examples/voxel_sandbox/projection.rs) publishes dirty chunks
  as [managed meshes](../rendering/ThreeD.md#host-built-meshes-and-editable-chunks).
- [view.rs](../../examples/voxel_sandbox/view.rs) and
  [presentation.rs](../../examples/voxel_sandbox/presentation.rs) draw the HUD,
  camera and selected-block outline.

Sim;Logic supplies ECS, ordered systems, fixed time, Commands, input ownership,
typed edit events, Application Resources, World replacement, managed text and
the 3D bridge. Sim;Engine owns the actual mesh rendering, clipping and depth
buffer. Directional face colors are baked by the game's mesher; they are not
a new lighting engine.

## Loading and update order

There is one active ECS World, not two simultaneous schedules. Both regions'
canonical block arrays live in an Application Resource. A factory captures
only an immutable UI recipe and region identifier; it does not read live
application data or capture a mutable lock.

After replacement, a valid loading screen appears. A following FrameUpdate
builds the bounded projection from saved data. A later frame confirms the
committed chunk stamps, enables the 3D view and opens gameplay. Until then,
movement and editing are disabled. An empty chunk also has a stamp, so it is
distinguishable from a failed creation batch. Returning restores edits, not a
freshly generated substitute map.

UI and game actions have one FrameUpdate owner. Accepted movement reaches
subsequent fixed ticks; FrameUpdate cannot undo a tick that already ran.
The player uses fixed-step collision; its 3D camera is not interpolated yet.
Pause or leaving a World clears pending controls, and old-frame actions after
a travel request are ignored. Continuous physical held state follows the
runtime's normal policy; old press occurrences are not replayed in the new World.

## Bounds and current limits

Each region is 24 x 16 x 24 cells, split into eighteen 8-cube chunks. Only exposed
faces are emitted, grouped by material and top/side/bottom shade: at most 270
mesh objects, not one entity per block. Changed chunks and their affected
boundary neighbors rebuild; unchanged chunks keep their shared mesh assets.
Both regions remain in memory, but only the active one has a render projection.

This example deliberately has no infinite streaming, textures, transparency,
lighting, crafting, multiplayer, sound or general drag-and-drop inventory.
The bottom buttons are a small hotbar, not a full widget toolkit. Layout is
responsive; extremely small windows can still clip fixed-size font labels.
The game does not claim to exercise every optional feature in the framework.

The shared model and runtime scenarios can run without a window:

```bash
cargo test --no-default-features --features text --test voxel_sandbox
```

The `text` feature currently compiles Engine's GPU dependencies even for these
windowless tests; it does not create a device. Use release mode when playing.

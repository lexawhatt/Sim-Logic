# Twin Fields: a two-region voxel sandbox

[Documentation](../README.md) / Examples

Build, dig and walk between a green meadow and a sandy canyon. This is an
original creative building game, not Minecraft compatibility or a full survival
game. Terrain streams around the player; edits and the nine-slot hotbar survive
travel between both worlds. The inventory contains 32 original block materials.

```bash
cargo run --release --features text --example voxel_sandbox
```

Requires a working desktop GPU backend. The existing licensed DejaVu test font
is embedded; there are no downloaded game assets. Sound is not enabled.

## Renderer limitation

This prototype pins Engine `0.4.0-dev.5`, exact Git revision `98b2c4d7`, and
explicitly selects `ThreeDSurfacePolicy::Native` for the free camera. The
library's default remains StrictPortable. No camera-coordinate workaround or
second renderer is used.

This is a development integration, not the final Engine release. A one-sided
block is intentionally invisible from inside. Transparent panes are separate
objects; intersecting transparent surfaces and triangles within one mesh are
not correctly sorted in general. Ordinary mipmaps can reduce leaf-mask coverage
at a distance. Keep manual saves: other fatal rendering errors still exit.

## Controls

| Input | Action |
| --- | --- |
| W/A/S/D | Walk forward, left, backward, right |
| Arrow keys | Look horizontally or vertically |
| First click in the play area | Capture and hide the mouse; this click does not edit |
| Mouse motion | Look while captured |
| Middle mouse drag | Fallback look if OS capture is unavailable |
| Space | Jump; double-tap within 280 ms to toggle creative flight |
| Space / Left Shift while flying | Rise / descend |
| Left mouse while captured | Break the block under the center crosshair |
| Right mouse while captured | Place the selected block on that face |
| 1-9 or unlocked hotbar buttons | Select one of nine hotbar slots |
| E or F5 | Open/close the block inventory; click a material to assign the active slot |
| F3 | Toggle detailed diagnostics |
| F4 | Show/hide the separate renderer study panels (hidden by default) |
| N or Other world | Travel to the other region |
| P or Pause | Pause/resume; the interface remains active |
| F7 / F9 | Save / load both regions and inventory |
| L | Compare Lambert lighting with Unlit colors |
| F | Toggle distance fog |
| M | Toggle complete texture mip chains |
| T | Paint a small region of the gray study board's texture |
| V | Switch perspective / orthographic projection |
| Escape | Open/close the pause menu; Quit game exits |

An open menu pauses movement and frees the OS cursor. Returning to gameplay
explicitly requests capture again. Losing window focus releases capture and
opens the pause menu; focus return alone never grabs the mouse. Platforms may
reject capture; its actual status appears in F3 and arrow/middle-drag look
remain available. Relative mouse motion is applied once per frame, without
delta-time multiplication or fixed-tick replay.

The center crosshair decides which block is edited. UI clicks require a press
and release on the same eligible button; a cancelled or dragged-out gesture
does not build through the interface. Building is limited to six blocks of
reach and cannot overlap the player's body. Creative mode supplies unlimited
blocks. The model also retains a finite-inventory mode for tests: placement
consumes one, breaking returns one, and each material holds at most 999.
Flight moves at seven blocks/second, has no gravity, and retains collision.
Menus clear movement but preserve the flight toggle within that World. Flight
mode resets on travel/load; airborne positions are saved. The bottom stone
layer, horizontal world boundary and 64-block flight ceiling cannot be crossed.

## Saving

There is no automatic load or autosave. F7 writes `voxel-sandbox-v2.save` in the
working directory. To choose a different file:

```bash
cargo run --release --features text --example voxel_sandbox -- /path/to/my.save
```

The default save and its temporary files are ignored by this repository's Git
rules; copying the game to someone else does not automatically include your save.

The version-2 format stores the seed, changes to generated terrain in both
regions, inventory, hotbar and each region's last player position and view.
It contains no ECS or GPU handles. Loading checks bounded size, checksum,
sorted unique edits, block values, inventory
bounds and safe player positions before requesting a replacement. The checksum
detects ordinary corruption; it is not authentication.

Saving validates first, writes and syncs a newly created sibling temporary file,
then renames it over the destination. Earlier failures keep the old save. A
foreign temporary file is not overwritten or removed. The directory is not
fsynced, so this is not a full power-loss durability guarantee. File errors
appear in the HUD and do not replace the running game.

Old `SVXLS001` finite-world files are deliberately not reinterpreted. Loading
reports an unsupported version, and saving refuses to replace a legacy file.
Keep that file and use the earlier example to open it; automatic migration is
not implemented. The new default filename leaves the old default untouched.

## What belongs where

- [model/](../../examples/voxel_sandbox/model/) owns seeded terrain, voxel ray
  traversal, collision, inventory, exposed-face meshing and the save format.
- [app.rs](../../examples/voxel_sandbox/app.rs) wires the application and fixed
  movement; [app/routing.rs](../../examples/voxel_sandbox/app/routing.rs) owns
  ordered gameplay/menu input.
- [scene.rs](../../examples/voxel_sandbox/scene.rs) constructs valid loading Worlds.
- [projection.rs](../../examples/voxel_sandbox/projection.rs) publishes dirty chunks
  as [managed meshes](../rendering/ThreeD.md#host-built-meshes-and-editable-chunks).
- [view.rs](../../examples/voxel_sandbox/view.rs) and
  [presentation.rs](../../examples/voxel_sandbox/presentation.rs) draw the HUD,
  camera and selected-block outline.

Sim;Logic supplies ECS, ordered systems, fixed time, Commands, input ownership,
typed edit events, OS pointer capture, Application Resources, World replacement, managed text and
the 3D bridge. Sim;Engine owns the actual mesh rendering, clipping and depth
buffer, normal-based illumination, fog, mip generation and GPU uploads.
The mesher supplies face normals, UVs and a small vertex tint. Thirty-two independent
procedural tiles need no downloaded assets. Leaves use Mask; two floating
study panes use Blend. The panels have no collision or saved block identity.
The gray board starts with the terrain's shared stone texture: painting it
isolates the board, then repeated patches can reuse its GPU allocation.

## Loading and update order

There is one active ECS World, not two simultaneous schedules. Both regions'
seeded terrain and sparse edits live in an Application Resource. A factory captures
only an immutable UI recipe and region identifier; it does not read live
application data or capture a mutable lock.

After replacement, a valid loading screen appears. A following FrameUpdate
progressively builds the bounded projection from saved data. A later frame confirms the
committed chunk stamps, enables the 3D view and opens gameplay. Until then,
movement and editing are disabled. An empty chunk also has a stamp, so it is
distinguishable from a failed creation batch. Returning restores edits, not a
freshly generated substitute map.

UI and game actions have one FrameUpdate owner. Accepted movement reaches
subsequent fixed ticks; FrameUpdate cannot undo a tick that already ran. The
explicit focus-loss boundary also reaches FixedInput, so catch-up movement
stops before the frame router opens the menu.
The player uses fixed-step collision; its 3D camera is not interpolated yet.
Pause or leaving a World clears pending controls, and old-frame actions after
a travel request are ignored. Continuous physical held state follows the
runtime's normal policy; old press occurrences are not replayed in the new World.

## Bounds and current limits

Each region covers coordinates `[-4096, 4096)` in X/Z and `[0, 16)` in Y.
Only a 5 x 5 column neighborhood of 8-cube chunks is retained: at most 50
chunks per region, not the entire map. After the bounded initial bootstrap,
at most two chunks generate and two chunks mesh per frame. Collision and ray
picking use the same seeded rules even before a render chunk is ready.

Only exposed faces are emitted, grouped by material and top/side/bottom shade,
not one entity per block. Changed chunks and affected boundary neighbors
rebuild; surviving material/shade
parts retain their entities and Engine object IDs across mesh revisions.
Unchanged chunks keep their shared mesh assets.
Eviction removes cached blocks and render objects, never canonical edits.
Each region permits 16,384 changed cells; reaching that cap rejects the next
new edit without consuming inventory. Restoring a generated cell frees its
edit entry. Saves are bounded to 426,133 bytes. Both regions keep their state,
but only the active one has a render projection.

This example deliberately has no unlimited world or storage, crafting, multiplayer,
sound or general drag-and-drop inventory.
The inventory assigns slots; it is not a drag-and-drop widget toolkit. Layout is
responsive; extremely small windows can still clip fixed-size font labels.
The game exercises new Engine 0.4 capabilities, not every optional feature in
the framework. Lighting is ambient plus one directional source: no shadows,
point lights or physically based materials.

F3 reports player coordinates, signed chunk position, facing, seed, flight and
ground state, resident/pending/generated chunks, retained edits, source mesh
counts, render switches, target block and requested/actual mouse capture.
Values refresh every 250 ms. The displayed host-frame interval is unscaled
wall time, not CPU execution time, GPU time or a confirmed-present FPS counter.

## Bounded integration drive

```bash
cargo run --release --features text --example voxel_sandbox -- --test-drive
```

This mode runs 128 camera poses per region, including far positive/negative
streaming destinations, travels once, edits terrain and the
board texture, toggles the presentation modes, and checks a save round-trip
in memory. It closes itself and rejects skipped or incomplete presentation.
It does not load or overwrite your save. The final report separates dynamic
mesh/texture uploads from initial creation and prints asynchronous GPU pass
diagnostics. Missing GPU samples are not reported as zero cost.

This is a smoke drive, not a pixel oracle or an FPS guarantee. Standalone
Engine recovery has a separate manual regression in
[engine_dev4_gpu.rs](../../tests/engine_dev4_gpu.rs). The unchanged test passes
on dev.5 with NVIDIA/Vulkan; Intel is not confirmed. The Logic adapter still
restores whole scenes to preserve shared resources and object IDs.

The shared model and runtime scenarios can run without a window:

```bash
cargo test --no-default-features --features text --test voxel_sandbox
```

The `text` feature currently compiles Engine's GPU dependencies even for these
windowless tests; it does not create a device. Use release mode when playing.

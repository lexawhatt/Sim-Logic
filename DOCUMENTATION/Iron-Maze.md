# Iron Maze

Iron Maze is a small first-person shooter example with an original map and
artwork drawn in code. Clear six sentries, collect supplies, and reach the
exit. It combines typed input, fixed simulation, screen drawing, and a real
level restart in one playable application.

From the repository, run:

```bash
cargo run --release --example iron_maze
```

The default `desktop` feature opens a window through Sim;Logic's desktop host.
See [getting started](Getting-Started.md) for the toolchain and headless setup.

## Controls and objective

| Control | Action |
| --- | --- |
| W / S | Move forward / backward. |
| A / D | Strafe left / right. |
| Left / Right arrow | Turn left / right. |
| Left mouse button or Space | Fire; hold to repeat at the weapon's cooldown. |
| Enter | Reset the level at the next fixed tick. |
| Escape | Exit the application. |

Aim with the center crosshair and turn with the arrow keys. The mouse button
fires in the facing direction; moving the pointer does not aim the weapon.
There is no mouse look or pointer capture, so keep the pointer over the game
window when firing with the mouse. Space offers the same fire action.

You start with 100 health and 60 rounds. Each sentry takes three hits. A shot
hits the nearest living sentry along its ray, and walls stop both shots and
line of sight. Holding fire still consumes ammunition and respects the
0.24-second shot cooldown; a quick click is retained until a fixed tick can
receive it.

Sentries pursue you when they can see you and attack at close range. They use
simple line-of-sight pursuit and wall sliding, without route planning around
hidden corridors. Walls and living sentries also block player movement.

Walk over supplies to collect them. Health packs restore 35 health, capped at
100; ammo packs add 24 rounds, capped at 99. A pack stays available while its
corresponding value is already full, and disappears once used.

The HUD shows health, ammunition, kills, and the objective. Use the minimap to
find the six sentries and the exit beacon near the lower-right corner, at map
coordinates `(13.5, 14.5)`. After all six sentries are defeated, approach the
beacon to win. Reaching it early does not complete the level. Zero health ends
the attempt. Win and death stop gameplay, while Enter and Escape remain
available on the result screen.

## How the picture is drawn

The example uses 2.5D raycasting: the map and movement are two-dimensional,
while wall distance determines the height of a vertical strip on screen.
The renderer casts 320 wall rays for a 640 by 360 virtual picture, draws the
visible parts of enemy and pickup shapes, then adds the weapon, HUD, and result
overlay. The picture scales to the window with borders when its aspect ratio
differs.

All of those shapes are ordinary managed `ScreenRectangleVisual` entities.
Sim;Engine draws the resulting screen scene. The example does not add a
general 3D renderer to Sim;Logic. Its small pixel font is also drawn from
rectangle patterns in the example; this does not introduce a public text or
font API.

A World owns a fixed pool of 8,192 rectangle entities and a reusable staging
buffer. FrameUpdate changes their geometry and color; unused slots become
transparent. Ordinary frames keep the same entities. If drawing exceeds the
staging capacity, it returns an error before updating the pool. This is a
practical limit on the example's detail and content. Reusing that storage is
not a promise of a particular frame rate or allocation-free GPU presentation.

There are no external Doom or Quake assets, new dependencies, or audio.
The map, sentries, weapon, materials, and glyphs are defined in the example.

## Where the code lives

| File | Responsibility |
| --- | --- |
| [main.rs](../examples/iron_maze/main.rs) | Window entry point and control instructions. |
| [mod.rs](../examples/iron_maze/mod.rs) | Input bindings, Systems, World factory, and restart/exit routing. |
| [game.rs](../examples/iron_maze/game.rs) | Canonical `GameState`: movement, firing, enemies, pickups, and win/death rules. |
| [level.rs](../examples/iron_maze/level.rs) | The bounded grid map, wall rays, clearance checks, and sliding. |
| [render.rs](../examples/iron_maze/render.rs) | Wall projection, shapes, HUD, viewport layout, and rectangle pool. |
| [font.rs](../examples/iron_maze/font.rs) | The example's small glyph patterns. |

The fixed System samples `FixedInput` and advances `GameState` with an explicit
step of approximately 1/120 second. FrameUpdate reads that state to prepare
the picture. A resized or skipped presentation does not decide hits, movement,
or scores. The player pose is drawn from the current fixed state; this custom
raycaster does not interpolate its `GameState` resource automatically.

Enter replaces the `GameState` value at the first fixed tick that receives its
press, then skips gameplay for that tick. Later catch-up ticks may act on
controls that are still held, but do not replay old press edges. The World
generation and rectangle pool stay unchanged, and FrameUpdate draws the reset
state using the current viewport. This avoids rebuilding thousands of screen
entities or briefly showing the default-size candidate view after a resize.
Other examples demonstrate World replacement; this game's restart is an
ordinary fixed System changing its own Resource.

These game rules stay in the example. Sim;Logic supplies input delivery,
scheduling, managed state, Commands, extraction, and desktop coordination;
it does not acquire shooter-specific rules.

## Check the same game without a window

The [integration tests](../tests/iron_maze.rs) import the same application and
game modules used by the desktop entry point:

```bash
cargo test --no-default-features --test iron_maze
```

They cover map connectivity and blocked rays, movement, firing and cooldowns,
enemy attacks, supplies, win/death, input delivery, fixed-boundary restart,
exit, and finite geometry across viewport sizes. The headless runner still
extracts visual state, so these tests can inspect the rectangle pool without
creating a window or submitting work to a GPU. Real desktop interaction and
rendering require their own checks.

The game also has a CPU-frame allocation check:

```bash
cargo bench --no-default-features --bench iron_maze
```

It warms the complete headless game, then measures 120 frames with held turn
and fire controls, two fixed ticks per frame, and extraction of the reusable
screen pool. It checks that simulation actually advances and requires zero
new allocation calls during those measured frames. The printed timing is
machine-dependent and does not include GPU submission or window event handling.

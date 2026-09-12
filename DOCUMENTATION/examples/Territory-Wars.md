# Frontier: territory conquest

[Documentation](../README.md) / Examples

Frontier is an original offline strategy demo on Sim;Logic, inspired by
[Territorial.io](https://territorial.io/tutorial). Grow an army, expand across
an island, and attack neighboring factions without leaving home undefended.
It uses neither Territorial.io assets nor its source code, and does not claim
to reproduce the original game's formulas or full feature set.

From the repository, run:

```bash
cargo run --release --example territory_wars
cargo run --release --example territory_wars -- --seed 5328204
cargo run --release --example territory_wars -- --debug
```

The default `desktop` feature opens the window. The optional seed is an unsigned
decimal integer; the default is `5328204`. See [getting started](../Getting-Started.md)
for the toolchain and platform setup.

The interface is designed for a 1440-by-900 logical-pixel window. Smaller
windows keep the layout letterboxed, but the bitmap inspector text becomes
harder to read; use the default size when showing the calculations.

`--demo` starts from a prepared battle after 24 simulated seconds of ordinary
expansion. `--debug` does the same with the inspector already open. Both leave
control to you once the window opens; there is no continuing autoplayer.

## Playing

Click unclaimed land to choose your starting position on the 96-by-64 map.
You and six bots each begin with 13 cells and 620 home troops. The island's
land is connected; water cannot be crossed. Bots receive income immediately
but wait ten simulated seconds before issuing their first orders.

Choose an attack percentage, then click neutral land or a rival's color.
The target owner must share a land border with you somewhere: the click selects
an owner, not an exact cell for an army to march toward. Space orders neutral
expansion without requiring a click. Each faction can have only one expedition
at a time, and sending another order while it is moving does not spend troops.

Start with 25% expansion and allow reserves to recover between attacks.
Troops sent away no longer defend your existing territory. Sending 100% can
gain land quickly but makes a neighboring counterattack much cheaper.

Control at least 70% of the island's land, or eliminate all six rivals, to win.
Losing your final cell ends the match. A faction's starting cell has no special
capture rule. Win and defeat freeze the game while restart and exit remain usable.

| Control | Action |
| --- | --- |
| Left mouse button | Choose a start, attack a selected owner, or use a HUD control. |
| Middle mouse drag | Pan the zoomed map. Begin inside the map; the HUD stays fixed. |
| Mouse wheel | Zoom around the cursor inside the map, from 100% to 800%. |
| V | Reset the map view without restarting the match. |
| Space | Expand into bordering neutral land. |
| Slider / Left and Right arrows | Set dispatch percentage; arrows change it by 5 points. |
| 1 / 2 / 3 / 4 / 5 | Set 10% / 25% / 50% / 75% / 100%. |
| P | Pause or resume; ordinary attack orders are disabled while paused. |
| R | Restart the same seed and choose a new starting position. |
| N | Generate the next seed and return to the start screen. |
| F3 | Toggle the read-only mechanics inspector. |
| F4 | Arm or disarm cheats; also opens the inspector. |
| F5 | With cheats armed, grant up to 50,000 troops, limited by available capacity. |
| F6 | With cheats armed, toggle new computer orders. |
| F8 | With cheats armed, give the player all land and end with victory. |
| F9 | Advance exactly one 100 ms tick while paused. |
| Escape | Close the window. |

F6 does not freeze the simulation: existing bot expeditions and income continue.
Using F5, F6, or F8 marks the run **ASSISTED** until a restart or new map, even
if cheats are subsequently disarmed. F3 only inspects state; F9 works without
arming cheats. Restart resets the pause, inspector, cheats, and assisted marker.
There is no saved match: closing the application discards the current game.

At 100% the whole map fits, so panning has no effect. Zooming and panning never
change game state and remain available while paused. Wheel events over the HUD
or letterbox do nothing. While MMB is held, left clicks cannot start a match,
attack, or activate HUD buttons. Leaving the window, losing focus, or receiving
a resized pointer sample cancels the drag; press MMB again to start a new one.
Restart and new map also reset the view. The footer shows the current zoom.

## Inspectable calculations

The simulation advances at 10 ticks per second. Income is credited once every
ten ticks, not once per displayed frame. For home troops `H`, owned cells `L`,
and deployed troops `D`, the next one-second payout is:

```text
territory income = 2 * L
interest         = floor(H / 25)
troop capacity   = 80 * L
credited income  = min(territory income + interest,
                      max(0, troop capacity - H - D))
```

Deployed troops count toward capacity but earn no interest. Territory losses
can leave an army temporarily above capacity; it receives no new income, but
existing troops are not silently deleted to enforce the cap. An eliminated
faction has no home troops, income, or surviving expedition.

An order moves `floor(H * percentage / 100)` troops from home into an
expedition. This transfer is not itself a combat expense. The expedition
must contain at least 12 troops. Neutral capture costs 12 troops per cell.
For enemy land, the cost is recalculated as fighting changes the defender:

```text
garrison = ceil(defender home troops / defender owned cells)
cell cost = 12 + garrison + floor(garrison / 4)
```

A successful capture also removes that garrison from the defender's home army.
An expedition captures at most six cells per tick, always along its faction's
current border. If it cannot afford the next defended cell, the final assault
spends its remaining troops without taking land. It removes up to
`max(0, remaining troops - 12)` troops from the defender's remaining reserve.
If no target border remains, surviving troops return home. Leftovers smaller
than the neutral-cell cost also return after a successful capture.

`floor(dispatched / 12)` estimates affordable neutral cells, not guaranteed
gains: another owner may block the remaining route. Enemy previews use the
current garrison density, which can change before the expedition finishes.
The inspector separates income, committed troops, actual latest-tick combat
spending and captures, and the bots' most recent decisions. These are game
statistics, not GPU profiling or a promise about frame rate.
The inspector's rectangle/image-item counts describe the previous drawn frame;
packed captions count as one image item, not one item per letter.

## Code and rendering boundaries

| File | Responsibility |
| --- | --- |
| [main.rs](../../examples/territory_wars/main.rs) | Seed argument and desktop entry point. |
| [app.rs](../../examples/territory_wars/app.rs) | Session state, bindings, and Sim;Logic Systems. |
| [interaction.rs](../../examples/territory_wars/app/interaction.rs) | Ordered gestures, fixed-screen controls, pause/restart precedence. |
| [navigation.rs](../../examples/territory_wars/navigation.rs) | Map transform, cursor-anchored zoom, bounded pan, and map clipping. |
| [simulation.rs](../../examples/territory_wars/simulation.rs) | Game state, validated orders, phases, and fixed-tick orchestration. |
| [economy.rs](../../examples/territory_wars/simulation/economy.rs) | Pure income and dispatch calculations, payout, capped debug grants. |
| [combat.rs](../../examples/territory_wars/simulation/combat.rs) | Pure cost estimates and bounded frontier capture. |
| [ai.rs](../../examples/territory_wars/simulation/ai.rs) | Staggered bot decisions and debug controls. |
| [terrain.rs](../../examples/territory_wars/simulation/terrain.rs) | Seeded integer map generation and connected-land selection. |
| [view.rs](../../examples/territory_wars/view.rs) | Managed visual pools; map and HUD drawing live in `view/`. |
| [drawing.rs](../../examples/territory_wars/drawing.rs) | Reusable drawing lists and the immutable bitmap caption atlas. |

The game rules have no renderer, ECS, wall-clock, or device dependency.
Sim;Logic owns input delivery, schedules, managed screen visuals, extraction,
and desktop coordination. Sim;Engine draws ordinary screen rectangles and image
regions. A small bitmap atlas contains example captions and digits; this adds
neither a new text library nor a custom renderer. Sim;Logic supplies generic
[ordered wheel input](../guides/Pointer-Input.md#wheel-scrolling); zoom speed,
map limits, gesture ownership, and fixed HUD policy belong to this example.

Restart replaces the example's `Session` data while keeping its active World
and visual pools. The simulation reuses its frontier buffer; rendering reuses
bounded drawing lists and ECS entities. This does not imply allocation-free
GPU presentation. The demo has no multiplayer, boats, save/load, sound, custom
map import, diplomacy, or full editor.

## Checks without a window

```bash
cargo test --no-default-features --test territory_wars
cargo bench --no-default-features --bench territory_wars
```

The test target imports the same example modules. Pure simulation tests cover
map connectivity, accounting, rejected-order atomicity, deterministic replay,
capture costs, exposed defenses, elimination, income limits, and debug controls.
A default-seed regression verifies victory using small orders spaced 1.5 seconds
apart without cheats; this is not a prediction of a human player's match time.
Runtime tests exercise input and extracted visuals without a window or GPU.
Desktop appearance and interaction require a separate live-window check.
The benchmark measures CPU game updates and extraction, with and without the
inspector. It does not count allocations or measure GPU presentation.

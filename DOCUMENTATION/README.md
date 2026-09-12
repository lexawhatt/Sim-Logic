# Sim;Logic documentation

Sim;Logic supplies the application loop, objects, input and update order.
Sim;Engine draws the result. Your application owns its game rules, scientific
models or other domain logic; rendered state is not the model itself.

These guides describe the current experimental API, which can still change.

## Start here

[Getting started](Getting-Started.md) walks through a moving ball, from running
a desktop example to testing the same application without a window.

## Application guides

- [Runtime](guides/Runtime.md): entities, Systems, Commands, fixed time,
  interpolation, pause and World replacement.
- [Input](guides/Pointer-Input.md): keys and mouse buttons, click-time positions,
  screen-to-world coordinates and cancellation on focus loss.
- [Audio output](guides/Audio-Output.md): optional device output, its lifetime,
  buffer settings and diagnostics.

## Drawing

- [Screen panels](rendering/Screen-HUD.md): rectangles that stay in place
  while the World camera moves, geometric hit tests and explicit button ownership.
- [Screen images](rendering/Screen-Images.md): assets, crop, tint, ordering,
  memory limits and the optional Ferris helper.
- [Screen text](rendering/Text.md): optional real-font labels, alignment,
  changing strings, limits and shared rendering caches.
- [Geometric 3D](rendering/ThreeD.md): cuboids, depth and switching views.

## Example applications

- [Frontier](examples/Territory-Wars.md): territory conquest, bots, separate game rules, and a mechanics inspector.
- [Iron Maze](examples/Iron-Maze.md): a small 2.5D shooter.
- [Piano roll](examples/Piano-Roll.md): editing, synthesized sound and a 2D/3D piano.

The [example sources](../examples/) also contain smaller applications focused
on individual features. Their catalog is in [getting started](Getting-Started.md#try-an-existing-application).

## Working on the library

[Code layout](internals/Code-Layout.md) explains where the implementation lives.
For exact signatures and error contracts, build the API reference:

```bash
cargo doc --no-deps --open
```

Add `--all-features` to include the optional audio, text and Ferris APIs.

## Current boundaries

There is one active World and one desktop window. Systems run sequentially;
World factories are synchronous and take no runtime payload. Behavior sugar,
an editor, a general UI/asset-loading toolkit and a `Faulted` recovery schedule
are not implemented. The topic guides describe their specific limits:
pointer buttons have no global focus or OS capture, audio is not a World-owned voice service,
and 3D does not yet load arbitrary models.

Headless use needs neither a window nor a GPU. Desktop examples have been
checked on Linux/Vulkan, not every platform. For the product overview and
individual helper recipes, see the [repository README](../README.md).

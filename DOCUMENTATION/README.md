# Sim;Logic documentation

Sim;Logic gives a Rust application a ready-made loop for input, objects,
updates, and drawing through Sim;Engine. You describe the application's data
and rules. The library handles when those rules run and how their results
reach the renderer. There is no visual editor or separate scripting language.

These pages describe the code available in this repository, not a promised
future API. The project is experimental and public APIs can still change.

## Start here

- [Getting started](Getting-Started.md): run a desktop example, then build and
  test a moving ball without a window.
- [How the runtime works](Runtime.md): objects, System order, Commands, input,
  fixed time, pause, and World replacement.
- [Screen-fixed panels](Screen-HUD.md): draw a simple status panel that stays
  in place while the World camera moves.
- [Pointer input](Pointer-Input.md): bind mouse buttons, preserve click-time
  coordinates, and place objects using an explicit camera.
- [Repository README](../README.md): individual helpers and their limits.
- [Examples](../examples): complete applications with ordinary Rust game logic.

For method signatures and detailed error contracts, build the API reference:

```bash
cargo doc --no-deps --open
```

## Where each project fits

| Project | Responsibility |
| --- | --- |
| Sim;Logic | Application state, input, update order, World changes, and preparing visual state. |
| Sim;Engine | Rendering, cameras, the GPU, and renderer recovery. |
| Sim;X | Scientific models, units, formulas, and rules specific to its applications. |

Generic movement and overlap helpers do not turn Sim;Logic into a physics
solver. Likewise, a rendered position is not the authoritative state of a
scientific model. Keep the model in components or resources and convert it to
visual data when needed.

## Current limits

There is one active World and one desktop window. Systems run sequentially.
World factories are synchronous and accept no runtime payload. There is no
Behavior interface, editor, general UI toolkit, asset-loading pipeline, or
application-level `Faulted` recovery schedule yet.

Pointer input covers cursor movement and left, right, and middle buttons.
There is no pointer capture, wheel, touch, pen, text entry, UI focus, or
built-in hit testing.

Headless tests need neither a window nor a GPU. The desktop examples currently
target the Linux/Vulkan path; this is not a claim that every platform and
renderer configuration has been tested.

# Sim;Logic

Sim;Logic helps you build interactive Rust applications without wiring up an
ECS, window loop, input handling and renderer integration every time.
You write ordinary Rust components and functions. Sim;Logic runs them;
[Sim;Engine](https://github.com/lexawhatt/Sim-Engine) draws the result.

It is useful for simulations, tools and games. It is not a visual editor,
a physics engine or a second renderer.

## What is included

- One active World, typed components and resources, sequential Systems, and
  deferred structural Commands.
- Fixed-step simulation, interpolation, pause, time scaling and World replacement.
- Keyboard and pointer input, scroll, cursor capture and headless input injection.
- World-space 2D visuals; screen geometry, images, text, clipping and UI helpers.
- A retained 3D bridge for host-built meshes, materials, lighting and textures.
- Optional audio output, explicit resource limits and presentation diagnostics.

The current dependency is Sim;Engine **0.4.2**, pinned exactly.
The 0.1 API is pre-1.0; future minor releases may change it.

## Install

Rust **1.95 or newer** is required. The manifest below targets the first
crates.io release; until it is published, use a checkout path instead of
`version`.

```toml
[dependencies]
sim-logic = "0.1.0"
```

The default `desktop` feature includes the window and GPU adapter. For a
windowless application or test:

```toml
sim-logic = { version = "0.1.0", default-features = false }
```

| Optional feature | Adds |
| --- | --- |
| `fonts` | Engine's CPU font loading, shaping and rasterization. |
| `headless-text` | Managed fonts and text preparation, without a GPU. Includes `fonts`. |
| `text` | GPU text rendering. Includes `headless-text`, but not the desktop window host. |
| `audio` | Single-source audio device output through Rodio. |
| `easter-eggs` | The bundled Ferris `draw_crab` helper. |

Keep default features for the stock desktop host; add `features = ["text"]`
for labels. Font files belong to the application. On Linux, `audio` requires
ALSA development files and `pkg-config` at build time.

## A moving ball

```rust,no_run
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action { Up, Down, Left, Right }

const MOVEMENT: DigitalAxis2d<Action> = DigitalAxis2d::new(
    Action::Left, Action::Right, Action::Down, Action::Up,
);

fn main() -> LogicResult {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.bind_wasd_and_arrows(MOVEMENT)?;

    let camera = ActiveCamera2d::centered(32.0)?;
    let ball = CircleVisual::new(1.0, Color::rgb8(68, 144, 255))?;
    let movement = DigitalMovement2d::new(MOVEMENT, 8.0)?;
    let initial = app.register_world("main", move |world| {
        world.spawn(camera)?;
        world.spawn((ball, movement))?;
        Ok(())
    })?;

    app.add_digital_movement2d_system();
    app.run(initial)?;
    Ok(())
}
```

WASD and arrows move the ball; closing the window exits. No key has hidden
pause or quit behavior. The [getting-started guide](DOCUMENTATION/Getting-Started.md)
tests the same movement without opening a window.

## Run the examples

From this checkout:

```bash
cargo run --release --example moving_ball
cargo run --release --example territory_wars
cargo run --release --features text --example voxel_sandbox
cargo run --release --features audio --example piano_roll
```

The larger examples keep their game rules outside the library:

- [Twin Fields](DOCUMENTATION/examples/Voxel-Sandbox.md): two voxel worlds,
  building, inventory and creative flight.
- [Frontier](DOCUMENTATION/examples/Territory-Wars.md): territory conquest,
  bots, camera navigation and a mechanics inspector.
- [Piano roll](DOCUMENTATION/examples/Piano-Roll.md): editing, synthesis and
  a switchable 2D/3D piano.
- [Iron Maze](DOCUMENTATION/examples/Iron-Maze.md): a small 2.5D shooter.

## Learn more

Start with the [documentation index](DOCUMENTATION/README.md) or generate the
API reference with `cargo doc --all-features --no-deps --open`.
[Release checks](DOCUMENTATION/internals/Releasing.md) describe how to validate
the package. User-visible changes are in the [changelog](CHANGELOG.md).

There is one window and one active World. Systems run sequentially. World
factories are synchronous, without runtime payloads. There is no general
widget/layout toolkit, model importer or automatic collision solver.
[Runtime boundaries](DOCUMENTATION/guides/Runtime.md) and the rendering guides
describe failure handling and supported paths. Desktop verification currently
covers Linux/Vulkan; Windows has not been qualified by Sim;Logic.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Bundled third-party assets retain their own
[font](tests/assets/text/README.md) and
[Ferris](src/rendering/easter_eggs/assets/README.md) notices.

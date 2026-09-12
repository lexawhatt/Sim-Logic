# Getting started

[Documentation](README.md)

The checkout requires Rust 1.95 or newer. Its default `desktop` feature includes
the window and GPU dependencies. Use `default-features = false` when only the
headless runtime is needed.

## Try an existing application

From this repository:

```bash
cargo run --release --example moving_ball
```

Move with WASD or the arrow keys. Space pauses or resumes fixed simulation.
Enter replaces the blue World with the orange World while simulation is
running. Escape exits in either pause state. Closing the window also ends the
application.

The Enter policy runs during fixed updates. It therefore does nothing while
paused, and a successful fixed-stage replacement skips that frame's Space
policy. These choices belong to the example, not to the physical keys.

The complete setup is in [moving_ball.rs](../examples/moving_ball.rs). Other
examples cover individual features or combine them into a small game:

| Example | What it shows |
| --- | --- |
| `projectile_arena` | Firing, projectile lifetime, hits, and score. |
| `coin_pickup` | Passing a typed event from collision detection to score logic. |
| `rectangle_room` | Testing a proposed move against rectangular walls. |
| `camera_follow` | Following one moving entity with an interpolated camera. |
| `persistent_score` | Keeping Application Resources across World replacement. |
| `acceleration_vectors` | Ordered acceleration and velocity updates, plus vector drawing. |
| `screen_hud` | A screen-fixed status panel that keeps updating during pause. |
| `click_to_place` | Placing bounded objects from click-time pointer coordinates. |
| `input_cancellation` | Headless inspection of physical controls and cancelled releases; run with `--no-default-features`. |
| [image_board](rendering/Screen-Images.md) | Shared immutable image pixels, cropping, filtering, tint, and mixed panel/image order. |
| [iron_maze](examples/Iron-Maze.md) | A playable 2.5D shooter with fixed game rules, a screen-rectangle view, and level restart. |
| [territory_wars](examples/Territory-Wars.md) | Island conquest against six bots, independent economic/combat rules, and an opt-in debug/cheat panel. |
| [text_labels](rendering/Text.md) | Real Latin/Cyrillic labels, changing text, alignment and mixed screen order; add `--features text`. |

Run any of them with `cargo run --release --example <name>`.

## A complete headless program

For a separate Cargo project next to the `Sim-Logic` checkout, add:

```toml
[dependencies]
sim-logic = { path = "../Sim-Logic", default-features = false }
```

Adjust the path if the checkout lives elsewhere. Copy this complete program
into `src/main.rs` and run `cargo run`. It feeds one D-key press into the same
runtime used by the desktop host, then checks that the ball moved:

```rust
use std::time::Duration;
use sim_logic::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action { Up, Down, Left, Right }

const MOVEMENT: DigitalAxis2d<Action> = DigitalAxis2d::new(
    Action::Left, Action::Right, Action::Down, Action::Up,
);

#[derive(Component)]
struct Ball;

fn main() -> LogicResult {
    let step = Duration::from_millis(10);
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(step, 8)?);
    let mut app = Application::<Action>::new(config)?;
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

    let mut runner = app.build_headless(initial)?;
    let input = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let report = match runner.advance_frame(FrameRequest::new(step, &input, viewport)) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    assert!(report.failure().is_none(), "{:?}", report.failure());
    assert_eq!(report.fixed_ticks_attempted(), 1);

    let (ball, _) = runner.components::<Ball>().next().ok_or("ball is missing")?;
    let position = runner.component::<Transform2d>(ball)?.translation();
    assert!((position.x() - 0.08).abs() < 0.000_001);
    assert_eq!(position.y(), 0.0);
    assert_eq!(report.extracted_generation(), Some(runner.world_generation()));
    println!("Ball moved to {position:?} without opening a window.");
    Ok(())
}
```

`Ball` is a marker: an empty component that helps find this particular kind of
entity. Custom components need approval before startup; standard visual and
movement components are already approved. Spawning the circle also supplies
an origin `Transform2d` if none was provided.

The movement component stores an axis and a speed in world units per second.
The registered movement System applies that speed once per fixed tick. With
an explicit 0.01-second step, holding D moves the ball 0.08 units. Diagonal
input is normalized and idle input produces zero movement.

The headless driver accepts elapsed time and physical events explicitly. A
press remains held until a release is supplied; an empty event slice does not
release the key. Check both `FrameOutcome` and `report.failure()`: accepting a
frame does not mean every later System or extraction step succeeded.

The position checked above is the simulation position. The drawn position can
lag behind it by one fixed step because of interpolation. Headless execution
still prepares a render snapshot, but submits nothing to a GPU.

## Put the same setup in a window

Enable the default features in your dependency, keep the setup through
`app.add_digital_movement2d_system()`, and replace the headless driver with
`app.run(initial)?;`. The desktop host collects input and measures time for
you. Add explicit pause and exit Systems if wanted; the
[Moving Ball example](../examples/moving_ball.rs) shows both.

For the repository's headless tests:

```bash
cargo test --no-default-features
```

Next: [how the runtime works](guides/Runtime.md).

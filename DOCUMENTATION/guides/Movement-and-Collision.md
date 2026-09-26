# Movement, cameras and overlap queries

[Documentation](../README.md) / Guides

These are opt-in helpers, not a physics engine. Components hold configuration;
registered fixed-update Systems do the work. The
[getting-started program](../Getting-Started.md) gives a complete movement setup.

## Move from keyboard input

`DigitalAxis2d<Action>` identifies left, right, down and up logical actions.
Bind physical keys with `bind_wasd`, `bind_arrows` or `bind_wasd_and_arrows`.
The binding presets validate the complete change before installing it.

`FixedInput::digital_axis` returns raw components in `-1..=1`; opposite actions
cancel. `normalized_digital_axis` keeps diagonal speed equal to cardinal speed
and returns zero when idle. Neither method consumes press/release edges.
`FrameInput` offers the same axis helpers for frame-owned UI or camera policy.

For ordinary constant-speed World movement:

```rust,ignore
let movement = DigitalMovement2d::new(MOVEMENT, 8.0)?;
world.spawn((Player, movement, player_visual))?;
// During application setup:
app.bind_wasd_and_arrows(MOVEMENT)?;
app.add_digital_movement2d_system();
```

Speed is in world units per second, not pixels per frame. The standard System
normalizes diagonals and updates `Transform2d` once per fixed tick. Registering
it twice applies movement twice. It supplies no collision response or facing.

## Integrate velocity and acceleration

`LinearVelocity2d` stores world units per second. `LinearAcceleration2d` stores
world units per second squared, not force. Both require their explicit adapters:

```rust,ignore
let acceleration = LinearAcceleration2d::new(Vec2::new(0.0, -9.81))?;
world.spawn((FallingBody, acceleration, body_visual))?;
// During application setup, in this order:
app.add_linear_acceleration2d_system();
app.add_linear_velocity2d_system();
```

Acceleration supplies zero velocity if missing; velocity supplies an origin
Transform if missing. Acceleration before velocity means the position uses
the newly updated velocity (semi-implicit Euler). Reversing the registration
order deliberately uses the old velocity for this tick.

All movement helpers skip disabled entities. Components created through
Commands first participate after the stage barrier, on a later fixed tick.
Overflow reports a System failure; writes to entities already visited by that
System are not rolled back. There is no mass, angular motion, drag, swept
solver or hidden substep schedule.

## Follow a moving entity

Add `CameraFollowTarget2d` to one enabled entity with a Transform, and register
`add_camera_follow2d_system` after movement if the camera should follow the new
position. Zero enabled followers is a no-op; multiple followers are an error.
Create an `ActiveCamera2d` explicitly in the World factory.

The camera and body use fixed-state interpolation for drawing. Direct
FrameUpdate writes to fixed-owned transforms/cameras are not an alternative
interpolation path. A teleport policy may need to reset both body and camera
history. The [camera-follow example](../../examples/camera_follow/game.rs)
shows the ordinary ordering.

## Keep collision geometry separate from appearance

Add `CircleCollider2d` or `RectangleCollider2d` independently of visuals.
Rectangles are axis-aligned and use full width/height; drawn rounded corners
do not change their collision shape. Tangency counts as overlap.

`CircleOverlapEntities`, `RectangleOverlapEntities` and the mixed overlap
queries take typed candidate filters. Each request performs an allocation-free
linear scan of live matching ECS rows, not a copied candidate snapshot or a
spatial-index lookup. Candidate order is not a gameplay tie-break.
Use disjoint filters when a moving query and candidate query could conflict.

Typical policies are:

- A collectible test sends a typed event and queues despawn.
- A wall test checks a translated candidate Transform before accepting movement.
- A projectile test resolves a hit, then continues with the other projectiles.

Examples: [coin pickup](../../examples/coin_pickup/game.rs),
[rectangle room](../../examples/rectangle_room/game.rs), and
[projectile arena](../../examples/projectile_arena/game.rs).

Queued despawns remain visible until the barrier. If several projectiles may
hit one target in the same tick, gameplay must track already claimed targets
or choose a different explicit resolution policy. Likewise, a proposed-position
overlap test can tunnel through a thin wall when the step is large. These
queries do not calculate swept contacts or resolve penetration automatically.

## Draw velocity or acceleration vectors

`LineVisual` starts at the entity's interpolated Transform and ends at its
configured world-space offset. Width is in logical pixels. A zero-length line
is accepted as a no-op. Copy the desired velocity/acceleration into the line
explicitly; no hidden System keeps the two components synchronized.

See [acceleration vectors](../../examples/acceleration_vectors/main.rs) for
an executable example. Screen-fixed paths and their hit tests use a different
coordinate space; see [screen geometry](../rendering/Screen-HUD.md).

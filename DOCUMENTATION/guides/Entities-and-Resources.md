# Entities, resources and events

[Documentation](../README.md) / Guides

The [getting-started program](../Getting-Started.md) is a complete runnable
setup. The fragments below show changes you can make to that setup. For stage
order and World replacement, see [Runtime](Runtime.md).

## Approve custom components before startup

Components are ordinary Rust types derived with `Component`. Register each
custom type through `app.approve_component::<T>()` before building the runner.
Standard Logic visuals and movement components are already approved.

Approval validates compatibility with managed spawning and component changes.
It does not make arbitrary Bevy lifecycle hooks or required components safe;
unsupported registrations return typed errors. Use the public approval APIs
rather than editing the underlying ECS registry.

`LogicEntity` carries application and World-generation identity. It is a
runtime handle, not a save-file ID. Handles from an old World cannot address
entities in its replacement. Store your own persistent IDs in application data.

## Build initial entities in the factory

`WorldBuilder::spawn` creates an entity immediately in the isolated candidate.
For a fixed small array, `spawn_array` validates the bundle and total capacity
before creating its first entity:

```rust,ignore
let enemies = world.spawn_array([
    (Enemy, Transform2d::from_xy(-4.0, 0.0)?, enemy_visual),
    (Enemy, Transform2d::from_xy( 0.0, 0.0)?, enemy_visual),
    (Enemy, Transform2d::from_xy( 4.0, 0.0)?, enemy_visual),
])?;
```

Returned handles match input order. This is a convenience for fixed arrays,
not a dynamic bulk-insertion performance guarantee.

## Defer structural changes during updates

Use `Commands` to spawn, despawn, insert or remove components inside a System.
Changes become visible at the stage barrier, after that stage's Systems, not
immediately after the calling System. Later Systems in the same stage can
still see an entity queued for removal.

```rust,ignore
commands.spawn((Projectile, transform, visual, velocity))?;
commands.insert(player, LinearVelocity2d::new(Vec2::new(2.0, 0.0))?)?;
commands.remove::<LinearVelocity2d>(other)?;
commands.despawn(expired)?;
```

For identical `Copy` bundles, `commands.spawn_copies(bundle, count)` stores one
erased template. It still creates separate entities and charges every copy
against the stage command limit. A zero count is a true no-op. Deferred spawn
returns no handles: the entities do not exist until the barrier.

Conflicts, invalid handles, unapproved components and budget overruns reject
the structural batch. Direct component and resource writes already performed
by Systems are **not** rolled back. Do not treat a failed stage as a transaction
over all application state.

## Temporarily disable an entity

`commands.disable(handle)` and `commands.enable(handle)` use the
same barrier. Disabled entities keep their identity, components and capacity
charge, but are omitted from ordinary managed queries, movement and extraction.
Save their handles elsewhere if you will enable them later: a normal query will
not rediscover them. You can also spawn an approved `Disabled` marker initially.

Conflicting structural requests in one batch, such as despawn plus a toggle on
the same handle, fail together. Disabling is not despawning or a memory-saving
pool. For hiding only a visual, use that visual's visibility controls instead.

## Choose where shared state lives

| Scope | Register or insert | Read/write in Systems | World replacement |
| --- | --- | --- | --- |
| World | `world.insert_resource(value)` in the factory | `Res<T>` / `ResMut<T>` | Replaced with the World. |
| Application | `app.register_app_resource(value)` before startup | `AppRes<T>` / `AppResMut<T>` | The same value survives. |

World Resources use types derived with `Resource`. Application Resources only
require `Send + Sync + 'static`, without a Bevy derive. They are transferred
without cloning; a World does not own or reset them. They are deliberately
unavailable to candidate factories and Startup Systems. This keeps candidate
preparation from mutating live application state before replacement succeeds.

Use World Resources for level state; use Application Resources for session
settings, persistent scores or an owned service. Initial candidate data should
come from explicit factory captures, not hidden reads of live application state.

Resource writes are ordinary immediate System writes. Borrow conflicts and
missing required resources are diagnosed rather than silently creating values.
The [persistent-score example](../../examples/persistent_score/game.rs) shows
the complete application-resource setup and replacement path.

## Send short-lived typed events

Register a `Copy` event type before startup. `EventWriter<T>` sends events and
`EventReader<T>` iterates the stage's events. Later Systems in that same stage
can observe earlier sends; this is not an asynchronous message queue. Register
the producer before the consumer.

Events do not persist across stage boundaries or World replacement. Repeated
catch-up ticks have separate event lifetimes. If one System both reads and
writes the same event type, use the writer's read access rather than requesting
conflicting reader and writer borrows. Ignoring a failed send does not turn it
into success: event-budget failure still fails the stage.

For data needed on a later frame, store it in an explicit bounded resource.
See [coin pickup](../../examples/coin_pickup/game.rs) for event-driven score
updates and the [runtime guide](Runtime.md) for input-edge lifetimes.

## Keep error types useful

`LogicResult<T = ()>` is shorthand for `Result<T, Box<dyn std::error::Error>>`.
It is handy when application code combines several error types with `?`.
Library APIs retain concrete errors. Use your own result alias when errors
must be `Send + Sync` or represented by a project-specific enum.

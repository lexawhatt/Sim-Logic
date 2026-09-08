# A small Ferris

Enable the optional `easter-eggs` Cargo feature to use `draw_crab` from the
library. This is not a separate example or a new renderer: it prepares a
normal `ScreenImageVisual` with the included transparent Ferris artwork.

```rust
use sim_logic::prelude::*;

fn main() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(1)
            .with_frame_limits(FrameLimits::new(2, 1, 6, 4096, 1024 * 1024, 1)),
    );
    let mut app = Application::<u8>::new(config)?;
    let crab = draw_crab(&mut app, LogicalScreenPosition::new(24.0, 24.0), 230.0)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("ferris", move |world| {
        world.spawn(camera)?;
        world.spawn(crab)?;
        Ok(())
    })?;
    app.run(initial)?;
    Ok(())
}
```

Use the [getting-started dependency setup](Getting-Started.md) with
`features = ["easter-eggs"]`. Desktop hosting uses the default `desktop`
feature. For a window-free application, disable default features and use
`app.build_headless(initial)?` instead of `app.run(initial)?`.

The position is the top-left corner in logical screen pixels. Width must be
positive and finite; height preserves the original 460:307 proportions.
The crab stays in place when the World camera moves. Linear sampling is
selected for smooth scaling. Change its tint, layer, position or size with
the ordinary image setters; copy it to place more crabs, or disable/despawn
its entity to hide it.

Call `draw_crab` during Application setup. It does not spawn an entity, start
a window, or draw immediately. The first successful call decodes the embedded
40,445-byte PNG and registers 564,880 pixel bytes. Subsequent calls on that
Application reuse its image handle without decoding or registering again,
including when the registry is full. The asset survives World replacement.

The first decode uses a temporary pixel buffer, then registration copies it
into Application-owned storage. The PNG decoder has its own temporary memory;
desktop presentation also keeps the existing Engine recovery copy and GPU
texture. The pixel limit is not a total-process memory guarantee. Neither
PNG decoding nor image registration runs each frame.

The helper never raises image, entity or frame budgets. The sample explicitly
allows one image placement and its texture; budget additional placements
normally. Invalid geometry is rejected before image work. Registration errors
leave existing assets intact and do not cache an invalid handle. See
[screen images](Screen-Images.md) for the shared limits and rendering behavior.

With `easter-eggs` disabled, this helper, its PNG decoder dependency and the
embedded image are absent from the build. The PNG is bundled in the crate;
no runtime files or downloads are needed. Ferris artwork is by Karen Rustad
Tölva and has a [CC0 dedication](https://rustacean.net/).

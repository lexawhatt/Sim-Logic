//! Public API consumer: no direct Engine imports, window or GPU required.
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};
use std::time::Duration;

#[path = "screen_paths/runtime.rs"]
mod runtime;
#[path = "screen_paths/values.rs"]
mod values;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

fn points() -> [LogicalScreenPosition; 3] {
    [(10.25, 20.5), (50.25, 20.5), (70.25, 40.5)].map(|(x, y)| LogicalScreenPosition::new(x, y))
}

fn path() -> LogicResult<ScreenPolylineVisual> {
    Ok(ScreenPolylineVisual::new(
        &points(),
        StrokeStyle2d::new(2.0, Color::WHITE).with_join(StrokeJoin2d::Round),
    )?)
}

fn runner_with_paths(
    paths: Vec<ScreenPolylineVisual>,
    limits: RenderLimits,
) -> LogicResult<HeadlessRunner<Action>> {
    let mut config = AppConfig::default();
    config.set_render_limits(limits);
    let mut app = Application::<Action>::new(config)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("paths", move |world| {
        world.spawn(camera)?;
        for path in &paths {
            world.spawn(path.clone())?;
        }
        Ok(())
    })?;
    Ok(app.build_headless(initial)?)
}

fn pointer(x: f32, y: f32) -> LogicResult<PointerSample> {
    Ok(PointerSample::new(
        LogicalScreenPosition::new(x, y),
        LogicalViewport::new(800.0, 600.0)?,
    )?)
}

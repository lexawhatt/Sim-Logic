use sim_logic::prelude::*;
use std::time::Duration;

pub const STEP: Duration = Duration::from_millis(100);
pub const PIXELS: [u8; 16] = [
    255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 255, 255, 255, 0, 255,
];

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestAction {
    Modify,
    Replace,
}

pub fn application(image_limit: usize) -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 4)?);
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(image_limit)
            .with_frame_limits(FrameLimits::new(
                32,
                12000,
                2_100_000,
                140 * 1024 * 1024,
                1024,
                12000,
            )),
    );
    Ok(Application::new(config)?)
}

pub fn image(id: ImageAssetId, x: f32, depth: f32) -> LogicResult<ScreenImageVisual> {
    let mut visual = ScreenImageVisual::new(
        id,
        LogicalScreenPosition::new(x, 20.0),
        LogicalScreenVector::new(80.0, 60.0),
    )?;
    visual.set_draw_order_depth(depth)?;
    Ok(visual)
}

pub fn rectangle(x: f32, depth: f32) -> LogicResult<ScreenRectangleVisual> {
    let mut visual = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, 10.0),
        LogicalScreenVector::new(80.0, 40.0),
        Color::WHITE,
    )?;
    visual.set_draw_order_depth(depth)?;
    Ok(visual)
}

pub fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    delta: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        delta,
        events,
        LogicalViewport::new(800.0, 600.0)?,
    )) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

pub fn snapshot(runner: &HeadlessRunner<TestAction>) -> LogicResult<&ExtractedFrame> {
    runner
        .extracted_frame()
        .ok_or_else(|| "expected complete image snapshot".into())
}

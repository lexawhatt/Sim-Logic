use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

pub(super) const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TestAction {
    Replace,
}

pub(super) fn screen_rectangle(x: f32) -> Result<ScreenRectangleVisual, Box<dyn Error>> {
    Ok(ScreenRectangleVisual::new(
        LogicalScreenPosition::new(x, 24.0),
        LogicalScreenVector::new(100.0, 20.0),
        Color::WHITE,
    )?)
}

pub(super) fn application(limits: RenderLimits) -> Result<Application<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
    config.set_render_limits(limits);
    Ok(Application::new(config)?)
}

pub(super) fn advance<A: Action>(
    runner: &mut HeadlessRunner<A>,
    elapsed: Duration,
    events: &[InputEvent],
    width: f32,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(width, 600.0)?;
    match runner.advance_frame(FrameRequest::new(elapsed, events, viewport)) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

pub(super) fn snapshot<A: Action>(
    runner: &HeadlessRunner<A>,
) -> Result<&sim_logic::ExtractedFrame, Box<dyn Error>> {
    runner
        .extracted_frame()
        .ok_or_else(|| "a complete published snapshot should exist".into())
}

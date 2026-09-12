use sim_logic::prelude::*;
use std::time::Duration;

pub const FONT: &[u8] = include_bytes!("../assets/text/DejaVuSans.ttf");
pub const STEP: Duration = Duration::from_millis(100);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub enum TestAction {
    Modify,
    Replace,
}

pub fn limits(count: usize, bytes: usize, glyphs: usize) -> RenderLimits {
    RenderLimits::default()
        .with_max_screen_images(4)
        .with_max_screen_texts(count)
        .with_max_screen_text_bytes(bytes)
        .with_max_screen_text_glyphs(glyphs)
}

pub fn application(limits: RenderLimits) -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(STEP, 4)?);
    config.set_render_limits(limits);
    Ok(Application::new(config)?)
}

pub fn font(app: &mut Application<TestAction>) -> LogicResult<TextFont> {
    Ok(app.register_font(FONT.to_vec(), TextSettings::new(24.0)?)?)
}

pub fn label(font: TextFont, text: &str, x: f32) -> LogicResult<ScreenTextVisual> {
    Ok(ScreenTextVisual::new(
        font,
        text,
        LogicalScreenPosition::new(x, 40.0),
    )?)
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
        .ok_or_else(|| "expected complete text snapshot".into())
}

pub fn edge(state: ButtonState) -> InputEvent {
    InputEvent::key(PhysicalKeyCode::Space, state)
}

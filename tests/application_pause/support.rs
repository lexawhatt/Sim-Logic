use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

pub(super) const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TestAction {
    Pause,
    Replace,
}

#[derive(Component)]
pub(super) struct Marker;

#[derive(Default)]
pub(super) struct Probe {
    pub(super) fixed_ticks: Vec<u64>,
    pub(super) fixed_pause_presses: usize,
    pub(super) held_pause_ticks: usize,
    pub(super) frame_pause_states: Vec<bool>,
}

pub(super) fn application() -> Result<Application<TestAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 4)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Marker>()?;
    application.register_app_resource(Probe::default())?;
    application.bind_key(PhysicalKeyCode::Space, TestAction::Pause)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Replace)?;
    Ok(application)
}

pub(super) fn register_world(
    application: &mut Application<TestAction>,
    name: &str,
    x: f32,
) -> Result<WorldFactoryId, Box<dyn Error>> {
    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(1.0, Color::WHITE)?;
    let transform = Transform2d::from_xy(x, 0.0)?;
    Ok(application.register_world(name, move |world| {
        world.spawn(camera)?;
        world.spawn((transform, visual))?;
        Ok(())
    })?)
}

pub(super) fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    match runner.advance_frame(FrameRequest::new(elapsed, events, viewport)) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

pub(super) fn observe_fixed(
    time: FixedTime,
    input: FixedInput<TestAction>,
    mut probe: AppResMut<Probe>,
) {
    probe.fixed_ticks.push(time.tick_index());
    probe.fixed_pause_presses += input.pressed(TestAction::Pause).count();
    probe.held_pause_ticks += usize::from(input.held(TestAction::Pause));
}

pub(super) fn observe_frame(time: FrameTime, mut probe: AppResMut<Probe>) {
    probe.frame_pause_states.push(time.is_paused());
}

pub(super) fn move_visuals(
    mut transforms: Query<&mut Transform2d>,
    mut cameras: Query<&mut ActiveCamera2d>,
) -> LogicResult {
    for mut transform in &mut transforms {
        transform.translate_by(Vec2::new(1.0, 0.0))?;
    }
    for mut camera in &mut cameras {
        camera.pan_by(Vec2::new(1.0, 0.0))?;
    }
    Ok(())
}

pub(super) fn probe(runner: &HeadlessRunner<TestAction>) -> Result<&Probe, Box<dyn Error>> {
    runner
        .app_resource::<Probe>()
        .ok_or_else(|| "application observation should remain available".into())
}

pub(super) fn extracted_x(runner: &HeadlessRunner<TestAction>) -> Result<f32, Box<dyn Error>> {
    let frame = runner
        .extracted_frame()
        .ok_or("frame should publish an extraction")?;
    let [circle] = frame.resolved_circles() else {
        return Err("World should extract exactly one circle".into());
    };
    Ok(circle.position().x())
}

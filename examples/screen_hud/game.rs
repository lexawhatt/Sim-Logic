use sim_engine::Layer;
use sim_logic::prelude::*;

const PROGRESS_PERIOD_SECONDS: f32 = 6.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemoAction {
    Pause,
    Exit,
}

#[derive(Component)]
pub struct MovingBody;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Component)]
pub enum HudPart {
    Panel,
    Status,
    ProgressTrack,
    ProgressFill,
    Activity,
}

#[derive(Resource, Default)]
struct SimulationProgress(f32);

#[derive(Resource, Default)]
struct PresentationClock(f32);

fn advance_progress(time: FixedTime, mut progress: ResMut<SimulationProgress>) {
    progress.0 = (progress.0 + time.seconds_f32()).rem_euclid(PROGRESS_PERIOD_SECONDS);
}

fn update_hud(
    input: FrameInput<DemoAction>,
    time: FrameTime,
    viewport: FrameViewport,
    progress: Res<SimulationProgress>,
    mut clock: ResMut<PresentationClock>,
    mut rectangles: Query<(&HudPart, &mut ScreenRectangleVisual)>,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(DemoAction::Exit) {
        commands.request_exit()?;
        return Ok(());
    }
    let paused = if input.has_press_occurrence(DemoAction::Pause) {
        let requested = !time.is_paused();
        commands.set_paused(requested)?;
        requested
    } else {
        time.is_paused()
    };

    clock.0 = (clock.0 + time.seconds_f32()).rem_euclid(1.0);
    let pulse = 0.55 + 0.45 * (clock.0 * std::f32::consts::TAU).sin().abs();
    let width = (viewport.logical().width() - 48.0).clamp(160.0, 300.0);
    let x = (viewport.logical().width() - width - 24.0).max(12.0);
    let track_width = width - 40.0;
    let progress_width = (track_width * progress.0 / PROGRESS_PERIOD_SECONDS).max(1.0);

    for (part, mut rectangle) in &mut rectangles {
        let (position, size) = match part {
            HudPart::Panel => ((x, 24.0), (width, 92.0)),
            HudPart::Status => ((x + 20.0, 44.0), (width - 70.0, 10.0)),
            HudPart::ProgressTrack => ((x + 20.0, 78.0), (track_width, 14.0)),
            HudPart::ProgressFill => ((x + 20.0, 78.0), (progress_width, 14.0)),
            HudPart::Activity => ((x + width - 30.0, 44.0), (10.0, 10.0)),
        };
        rectangle.set_geometry(
            LogicalScreenPosition::new(position.0, position.1),
            LogicalScreenVector::new(size.0, size.1),
        )?;
        match part {
            HudPart::Status => rectangle.set_color(if paused {
                Color::rgb8(255, 185, 70)
            } else {
                Color::rgb8(75, 215, 155)
            })?,
            HudPart::Activity => rectangle.set_color(Color::WHITE.with_alpha(pulse))?,
            _ => {}
        }
    }
    Ok(())
}

#[cfg_attr(
    test,
    allow(dead_code, reason = "the shared test uses the custom-time builder")
)]
pub fn build_application() -> LogicResult<(Application<DemoAction>, WorldFactoryId)> {
    build_application_with_time(TimeConfig::default())
}

pub fn build_application_with_time(
    time: TimeConfig,
) -> LogicResult<(Application<DemoAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;
    application.approve_components::<(MovingBody, HudPart)>()?;
    application.bind_key(PhysicalKeyCode::Space, DemoAction::Pause)?;
    application.bind_key(PhysicalKeyCode::Escape, DemoAction::Exit)?;

    // Camera follow observes the body's new canonical position in this tick.
    application.add_linear_velocity2d_system();
    application.add_camera_follow2d_system();
    application.add_fixed_system(advance_progress);
    application.add_fallible_frame_system(update_hud);

    let offset = Vec2::new(0.0, 2.0);
    let camera = ActiveCamera2d::new(Camera2d::new(offset, 32.0)?);
    let follow = CameraFollowTarget2d::new(offset)?;
    let body = CircleVisual::new(0.55, Color::rgb8(68, 144, 255))?;
    let velocity = LinearVelocity2d::new(Vec2::new(1.8, 0.0))?;
    let landmark = RectangleVisual::new(Vec2::new(0.15, 3.0), Color::rgb8(42, 56, 77))?;
    let background = WorldBackground::new(Color::rgb8(10, 16, 28))?;
    let landmarks = [-12.0, -8.0, -4.0, 0.0, 4.0, 8.0, 12.0, 16.0, 20.0]
        .into_iter()
        .map(|x| Transform2d::from_xy(x, -1.5).map(|transform| (transform, landmark)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut hud = Vec::new();
    for (part, position, size, color, layer) in [
        (
            HudPart::Panel,
            (24.0, 24.0),
            (300.0, 92.0),
            Color::rgb8(22, 31, 49),
            0,
        ),
        (
            HudPart::Status,
            (44.0, 44.0),
            (230.0, 10.0),
            Color::rgb8(75, 215, 155),
            1,
        ),
        (
            HudPart::ProgressTrack,
            (44.0, 78.0),
            (260.0, 14.0),
            Color::rgb8(44, 59, 80),
            1,
        ),
        (
            HudPart::ProgressFill,
            (44.0, 78.0),
            (1.0, 14.0),
            Color::rgb8(68, 144, 255),
            2,
        ),
        (
            HudPart::Activity,
            (294.0, 44.0),
            (10.0, 10.0),
            Color::WHITE,
            1,
        ),
    ] {
        let mut rectangle = ScreenRectangleVisual::new(
            LogicalScreenPosition::new(position.0, position.1),
            LogicalScreenVector::new(size.0, size.1),
            color,
        )?;
        rectangle.set_layer(Layer::new(layer));
        hud.push((part, rectangle));
    }

    let initial = application.register_world("screen-hud", move |world| {
        world.insert_resource(background)?;
        world.insert_resource(SimulationProgress::default())?;
        world.insert_resource(PresentationClock::default())?;
        world.spawn(camera)?;
        world.spawn((MovingBody, body, velocity, follow))?;
        for &landmark in &landmarks {
            world.spawn(landmark)?;
        }
        for &part in &hud {
            world.spawn(part)?;
        }
        Ok(())
    })?;
    Ok((application, initial))
}

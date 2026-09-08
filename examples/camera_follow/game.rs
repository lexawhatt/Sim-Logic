use sim_logic::prelude::*;

const PLAYER_SPEED: f32 = 8.0;
const CAMERA_OFFSET: Vec2 = Vec2::new(0.0, 2.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerAction {
    Up,
    Down,
    Left,
    Right,
}

const MOVEMENT: DigitalAxis2d<PlayerAction> = DigitalAxis2d::new(
    PlayerAction::Left,
    PlayerAction::Right,
    PlayerAction::Down,
    PlayerAction::Up,
);

#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "the shared integration module uses the custom-time builder"
    )
)]
pub(crate) fn build_application() -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    build_application_with_time(TimeConfig::default())
}

pub(crate) fn build_application_with_time(
    time: TimeConfig,
) -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;

    application.bind_wasd_and_arrows(MOVEMENT)?;

    let camera = ActiveCamera2d::new(Camera2d::new(CAMERA_OFFSET, 28.0)?);
    let player_visual = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?;
    let player_movement = DigitalMovement2d::new(MOVEMENT, PLAYER_SPEED)?;
    let camera_follow = CameraFollowTarget2d::new(CAMERA_OFFSET)?;
    let landmarks = [
        (
            Transform2d::from_xy(-5.0, 0.0)?,
            CircleVisual::new(0.3, Color::rgb8(255, 190, 64))?,
        ),
        (
            Transform2d::from_xy(5.0, 0.0)?,
            CircleVisual::new(0.3, Color::rgb8(255, 96, 96))?,
        ),
        (
            Transform2d::from_xy(0.0, 5.0)?,
            CircleVisual::new(0.3, Color::rgb8(96, 220, 140))?,
        ),
    ];
    let initial_world = application.register_world("camera-follow", move |world| {
        world.spawn(camera)?;
        world.spawn((camera_follow, player_visual, player_movement))?;
        world.spawn_array(landmarks)?;
        Ok(())
    })?;

    // Registration order is explicit: the camera observes the player's new
    // canonical position from this same fixed tick.
    application.add_digital_movement2d_system();
    application.add_camera_follow2d_system();

    Ok((application, initial_world))
}

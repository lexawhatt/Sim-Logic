use sim_logic::prelude::*;

const PLAYER_SPEED: f32 = 8.0;

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

#[derive(Component)]
pub struct Player;

#[derive(Component, Clone, Copy)]
pub struct Wall;

type PlayerCollisionSingle<'world, 'state> = Single<
    'world,
    'state,
    (
        LogicEntityRef,
        &'static mut Transform2d,
        &'static CircleCollider2d,
    ),
    (With<Player>, Without<Wall>),
>;
type WallOverlaps<'world, 'state> =
    RectangleOverlapEntities<'world, 'state, (With<Wall>, Without<Player>)>;

fn move_player(
    input: FixedInput<PlayerAction>,
    time: FixedTime,
    player: PlayerCollisionSingle,
    walls: WallOverlaps,
) -> LogicResult {
    let direction = input.normalized_digital_axis(MOVEMENT);
    let (entity, mut transform, collider) = player.into_inner();
    if direction == Vec2::ZERO {
        return Ok(());
    }
    let proposed = transform.translated_by(direction * (PLAYER_SPEED * time.seconds_f32()))?;
    let blocked = walls.has_overlap_with_circle(entity.handle(), &proposed, collider)?;
    if !blocked {
        transform.set_translation(proposed.translation())?;
    }
    Ok(())
}

#[cfg_attr(
    test,
    allow(
        dead_code,
        reason = "the shared headless test uses the custom-time builder"
    )
)]
pub fn build_application() -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    build_application_with_time(TimeConfig::default())
}

pub fn build_application_with_time(
    time: TimeConfig,
) -> LogicResult<(Application<PlayerAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_time(time);
    let mut application = Application::new(config)?;
    application.approve_components::<(Player, Wall)>()?;

    application.bind_wasd_and_arrows(MOVEMENT)?;
    application.add_fallible_fixed_system(move_player);

    let camera = ActiveCamera2d::centered(30.0)?;
    let background = WorldBackground::new(Color::rgb8(12, 18, 30))?;

    let player_body = CircleVisual::new(0.5, Color::rgb8(68, 144, 255))?.with_matching_collider();

    let wall_color = Color::rgb8(54, 68, 92);
    let horizontal_wall =
        RectangleVisual::rounded(Vec2::new(16.0, 0.7), wall_color, 0.2)?.with_matching_collider();
    let vertical_wall =
        RectangleVisual::rounded(Vec2::new(0.7, 12.0), wall_color, 0.2)?.with_matching_collider();
    let walls = [
        (Wall, Transform2d::from_xy(0.0, -6.0)?, horizontal_wall),
        (Wall, Transform2d::from_xy(0.0, 6.0)?, horizontal_wall),
        (Wall, Transform2d::from_xy(-8.0, 0.0)?, vertical_wall),
        (Wall, Transform2d::from_xy(8.0, 0.0)?, vertical_wall),
    ];

    let initial_world = application.register_world("rectangle-room", move |world| {
        world.insert_resource(background)?;
        world.spawn(camera)?;
        world.spawn((Player, player_body))?;
        world.spawn_array(walls)?;
        Ok(())
    })?;

    Ok((application, initial_world))
}

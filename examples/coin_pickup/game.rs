use sim_logic::prelude::*;

const PLAYER_SPEED: f32 = 10.0;
const PLAYER_RADIUS: f32 = 0.5;
const COIN_RADIUS: f32 = 0.3;

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

#[derive(Component)]
pub struct Coin;

#[derive(Debug, Clone, Copy)]
pub struct CoinCollected {
    pub coin: LogicEntity,
    pub points: u32,
}

#[derive(Resource, Default)]
pub struct Score {
    pub points: u32,
    pub last_coin: Option<LogicEntity>,
}

fn detect_pickups(
    player: Single<(LogicEntityRef, &Transform2d, &CircleCollider2d), With<Player>>,
    coins: CircleOverlapEntities<With<Coin>>,
    mut commands: Commands,
    mut collected: EventWriter<CoinCollected>,
) -> LogicResult {
    let (player, transform, collider) = player.into_inner();

    for coin in coins.iter_overlapping(player.handle(), transform, collider)? {
        commands.despawn(coin)?;
        collected.send(CoinCollected { coin, points: 1 })?;
    }
    Ok(())
}

fn apply_score(collected: EventReader<CoinCollected>, mut score: ResMut<Score>) {
    for event in &collected {
        score.points = score.points.saturating_add(event.points);
        score.last_coin = Some(event.coin);
    }
}

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
    application.approve_components::<(Player, Coin)>()?;

    application.bind_wasd_and_arrows(MOVEMENT)?;

    let camera = ActiveCamera2d::centered(20.0)?;
    let player_body =
        CircleVisual::new(PLAYER_RADIUS, Color::rgb8(68, 144, 255))?.with_matching_collider();
    let player_movement = DigitalMovement2d::new(MOVEMENT, PLAYER_SPEED)?;
    let coin_body =
        CircleVisual::new(COIN_RADIUS, Color::rgb8(255, 210, 48))?.with_matching_collider();
    let coin_transforms = [
        Transform2d::from_xy(2.0, 0.0)?,
        Transform2d::from_xy(4.0, 1.0)?,
        Transform2d::from_xy(-3.0, -1.0)?,
    ];
    let initial_world = application.register_world("coin-pickup", move |world| {
        world.spawn(camera)?;
        world.spawn((Player, player_body, player_movement))?;
        world.spawn_array(coin_transforms.map(|transform| (Coin, transform, coin_body)))?;
        world.insert_resource(Score::default())?;
        Ok(())
    })?;

    application.add_digital_movement2d_system();
    application.add_fallible_fixed_system(detect_pickups);
    application.add_fixed_system(apply_score);

    Ok((application, initial_world))
}

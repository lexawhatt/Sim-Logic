use sim_logic::prelude::*;

const BASE_RESULT_RADIUS: f32 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerAction {
    CollectPoint,
    ShowResults,
}

/// Application-owned state: it deliberately has no Bevy `Resource` derive.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SessionScore {
    pub points: u32,
}

#[derive(Debug)]
pub struct GameSettings {
    pub points_per_collect: u32,
}

#[derive(Resource)]
pub struct PlayRoom;

#[derive(Resource, Default)]
pub struct ResultsRoom {
    pub observed_score: Option<u32>,
    pub score_was_newly_installed: bool,
}

#[derive(Component)]
pub struct ScoreOrb;

fn collect_point(
    input: FixedInput<PlayerAction>,
    play_room: Option<Res<PlayRoom>>,
    settings: AppRes<GameSettings>,
    mut score: AppResMut<SessionScore>,
) {
    if play_room.is_none() {
        return;
    }
    for _edge in input.pressed(PlayerAction::CollectPoint) {
        score.points = score.points.saturating_add(settings.points_per_collect);
    }
}

fn report_result(score: AppRes<SessionScore>, mut results: Option<ResMut<ResultsRoom>>) {
    let Some(results) = results.as_mut() else {
        return;
    };
    if results.observed_score.is_none() {
        println!("Score carried into the results World: {}", score.points);
        results.observed_score = Some(score.points);
        results.score_was_newly_installed = score.is_added();
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
    application.approve_component::<ScoreOrb>()?;
    application.register_app_resource(SessionScore::default())?;
    application.register_app_resource(GameSettings {
        points_per_collect: 1,
    })?;
    application.bind_key(PhysicalKeyCode::Space, PlayerAction::CollectPoint)?;
    application.bind_key(PhysicalKeyCode::Enter, PlayerAction::ShowResults)?;

    let results_camera = ActiveCamera2d::centered(12.0)?;
    let result_visual = CircleVisual::new(BASE_RESULT_RADIUS, Color::rgb8(75, 175, 255))?;
    let results = application.register_world("results", move |world| {
        world.spawn(results_camera)?;
        world.spawn((ScoreOrb, result_visual))?;
        world.insert_resource(ResultsRoom::default())?;
        Ok(())
    })?;

    let play_camera = ActiveCamera2d::centered(12.0)?;
    let play_visual = CircleVisual::new(0.6, Color::rgb8(255, 205, 65))?;
    let play = application.register_world("play", move |world| {
        world.spawn(play_camera)?;
        world.spawn(play_visual)?;
        world.insert_resource(PlayRoom)?;
        world.insert_resource(WorldReplacementOnPress::new(
            PlayerAction::ShowResults,
            results,
        ))?;
        Ok(())
    })?;

    // The score changes before the transition request in this fixed tick, so
    // the updated value itself crosses the successful commit boundary.
    application.add_fixed_system(collect_point);
    application.add_world_replacement_on_press_system();
    application.add_frame_system(report_result);

    Ok((application, play))
}

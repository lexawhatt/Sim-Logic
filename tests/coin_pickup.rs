use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[path = "../examples/coin_pickup/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

fn advance(
    runner: &mut HeadlessRunner<game::PlayerAction>,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, events, viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded pickup frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("pickup frame failed: {failure}").into());
    }
    assert_eq!(report.fixed_ticks_attempted(), 1);
    Ok(report)
}

fn player_translation(runner: &HeadlessRunner<game::PlayerAction>) -> Result<Vec2, Box<dyn Error>> {
    let (player, _) = runner
        .components::<game::Player>()
        .next()
        .ok_or("the World should contain one player")?;
    Ok(runner.component::<Transform2d>(player)?.translation())
}

#[test]
fn movement_pickup_event_despawn_and_score_form_one_slice() -> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(initial_world)?;

    let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];
    let first = advance(&mut runner, &pressed)?;
    assert_eq!(first.despawned(), 0);
    assert_eq!(player_translation(&runner)?, Vec2::new(1.0, 0.0));
    assert_eq!(runner.components::<game::Coin>().count(), 3);
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.points),
        Some(0)
    );

    let second = advance(&mut runner, &[])?;
    assert_eq!(second.despawned(), 1);
    assert_eq!(player_translation(&runner)?, Vec2::new(2.0, 0.0));
    assert_eq!(runner.components::<game::Coin>().count(), 2);
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.points),
        Some(1)
    );

    let released = [InputEvent::key(
        PhysicalKeyCode::KeyD,
        ButtonState::Released,
    )];
    let third = advance(&mut runner, &released)?;
    assert_eq!(third.despawned(), 0);
    assert_eq!(player_translation(&runner)?, Vec2::new(2.0, 0.0));
    assert_eq!(runner.components::<game::Coin>().count(), 2);
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.points),
        Some(1),
        "the previous tick's CoinCollected event must not replay"
    );
    let collected_coin = runner
        .resource::<game::Score>()
        .and_then(|score| score.last_coin)
        .ok_or("the score should retain the last collected coin handle")?;
    assert!(matches!(
        runner.component::<game::Coin>(collected_coin),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("successful pickup frame should be extracted")?
            .resolved_circles()
            .len(),
        3
    );

    Ok(())
}

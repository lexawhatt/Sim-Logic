use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[path = "../examples/persistent_score/game.rs"]
mod game;
#[path = "application_resources/runtime.rs"]
mod runtime;

const FIXED_STEP: Duration = Duration::from_millis(100);

fn advance(
    runner: &mut HeadlessRunner<game::PlayerAction>,
    events: &[InputEvent],
    elapsed: Duration,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, events, viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded persistent-score frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("persistent-score frame failed: {failure}").into());
    }
    Ok(report)
}

#[test]
fn score_survives_same_frame_world_replacement() -> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, play) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(play)?;

    let collect = [InputEvent::key(
        PhysicalKeyCode::Space,
        ButtonState::Pressed,
    )];
    let collected = advance(&mut runner, &collect, FIXED_STEP)?;
    assert_eq!(collected.fixed_ticks_attempted(), 1);
    assert_eq!(
        runner
            .app_resource::<game::SessionScore>()
            .map(|s| s.points),
        Some(1)
    );
    assert!(runner.resource::<game::PlayRoom>().is_some());

    let replace = [
        InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
        InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
    ];
    let replaced = advance(&mut runner, &replace, FIXED_STEP)?;
    assert!(matches!(
        replaced.transition(),
        FrameTransition::Committed { old, new, .. } if old != new
    ));
    assert_eq!(runner.active_world_name(), "results");
    assert_eq!(
        runner
            .app_resource::<game::SessionScore>()
            .map(|s| s.points),
        Some(1)
    );
    assert!(runner.resource::<game::PlayRoom>().is_none());
    assert!(runner.resource::<game::ResultsRoom>().is_some());

    let (orb, _) = runner
        .components::<game::ScoreOrb>()
        .next()
        .ok_or("results World should contain its score orb")?;
    let visual = runner.component::<CircleVisual>(orb)?;
    assert_eq!(visual.radius(), 0.8);

    // A committed fixed-stage transition skips ordinary FrameUpdate. The
    // candidate snapshot is already a valid, static visual; the next frame
    // proves that an installed B can read the persistent score.
    let displayed = advance(&mut runner, &[], Duration::ZERO)?;
    assert_eq!(displayed.fixed_ticks_attempted(), 0);
    assert_eq!(
        runner
            .resource::<game::ResultsRoom>()
            .and_then(|results| results.observed_score),
        Some(1)
    );
    assert!(
        runner
            .resource::<game::ResultsRoom>()
            .is_some_and(|results| results.score_was_newly_installed)
    );

    Ok(())
}

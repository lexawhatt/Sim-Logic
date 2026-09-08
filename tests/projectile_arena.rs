use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

#[path = "../examples/projectile_arena/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

fn advance(
    runner: &mut HeadlessRunner<game::PlayerAction>,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, events, viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded arena frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("arena frame failed: {failure}").into());
    }
    assert_eq!(report.fixed_ticks_attempted(), 1);
    Ok(report)
}

fn only_translation<T: Component>(
    runner: &HeadlessRunner<game::PlayerAction>,
) -> Result<Vec2, Box<dyn Error>> {
    let (entity, _) = runner
        .components::<T>()
        .next()
        .ok_or("expected one matching entity")?;
    Ok(runner.component::<Transform2d>(entity)?.translation())
}

#[test]
fn projectile_arena_covers_movement_edge_spawn_collision_and_despawn() -> Result<(), Box<dyn Error>>
{
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_application(time)?;
    let mut runner = application.build_headless(initial_world)?;

    let first_events = [
        InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
    ];
    let first = advance(&mut runner, &first_events)?;
    assert_eq!(first.spawned(), 1);
    assert_eq!(first.despawned(), 0);
    assert_eq!(
        only_translation::<game::Player>(&runner)?,
        Vec2::new(1.0, 0.0)
    );
    assert_eq!(
        only_translation::<game::Projectile>(&runner)?,
        Vec2::new(1.0, 0.0)
    );
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.0),
        Some(0)
    );

    let second_events = [InputEvent::key(
        PhysicalKeyCode::KeyD,
        ButtonState::Released,
    )];
    let second = advance(&mut runner, &second_events)?;
    assert_eq!(second.spawned(), 0);
    assert_eq!(second.despawned(), 0);
    assert_eq!(runner.components::<game::Projectile>().count(), 1);
    assert_eq!(
        only_translation::<game::Projectile>(&runner)?,
        Vec2::new(2.0, 0.0)
    );

    let third_events = [InputEvent::key(
        PhysicalKeyCode::Space,
        ButtonState::Released,
    )];
    let third = advance(&mut runner, &third_events)?;
    assert_eq!(third.spawned(), 0);
    assert_eq!(third.despawned(), 2);
    assert_eq!(runner.components::<game::Player>().count(), 1);
    assert_eq!(runner.components::<game::Projectile>().count(), 0);
    assert_eq!(runner.components::<game::Target>().count(), 0);
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.0),
        Some(1)
    );

    let extracted = runner
        .extracted_frame()
        .ok_or("successful arena frame should be extracted")?;
    assert_eq!(extracted.resolved_circles().len(), 1);

    Ok(())
}

#[test]
fn one_tick_resolves_every_projectile_instead_of_returning_after_the_first_hit()
-> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_multi_hit_regression(time)?;
    let mut runner = application.build_headless(initial_world)?;

    let report = advance(&mut runner, &[])?;

    assert_eq!(report.despawned(), 5);
    assert_eq!(runner.components::<game::Projectile>().count(), 0);
    assert_eq!(runner.components::<game::Target>().count(), 0);
    assert_eq!(
        runner.resource::<game::Score>().map(|score| score.0),
        Some(2)
    );
    Ok(())
}

#[test]
fn rejected_hit_commands_do_not_claim_or_age_live_entities() -> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial_world) = game::build_enqueue_failure_regression(time)?;
    let mut runner = application.build_headless(initial_world)?;

    for attempt in 0..2 {
        let viewport = LogicalViewport::new(800.0, 600.0)?;
        let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport));
        let FrameOutcome::Advanced(report) = outcome else {
            return Err("bounded arena failure frame was rejected before execution".into());
        };
        let Some(FrameFailure::System { stage, error }) = report.failure() else {
            return Err("command enqueue failure should stop the FixedUpdate System".into());
        };
        assert_eq!(*stage, Stage::FixedUpdate);
        assert!(
            error
                .reason()
                .contains("stage command limit of 1 was exceeded")
        );
        assert_eq!(report.fixed_ticks_attempted(), 1);
        assert_eq!(report.spawned(), 0);
        assert_eq!(report.despawned(), 0);
        let (projectiles, targets, score, claimed, lifetime) =
            game::enqueue_failure_state(&runner).ok_or("arena failure state disappeared")?;
        assert_eq!(projectiles, 1);
        assert_eq!(targets, 1);
        assert_eq!(score, 0);
        assert!(!claimed, "attempt {attempt}");
        assert_eq!(lifetime, 2, "attempt {attempt}");
    }
    Ok(())
}

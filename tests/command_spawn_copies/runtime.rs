use std::{collections::HashSet, error::Error, time::Duration};

use sim_logic::{commands::CommandBatchError, prelude::*};

const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component, Clone, Copy)]
struct CopyActor(u8);

#[derive(Component, Clone, Copy)]
struct UnapprovedCopy;

#[derive(Resource, Clone, Copy)]
struct CopySpawnTemplate(CircleVisual);

#[derive(Resource, Default)]
struct QueueOnce(bool);

#[derive(Resource, Default)]
struct BeforeBarrierCount(usize);

fn queue_zero_one_and_many(
    template: Res<CopySpawnTemplate>,
    mut once: ResMut<QueueOnce>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    if once.0 {
        return Ok(());
    }
    commands.spawn_copies(UnapprovedCopy, 0)?;
    commands.spawn_copies(CopyActor(1), 1)?;
    commands.spawn_copies((CopyActor(7), template.0), 3)?;
    once.0 = true;
    Ok(())
}

fn observe_before_barrier(actors: Query<&CopyActor>, mut observation: ResMut<BeforeBarrierCount>) {
    observation.0 = actors.iter().count();
}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

#[test]
fn public_copy_spawns_are_deferred_distinct_and_spatially_equivalent() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<CopyActor>()?;
    application.add_fallible_fixed_system(queue_zero_one_and_many);
    application.add_fixed_system(observe_before_barrier);

    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(0.5, Color::WHITE)?;
    let initial = application.register_world("copy-spawn", move |world| {
        world.spawn(camera)?;
        world.insert_resource(CopySpawnTemplate(visual))?;
        world.insert_resource(QueueOnce::default())?;
        world.insert_resource(BeforeBarrierCount::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded repeated-spawn frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.spawned(), 4);
    assert_eq!(
        runner
            .resource::<BeforeBarrierCount>()
            .ok_or("barrier observation should remain")?
            .0,
        0
    );

    let actors: Vec<_> = runner
        .components::<CopyActor>()
        .map(|(entity, actor)| (entity, actor.0))
        .collect();
    assert_eq!(
        actors.iter().map(|(_, value)| *value).collect::<Vec<_>>(),
        [1, 7, 7, 7]
    );
    assert_eq!(
        actors
            .iter()
            .map(|(entity, _)| *entity)
            .collect::<HashSet<_>>()
            .len(),
        4
    );
    let generation = actors[0].0.world_generation();
    assert!(
        actors
            .iter()
            .all(|(entity, _)| entity.world_generation() == generation)
    );
    for (entity, value) in &actors {
        if *value == 7 {
            let transform = runner.component::<Transform2d>(*entity)?;
            assert_eq!(transform.previous_translation(), Vec2::ZERO);
            assert_eq!(transform.translation(), Vec2::ZERO);
        }
    }
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("repeated visuals should extract after the barrier")?
            .resolved_circles()
            .len(),
        3
    );
    Ok(())
}

#[derive(Resource, Default)]
struct LimitPhase(u8);

fn exceed_then_fill_command_budget(mut phase: ResMut<LimitPhase>, mut commands: Commands) {
    match phase.0 {
        0 => {
            assert!(commands.spawn(CopyActor(1)).is_ok());
            assert!(matches!(
                commands.spawn_copies(CopyActor(2), 3),
                Err(CommandEnqueueError::LimitExceeded { limit: 3 })
            ));
            assert!(commands.spawn_copies(UnapprovedCopy, 0).is_ok());
        }
        1 => assert!(commands.spawn_copies(CopyActor(3), 3).is_ok()),
        _ => {}
    }
    phase.0 += 1;
}

#[test]
fn logical_command_limit_rejects_the_whole_batch_and_storage_is_reusable()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    config.set_command_limit(3)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<CopyActor>()?;
    application.add_fixed_system(exceed_then_fill_command_budget);
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("copy-spawn-limit", move |world| {
        world.spawn(camera)?;
        world.insert_resource(LimitPhase::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let first = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?));
    let FrameOutcome::Advanced(first) = first else {
        return Err("first command-limit frame was rejected".into());
    };
    assert!(matches!(
        first.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FixedUpdate,
            error: CommandBatchError::LimitExceeded { limit: 3 },
        })
    ));
    assert_eq!(first.spawned(), 0);
    assert_eq!(runner.components::<CopyActor>().count(), 0);

    let second = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?));
    let FrameOutcome::Advanced(second) = second else {
        return Err("second command-limit frame was rejected".into());
    };
    assert!(second.failure().is_none());
    assert_eq!(second.spawned(), 3);
    assert_eq!(runner.components::<CopyActor>().count(), 3);
    Ok(())
}

fn queue_beyond_entity_limit(mut commands: Commands) {
    assert!(commands.spawn_copies(CopyActor(9), 2).is_ok());
}

#[test]
fn entity_limit_rejects_every_copy_before_structural_mutation() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    config.set_entity_limit(2)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<CopyActor>()?;
    application.add_fixed_system(queue_beyond_entity_limit);
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("copy-spawn-entity-limit", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("entity-limit frame was rejected".into());
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FixedUpdate,
            error: CommandBatchError::EntityLimitExceeded {
                limit: 2,
                requested: 3,
            },
        })
    ));
    assert_eq!(report.spawned(), 0);
    assert_eq!(runner.components::<CopyActor>().count(), 0);
    Ok(())
}

use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use sim_logic::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Fail,
    Succeed,
}

#[derive(Resource)]
struct Targets {
    failing: WorldFactoryId,
    succeeding: WorldFactoryId,
}

struct PersistentProbe {
    value: usize,
    drops: Arc<AtomicUsize>,
}

impl Drop for PersistentProbe {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Resource)]
struct WorldProbe(Arc<AtomicUsize>);

impl Drop for WorldProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn request_test_target(
    input: FixedInput<TestAction>,
    targets: Option<Res<Targets>>,
    mut persistent: AppResMut<PersistentProbe>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    let Some(targets) = targets else {
        return Ok(());
    };
    for edge in input.pressed(TestAction::Fail) {
        persistent.value += 1;
        commands.replace_world(edge.intent(), targets.failing)?;
    }
    for edge in input.pressed(TestAction::Succeed) {
        persistent.value += 1;
        commands.replace_world(edge.intent(), targets.succeeding)?;
    }
    Ok(())
}

fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(640.0, 480.0)?)
}

#[test]
fn failed_preparation_keeps_the_live_value_and_repeated_commits_move_it_once_each()
-> Result<(), Box<dyn Error>> {
    let app_drops = Arc::new(AtomicUsize::new(0));
    let world_drops = Arc::new(AtomicUsize::new(0));
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.register_app_resource(PersistentProbe {
        value: 0,
        drops: app_drops.clone(),
    })?;
    application.bind_key(PhysicalKeyCode::KeyW, TestAction::Fail)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Succeed)?;

    let failing = application.register_world("failing", |_world| {
        Err(WorldBuildError::user("deliberate preparation failure"))
    })?;
    let final_camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let final_world = application.register_world("final", move |world| {
        world.spawn(final_camera)?;
        Ok(())
    })?;
    let target_camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let target_world_drops = world_drops.clone();
    let succeeding = application.register_world("succeeding", move |world| {
        world.spawn(target_camera)?;
        world.insert_resource(Targets {
            failing,
            succeeding: final_world,
        })?;
        world.insert_resource(WorldProbe(target_world_drops.clone()))?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let source_world_drops = world_drops.clone();
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(Targets {
            failing,
            succeeding,
        })?;
        world.insert_resource(WorldProbe(source_world_drops.clone()))?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FixedUpdate, request_test_target);

    let mut runner = application.build_headless(source)?;
    let old_generation = runner.world_generation();
    let fail = [InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed)];
    let FrameOutcome::Advanced(failed) = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &fail,
        viewport()?,
    )) else {
        return Err("failed-transition frame should be accepted".into());
    };
    assert!(matches!(
        failed.transition(),
        FrameTransition::PreparationFailed { target, .. } if *target == failing
    ));
    assert!(failed.failure().is_none());
    assert_eq!(runner.world_generation(), old_generation);
    assert_eq!(
        runner.app_resource::<PersistentProbe>().map(|p| p.value),
        Some(1)
    );
    assert_eq!(app_drops.load(Ordering::SeqCst), 0);
    assert_eq!(world_drops.load(Ordering::SeqCst), 0);

    let succeed = [
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Released),
        InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
    ];
    let FrameOutcome::Advanced(committed) = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &succeed,
        viewport()?,
    )) else {
        return Err("successful-transition frame should be accepted".into());
    };
    assert!(matches!(
        committed.transition(),
        FrameTransition::Committed { target, .. } if *target == succeeding
    ));
    assert!(committed.failure().is_none());
    assert_eq!(
        runner.app_resource::<PersistentProbe>().map(|p| p.value),
        Some(2)
    );
    assert_eq!(app_drops.load(Ordering::SeqCst), 0);
    assert_eq!(world_drops.load(Ordering::SeqCst), 1);

    let succeed_again = [
        InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Released),
        InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
    ];
    let FrameOutcome::Advanced(committed_again) = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &succeed_again,
        viewport()?,
    )) else {
        return Err("second successful-transition frame should be accepted".into());
    };
    assert!(matches!(
        committed_again.transition(),
        FrameTransition::Committed { target, .. } if *target == final_world
    ));
    assert!(committed_again.failure().is_none());
    assert_eq!(
        runner.app_resource::<PersistentProbe>().map(|p| p.value),
        Some(3)
    );
    assert_eq!(app_drops.load(Ordering::SeqCst), 0);
    assert_eq!(world_drops.load(Ordering::SeqCst), 2);

    drop(runner);
    assert_eq!(app_drops.load(Ordering::SeqCst), 1);
    Ok(())
}

#[derive(Resource)]
struct DualScore(usize);

#[derive(Resource, Default)]
struct Observation {
    app: usize,
    world: usize,
    app_changed: bool,
    app_added: bool,
}

fn increment_app_score(mut score: AppResMut<DualScore>) {
    score.0 += 1;
}

fn observe_namespaces(
    app: AppRes<DualScore>,
    world: Res<DualScore>,
    mut observation: ResMut<Observation>,
) {
    observation.app = app.0;
    observation.world = world.0;
    observation.app_changed = app.is_changed();
    observation.app_added = app.is_added();
}

#[test]
fn same_type_has_distinct_app_and_world_namespaces_with_stage_visibility()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.register_app_resource(DualScore(7))?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let world = application.register_world("dual-resource", move |world| {
        world.spawn(camera)?;
        world.insert_resource(DualScore(99))?;
        world.insert_resource(Observation::default())?;
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, increment_app_score);
    application.add_system(Stage::FixedUpdate, observe_namespaces);

    let mut runner = application.build_headless(world)?;
    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        outcome,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    assert_eq!(
        runner.app_resource::<DualScore>().map(|score| score.0),
        Some(8)
    );
    assert_eq!(
        runner.resource::<DualScore>().map(|score| score.0),
        Some(99)
    );
    let observation = runner
        .resource::<Observation>()
        .ok_or("observation should remain World-local")?;
    assert_eq!((observation.app, observation.world), (8, 99));
    assert!(observation.app_changed);
    assert!(observation.app_added);

    let outcome = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    ));
    assert!(matches!(
        outcome,
        FrameOutcome::Advanced(ref report) if report.failure().is_none()
    ));
    let observation = runner
        .resource::<Observation>()
        .ok_or("observation should remain World-local")?;
    assert_eq!((observation.app, observation.world), (9, 99));
    assert!(observation.app_changed);
    assert!(
        !observation.app_added,
        "application-frame completion must advance standalone ECS change tracking"
    );
    Ok(())
}

struct Missing;

fn require_missing(_missing: AppRes<Missing>) {}

fn startup_read(_score: AppRes<DualScore>) {}

fn startup_write(_score: AppResMut<DualScore>) {}

fn fallible_startup_read(_score: AppRes<DualScore>) -> Result<(), &'static str> {
    Ok(())
}

#[test]
fn missing_requirement_is_rejected_before_the_factory_runs() -> Result<(), Box<dyn Error>> {
    let factory_runs = Arc::new(AtomicUsize::new(0));
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    let observed_runs = factory_runs.clone();
    let world = application.register_world("never-run", move |_world| {
        observed_runs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })?;
    application.add_system(Stage::FixedUpdate, require_missing);

    let error = match application.build_headless(world) {
        Err(error) => error,
        Ok(_) => return Err("an unregistered AppRes dependency must reject build".into()),
    };
    let (resource, stage, system) = match error {
        RunnerBuildError::MissingApplicationResource {
            resource,
            stage,
            system,
        } => (resource, stage, system),
        error => return Err(format!("unexpected build error: {error}").into()),
    };
    assert!(resource.ends_with("::Missing"));
    assert_eq!(stage, Stage::FixedUpdate);
    assert!(system.ends_with("::require_missing"));
    assert_eq!(factory_runs.load(Ordering::SeqCst), 0);
    Ok(())
}

fn assert_startup_rejected_before_factory(kind: u8) -> Result<(), Box<dyn Error>> {
    let factory_runs = Arc::new(AtomicUsize::new(0));
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.register_app_resource(DualScore(7))?;
    let observed_runs = factory_runs.clone();
    let world = application.register_world("never-run", move |_world| {
        observed_runs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    })?;
    match kind {
        0 => {
            application.add_system(Stage::Startup, startup_read);
        }
        1 => {
            application.add_system(Stage::Startup, startup_write);
        }
        _ => {
            application.add_fallible_system(Stage::Startup, fallible_startup_read);
        }
    }

    assert!(matches!(
        application.build_headless(world),
        Err(RunnerBuildError::InitialWorld(
            CandidateFailure::SystemSetup(
                SystemSetupError::StartupApplicationResourceAccess { .. }
            )
        ))
    ));
    assert_eq!(factory_runs.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn startup_cannot_read_or_write_live_application_resources() -> Result<(), Box<dyn Error>> {
    assert_startup_rejected_before_factory(0)?;
    assert_startup_rejected_before_factory(1)?;
    assert_startup_rejected_before_factory(2)
}

#[derive(Resource, Default)]
struct LaterRuns(usize);

fn mutate_then_fail(mut score: AppResMut<DualScore>) -> Result<(), &'static str> {
    score.0 += 1;
    Err("deliberate active-system failure")
}

fn should_not_run(mut later: ResMut<LaterRuns>) {
    later.0 += 1;
}

#[test]
fn direct_app_mutation_is_not_rolled_back_by_a_later_system_result() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.register_app_resource(DualScore(7))?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let world = application.register_world("fallible", move |world| {
        world.spawn(camera)?;
        world.insert_resource(LaterRuns::default())?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FixedUpdate, mutate_then_fail);
    application.add_system(Stage::FixedUpdate, should_not_run);

    let mut runner = application.build_headless(world)?;
    let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    )) else {
        return Err("fallible frame should be accepted".into());
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    assert_eq!(
        runner.app_resource::<DualScore>().map(|score| score.0),
        Some(8)
    );
    assert_eq!(runner.resource::<LaterRuns>().map(|runs| runs.0), Some(0));
    Ok(())
}

#[derive(Component)]
struct DuplicateDespawnTarget;

fn mutate_then_reject_commands(
    mut score: AppResMut<DualScore>,
    targets: Query<LogicEntityRef, With<DuplicateDespawnTarget>>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    score.0 += 1;
    let target = targets.single()?.handle();
    commands.despawn(target)?;
    commands.despawn(target)?;
    Ok(())
}

#[test]
fn direct_app_mutation_is_not_rolled_back_by_command_batch_rejection() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(10), 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<DuplicateDespawnTarget>()?;
    application.register_app_resource(DualScore(7))?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let world = application.register_world("rejected-commands", move |world| {
        world.spawn(camera)?;
        world.spawn(DuplicateDespawnTarget)?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FixedUpdate, mutate_then_reject_commands);

    let mut runner = application.build_headless(world)?;
    let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
        Duration::from_millis(10),
        &[],
        viewport()?,
    )) else {
        return Err("command-rejection frame should be accepted".into());
    };
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    assert_eq!(
        runner.app_resource::<DualScore>().map(|score| score.0),
        Some(8)
    );
    assert_eq!(runner.components::<DuplicateDespawnTarget>().count(), 1);
    Ok(())
}

#[derive(Resource, Default)]
struct CameraBreakPhase(bool);

fn mutate_and_break_then_repair_extraction(
    mut score: AppResMut<DualScore>,
    cameras: Query<LogicEntityRef, With<ActiveCamera2d>>,
    mut phase: ResMut<CameraBreakPhase>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    score.0 += 1;
    if !phase.0 {
        commands.spawn(ActiveCamera2d::new(Camera2d::new(
            Vec2::new(1.0, 0.0),
            10.0,
        )?))?;
        phase.0 = true;
    } else {
        let extra = cameras
            .iter()
            .nth(1)
            .ok_or("the failed snapshot should retain its second camera")?;
        commands.despawn(extra.handle())?;
    }
    Ok(())
}

#[test]
fn direct_app_mutation_survives_extraction_failure_and_recovery() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.register_app_resource(DualScore(7))?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 10.0)?);
    let world = application.register_world("repairable-extraction", move |world| {
        world.spawn(camera)?;
        world.insert_resource(CameraBreakPhase::default())?;
        Ok(())
    })?;
    application.add_fallible_system(Stage::FrameUpdate, mutate_and_break_then_repair_extraction);

    let mut runner = application.build_headless(world)?;
    let FrameOutcome::Advanced(broken) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?))
    else {
        return Err("extraction-failure frame should be accepted".into());
    };
    assert!(matches!(
        broken.failure(),
        Some(FrameFailure::Extraction(
            ExtractionError::MultipleActiveCameras
        ))
    ));
    assert_eq!(
        runner.app_resource::<DualScore>().map(|score| score.0),
        Some(8)
    );

    let FrameOutcome::Advanced(repaired) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?))
    else {
        return Err("extraction-recovery frame should be accepted".into());
    };
    assert!(repaired.failure().is_none());
    assert_eq!(
        runner.app_resource::<DualScore>().map(|score| score.0),
        Some(9)
    );
    assert_eq!(runner.components::<ActiveCamera2d>().count(), 1);
    Ok(())
}

#[test]
fn initial_preparation_failure_drops_the_uninstalled_value_once() -> Result<(), Box<dyn Error>> {
    let app_drops = Arc::new(AtomicUsize::new(0));
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.register_app_resource(PersistentProbe {
        value: 0,
        drops: app_drops.clone(),
    })?;
    let failing = application.register_world("initial-failure", |_world| {
        Err(WorldBuildError::user("deliberate initial failure"))
    })?;

    assert!(matches!(
        application.build_headless(failing),
        Err(RunnerBuildError::InitialWorld(CandidateFailure::Factory(_)))
    ));
    assert_eq!(app_drops.load(Ordering::SeqCst), 1);
    Ok(())
}

use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, commands::CommandBatchError, prelude::*};

const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct Actor;

#[derive(Resource)]
struct ToggleState {
    target: LogicEntity,
    phase: u8,
}

#[derive(Resource, Default)]
struct ActiveObservations(Vec<usize>);

fn toggle_each_frame(
    mut state: ResMut<ToggleState>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    match state.phase {
        0 => commands.disable(state.target)?,
        1 => commands.enable(state.target)?,
        _ => return Ok(()),
    }
    state.phase += 1;
    Ok(())
}

fn observe_active_actors(actors: Query<&Actor>, mut observations: ResMut<ActiveObservations>) {
    observations.0.push(actors.iter().count());
}

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded enablement frame was rejected".into());
    };
    Ok(report)
}

#[test]
fn frame_commands_toggle_extraction_through_a_saved_handle_without_manual_approval()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Actor>()?;
    application.add_fallible_frame_system(toggle_each_frame);
    application.add_frame_system(observe_active_actors);

    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(1.0, Color::WHITE)?;
    let initial = application.register_world("frame-enablement", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Actor, visual))?;
        world.insert_resource(ToggleState { target, phase: 0 })?;
        world.insert_resource(ActiveObservations::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;
    let target = runner
        .resource::<ToggleState>()
        .ok_or("toggle state should exist")?
        .target;
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("initial extraction should exist")?
            .resolved_circles()
            .len(),
        1
    );

    let disabled = advance(&mut runner, Duration::ZERO)?;
    assert!(disabled.failure().is_none());
    assert!(runner.component::<Disabled>(target).is_ok());
    assert!(runner.component::<Actor>(target).is_ok());
    assert!(
        runner
            .extracted_frame()
            .ok_or("disabled frame should extract")?
            .resolved_circles()
            .is_empty()
    );

    let enabled = advance(&mut runner, Duration::ZERO)?;
    assert!(enabled.failure().is_none());
    assert!(matches!(
        runner.component::<Disabled>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    let [visible] = runner
        .extracted_frame()
        .ok_or("enabled frame should extract")?
        .resolved_circles()
    else {
        return Err("enabled actor should be visible".into());
    };
    assert_eq!(visible.source(), target);

    let observed = advance(&mut runner, Duration::ZERO)?;
    assert!(observed.failure().is_none());
    assert_eq!(
        runner
            .resource::<ActiveObservations>()
            .ok_or("active observations should remain")?
            .0,
        [1, 0, 1],
        "each System invocation must see the state from before its stage barrier"
    );
    Ok(())
}

fn toggle_each_fixed_tick(
    mut state: ResMut<ToggleState>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    match state.phase {
        0 => commands.disable(state.target)?,
        1 => commands.enable(state.target)?,
        _ => return Ok(()),
    }
    state.phase += 1;
    Ok(())
}

#[test]
fn fixed_barriers_expose_disable_then_enable_between_catch_up_ticks() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 2)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Actor>()?;
    application.add_fallible_fixed_system(toggle_each_fixed_tick);
    application.add_fixed_system(observe_active_actors);
    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(1.0, Color::WHITE)?;
    let initial = application.register_world("fixed-enablement", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Actor, visual))?;
        world.insert_resource(ToggleState { target, phase: 0 })?;
        world.insert_resource(ActiveObservations::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let report = advance(&mut runner, FIXED_STEP * 2)?;
    assert!(report.failure().is_none());
    assert_eq!(report.fixed_ticks_attempted(), 2);
    assert_eq!(
        runner
            .resource::<ActiveObservations>()
            .ok_or("active observations should remain")?
            .0,
        [1, 0]
    );
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("catch-up frame should extract")?
            .resolved_circles()
            .len(),
        1
    );
    Ok(())
}

#[test]
fn startup_disable_shapes_the_candidate_before_first_extraction() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Actor>()?;
    application.add_system(
        Stage::Startup,
        |state: Res<ToggleState>, mut commands: Commands| {
            assert!(commands.disable(state.target).is_ok());
        },
    );
    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(1.0, Color::WHITE)?;
    let initial = application.register_world("startup-disable", move |world| {
        world.spawn(camera)?;
        let target = world.spawn((Actor, visual))?;
        world.insert_resource(ToggleState { target, phase: 0 })?;
        Ok(())
    })?;
    let runner = application.build_headless(initial)?;
    let target = runner
        .resource::<ToggleState>()
        .ok_or("toggle state should exist")?
        .target;

    assert!(runner.component::<Disabled>(target).is_ok());
    assert!(runner.component::<Actor>(target).is_ok());
    assert!(
        runner
            .extracted_frame()
            .ok_or("initial extraction should exist")?
            .resolved_circles()
            .is_empty()
    );
    Ok(())
}

#[derive(Resource)]
struct OrderedTargets {
    disable_then_enable: LogicEntity,
    enable_then_disable: LogicEntity,
    repeated_disable: LogicEntity,
    repeated_enable: LogicEntity,
}

fn queue_ordered_toggles(
    targets: Res<OrderedTargets>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    commands.disable(targets.disable_then_enable)?;
    commands.enable(targets.disable_then_enable)?;
    commands.enable(targets.enable_then_disable)?;
    commands.disable(targets.enable_then_disable)?;
    commands.disable(targets.repeated_disable)?;
    commands.disable(targets.repeated_disable)?;
    commands.enable(targets.repeated_enable)?;
    commands.enable(targets.repeated_enable)?;
    Ok(())
}

#[test]
fn repeated_and_mixed_toggle_calls_follow_command_order() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Actor>()?;
    application.add_fallible_frame_system(queue_ordered_toggles);
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("ordered-enablement", move |world| {
        world.spawn(camera)?;
        let disable_then_enable = world.spawn(Actor)?;
        let enable_then_disable = world.spawn(Actor)?;
        let repeated_disable = world.spawn(Actor)?;
        let repeated_enable = world.spawn(Actor)?;
        world.insert_resource(OrderedTargets {
            disable_then_enable,
            enable_then_disable,
            repeated_disable,
            repeated_enable,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let report = advance(&mut runner, Duration::ZERO)?;
    assert!(report.failure().is_none());
    let targets = runner
        .resource::<OrderedTargets>()
        .ok_or("ordered targets should remain")?;
    assert!(matches!(
        runner.component::<Disabled>(targets.disable_then_enable),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(
        runner
            .component::<Disabled>(targets.enable_then_disable)
            .is_ok()
    );
    assert!(
        runner
            .component::<Disabled>(targets.repeated_disable)
            .is_ok()
    );
    assert!(matches!(
        runner.component::<Disabled>(targets.repeated_enable),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(runner.components::<Actor>().count(), 4);
    Ok(())
}

fn run_despawn_conflict(initially_disabled: bool) -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Actor>()?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("enablement-conflict", move |world| {
        world.spawn(camera)?;
        let target = if initially_disabled {
            world.spawn((Actor, Disabled))?
        } else {
            world.spawn(Actor)?
        };
        world.insert_resource(ToggleState { target, phase: 0 })?;
        Ok(())
    })?;
    application.add_system(
        Stage::FrameUpdate,
        move |state: Res<ToggleState>, mut commands: Commands| {
            let toggled = if initially_disabled {
                commands.enable(state.target)
            } else {
                commands.disable(state.target)
            };
            assert!(toggled.is_ok());
            assert!(commands.despawn(state.target).is_ok());
        },
    );
    let mut runner = application.build_headless(initial)?;
    let target = runner
        .resource::<ToggleState>()
        .ok_or("toggle state should exist")?
        .target;

    let report = advance(&mut runner, Duration::ZERO)?;
    assert!(matches!(
        report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: CommandBatchError::ConflictingEntityCommands { entity },
        }) if *entity == target
    ));
    assert!(runner.component::<Actor>(target).is_ok());
    assert_eq!(
        runner.component::<Disabled>(target).is_ok(),
        initially_disabled,
        "the rejected batch must preserve the complete prior state"
    );
    Ok(())
}

#[test]
fn enable_and_disable_keep_the_existing_despawn_conflict_atomic() -> Result<(), Box<dyn Error>> {
    run_despawn_conflict(false)?;
    run_despawn_conflict(true)
}

#[test]
fn explicit_disabled_approval_remains_idempotent() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Disabled>()?;
    application.approve_component::<Disabled>()?;
    application.approve_component::<Actor>()?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("explicit-disabled-approval", move |world| {
        world.spawn(camera)?;
        world.spawn((Actor, Disabled))?;
        Ok(())
    })?;
    let runner = application.build_headless(initial)?;
    assert_eq!(runner.components::<Actor>().count(), 1);
    assert_eq!(runner.components::<Disabled>().count(), 1);
    Ok(())
}

#[derive(Resource)]
struct InvalidTargets {
    foreign: LogicEntity,
    missing: LogicEntity,
    phase: u8,
}

fn queue_invalid_toggle(
    mut targets: ResMut<InvalidTargets>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    match targets.phase {
        0 => commands.disable(targets.foreign)?,
        1 => commands.enable(targets.missing)?,
        _ => return Ok(()),
    }
    targets.phase += 1;
    Ok(())
}

#[test]
fn toggles_preserve_foreign_and_missing_target_diagnostics() -> Result<(), Box<dyn Error>> {
    let mut foreign_application = Application::<TestAction>::new(AppConfig::default())?;
    foreign_application.approve_component::<Actor>()?;
    let camera = ActiveCamera2d::centered(20.0)?;
    let foreign_world = foreign_application.register_world("foreign-enablement", move |world| {
        world.spawn(camera)?;
        world.spawn(Actor)?;
        Ok(())
    })?;
    let foreign_runner = foreign_application.build_headless(foreign_world)?;
    let foreign = foreign_runner
        .components::<Actor>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("foreign actor should exist")?;

    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Actor>()?;
    application.add_fallible_frame_system(queue_invalid_toggle);
    let camera = ActiveCamera2d::centered(20.0)?;
    let initial = application.register_world("invalid-enablement", move |world| {
        world.spawn(camera)?;
        let missing = world.spawn(Actor)?;
        world.despawn(missing)?;
        world.insert_resource(InvalidTargets {
            foreign,
            missing,
            phase: 0,
        })?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let foreign_report = advance(&mut runner, Duration::ZERO)?;
    assert!(matches!(
        foreign_report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: CommandBatchError::ForeignEntity { entity },
        }) if *entity == foreign
    ));

    let missing_report = advance(&mut runner, Duration::ZERO)?;
    let missing = runner
        .resource::<InvalidTargets>()
        .ok_or("invalid targets should remain")?
        .missing;
    assert!(matches!(
        missing_report.failure(),
        Some(FrameFailure::Commands {
            stage: Stage::FrameUpdate,
            error: CommandBatchError::MissingEntity { entity },
        }) if *entity == missing
    ));
    Ok(())
}

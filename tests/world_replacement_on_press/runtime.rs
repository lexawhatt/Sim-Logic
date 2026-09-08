use std::{
    error::Error,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use sim_logic::prelude::*;
use sim_logic::transition::TransitionRejection;

const FIXED_STEP: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Navigate,
}

#[derive(Default)]
struct ScheduleProbe {
    fixed_after_route: usize,
    frame_runs: usize,
}

#[derive(Default)]
struct ExitOnce(bool);

fn observe_fixed_after_route(mut probe: AppResMut<ScheduleProbe>) {
    probe.fixed_after_route += 1;
}

fn observe_frame(mut probe: AppResMut<ScheduleProbe>) {
    probe.frame_runs += 1;
}

fn request_exit_once(
    mut exit: AppResMut<ExitOnce>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    if !exit.0 {
        exit.0 = true;
        commands.request_exit()?;
    }
    Ok(())
}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    elapsed: Duration,
    events: &[InputEvent],
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, events, viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded replacement frame was rejected".into());
    };
    Ok(report)
}

fn fixed_config(max_fixed_ticks: u32) -> Result<AppConfig, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, max_fixed_ticks)?);
    Ok(config)
}

#[test]
fn retained_press_replaces_once_after_all_current_tick_systems_and_target_without_route_is_safe()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(4)?)?;
    application.register_app_resource(ScheduleProbe::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    application.add_fixed_system(observe_fixed_after_route);
    application.add_frame_system(observe_frame);

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();

    let retained = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert_eq!(retained.fixed_ticks_attempted(), 0);
    assert!(matches!(retained.transition(), FrameTransition::None));
    assert_eq!(runner.active_world_name(), "source");
    assert_eq!(
        runner
            .app_resource::<ScheduleProbe>()
            .ok_or("schedule probe should remain")?
            .frame_runs,
        1
    );

    let committed = advance(&mut runner, FIXED_STEP * 3, &[])?;
    let FrameTransition::Committed {
        old,
        new,
        target: committed_target,
        warning,
    } = committed.transition()
    else {
        return Err("retained fixed press should commit its target".into());
    };
    assert_eq!(*old, source_generation);
    assert_eq!(*new, runner.world_generation());
    assert_eq!(*committed_target, target);
    assert!(warning.is_none());
    assert_eq!(committed.fixed_ticks_attempted(), 1);
    assert_eq!(committed.extracted_generation(), Some(*new));
    assert_eq!(runner.active_world_name(), "target");
    let probe = runner
        .app_resource::<ScheduleProbe>()
        .ok_or("schedule probe should survive replacement")?;
    assert_eq!(probe.fixed_after_route, 1);
    assert_eq!(probe.frame_runs, 1);
    assert_eq!(runner.lifecycle().len(), 3);
    assert_eq!(runner.lifecycle()[0].event(), LifecycleEvent::WorldEnter);
    assert_eq!(runner.lifecycle()[1].event(), LifecycleEvent::WorldExit);
    assert_eq!(runner.lifecycle()[2].event(), LifecycleEvent::WorldEnter);

    let target_release = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Released,
        )],
    )?;
    assert!(target_release.failure().is_none());
    assert!(matches!(target_release.transition(), FrameTransition::None));

    let target_press_without_route = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(target_press_without_route.failure().is_none());
    assert!(matches!(
        target_press_without_route.transition(),
        FrameTransition::None
    ));
    assert_eq!(target_press_without_route.fixed_ticks_attempted(), 1);
    let probe = runner
        .app_resource::<ScheduleProbe>()
        .ok_or("schedule probe should remain in target")?;
    assert_eq!(probe.fixed_after_route, 3);
    assert_eq!(probe.frame_runs, 3);
    Ok(())
}

#[test]
fn separate_physical_presses_preserve_distinct_intents_and_converge() -> Result<(), Box<dyn Error>>
{
    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.bind_key(PhysicalKeyCode::Space, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("convergent-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("convergent-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let report = advance(
        &mut runner,
        FIXED_STEP,
        &[
            InputEvent::key(PhysicalKeyCode::Enter, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
        ],
    )?;
    let FrameTransition::Committed {
        target: committed,
        warning: Some(warning),
        ..
    } = report.transition()
    else {
        return Err("independent presses should converge on one target".into());
    };
    assert_eq!(*committed, target);
    assert_eq!(warning.distinct_intents(), 2);
    assert_eq!(runner.active_world_name(), "convergent-target");
    Ok(())
}

#[test]
fn duplicate_adapter_coalesces_the_same_input_intent_without_a_warning()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    application.add_world_replacement_on_press_system();

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("duplicate-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("duplicate-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;

    let report = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed {
            target: committed,
            warning: None,
            ..
        } if *committed == target
    ));
    assert_eq!(runner.active_world_name(), "duplicate-target");
    Ok(())
}

#[test]
fn duplicate_adapter_respects_command_limit_discards_prefix_and_does_not_retry_edge()
-> Result<(), Box<dyn Error>> {
    let mut config = fixed_config(4)?;
    config.set_command_limit(1)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    application.add_world_replacement_on_press_system();

    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("limited-target", move |world| {
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("limited-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();

    let failed = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::System {
            stage: Stage::FixedUpdate,
            ..
        })
    ));
    assert!(matches!(failed.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "limited-source");

    let next = advance(&mut runner, FIXED_STEP * 3, &[])?;
    assert!(next.failure().is_none());
    assert!(matches!(next.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "limited-source");
    Ok(())
}

#[test]
fn candidate_failure_preserves_source_and_does_not_retry_the_consumed_press()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();

    let failing = application.register_world("failing-target", |_world| {
        Err(WorldBuildError::User {
            message: "expected candidate failure".into(),
        })
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("preserved-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, failing))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();

    let failed = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        failed.transition(),
        FrameTransition::PreparationFailed { target, .. } if *target == failing
    ));
    assert_eq!(failed.extracted_generation(), Some(source_generation));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "preserved-source");

    let next = advance(&mut runner, FIXED_STEP, &[])?;
    assert!(next.failure().is_none());
    assert!(matches!(next.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);
    Ok(())
}

#[test]
fn foreign_factory_is_validated_only_on_press_and_preserves_the_source()
-> Result<(), Box<dyn Error>> {
    let mut foreign_application = Application::<TestAction>::new(fixed_config(1)?)?;
    let foreign_camera = ActiveCamera2d::centered(20.0)?;
    let foreign = foreign_application.register_world("foreign-target", move |world| {
        world.spawn(foreign_camera)?;
        Ok(())
    })?;

    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("invalid-route-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, foreign))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();

    let report = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        report.transition(),
        FrameTransition::Invalid(TransitionRequestFailure::InvalidFactory)
    ));
    assert!(report.failure().is_none());
    assert_eq!(report.extracted_generation(), Some(source_generation));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "invalid-route-source");
    Ok(())
}

#[derive(Resource)]
struct AlternateTarget(WorldFactoryId);

fn request_alternate_target(
    input: FixedInput<TestAction>,
    alternate: Res<AlternateTarget>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    for edge in input.pressed(TestAction::Navigate) {
        commands.replace_world(edge.intent(), alternate.0)?;
    }
    Ok(())
}

fn choose_alternate(
    alternate: Res<AlternateTarget>,
    mut route: ResMut<WorldReplacementOnPress<TestAction>>,
) {
    *route = WorldReplacementOnPress::new(TestAction::Navigate, alternate.0);
}

fn run_route_order_case(reroute_first: bool) -> Result<&'static str, Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;

    let first_camera = ActiveCamera2d::centered(20.0)?;
    let first = application.register_world("first-target", move |world| {
        world.spawn(first_camera)?;
        Ok(())
    })?;
    let alternate_camera = ActiveCamera2d::centered(20.0)?;
    let alternate = application.register_world("alternate-target", move |world| {
        world.spawn(alternate_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("ordered-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, first))?;
        world.insert_resource(AlternateTarget(alternate))?;
        Ok(())
    })?;
    if reroute_first {
        application.add_fixed_system(choose_alternate);
        application.add_world_replacement_on_press_system();
    } else {
        application.add_world_replacement_on_press_system();
        application.add_fixed_system(choose_alternate);
    }
    let mut runner = application.build_headless(source)?;

    let report = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    Ok(match runner.active_world_name() {
        "first-target" => "first-target",
        "alternate-target" => "alternate-target",
        _ => return Err("route order selected an unexpected World".into()),
    })
}

#[test]
fn route_resource_replacement_obeys_fixed_system_registration_order() -> Result<(), Box<dyn Error>>
{
    assert_eq!(run_route_order_case(true)?, "alternate-target");
    assert_eq!(run_route_order_case(false)?, "first-target");
    Ok(())
}

#[test]
fn custom_request_with_the_same_press_token_uses_normal_malformed_intent_arbitration()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(4)?)?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    application.add_fallible_fixed_system(request_alternate_target);

    let first_camera = ActiveCamera2d::centered(20.0)?;
    let first = application.register_world("conflict-first", move |world| {
        world.spawn(first_camera)?;
        Ok(())
    })?;
    let alternate_camera = ActiveCamera2d::centered(20.0)?;
    let alternate = application.register_world("conflict-alternate", move |world| {
        world.spawn(alternate_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("conflict-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, first))?;
        world.insert_resource(AlternateTarget(alternate))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();
    let lifecycle_len = runner.lifecycle().len();

    let rejected = advance(
        &mut runner,
        FIXED_STEP * 3,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        rejected.transition(),
        FrameTransition::Rejected(TransitionRejection::MalformedIntent)
    ));
    assert!(rejected.failure().is_none());
    assert_eq!(rejected.fixed_ticks_attempted(), 1);
    assert!(rejected.dropped_for_transition().is_some());
    assert_eq!(rejected.extracted_generation(), Some(source_generation));
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "conflict-source");
    assert_eq!(runner.lifecycle().len(), lifecycle_len);

    let next = advance(&mut runner, FIXED_STEP, &[])?;
    assert!(next.failure().is_none());
    assert_eq!(next.fixed_ticks_attempted(), 1);
    assert!(matches!(next.transition(), FrameTransition::None));
    assert_eq!(runner.world_generation(), source_generation);
    Ok(())
}

#[test]
fn exit_in_the_same_batch_suppresses_the_adapter_request_and_consumes_the_press()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(fixed_config(1)?)?;
    application.register_app_resource(ExitOnce::default())?;
    application.bind_key(PhysicalKeyCode::Enter, TestAction::Navigate)?;
    application.add_world_replacement_on_press_system();
    application.add_fallible_fixed_system(request_exit_once);

    let target_builds = Arc::new(AtomicUsize::new(0));
    let observed_target_builds = Arc::clone(&target_builds);
    let target_camera = ActiveCamera2d::centered(20.0)?;
    let target = application.register_world("suppressed-target", move |world| {
        observed_target_builds.fetch_add(1, Ordering::SeqCst);
        world.spawn(target_camera)?;
        Ok(())
    })?;
    let source_camera = ActiveCamera2d::centered(20.0)?;
    let source = application.register_world("exit-source", move |world| {
        world.spawn(source_camera)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Navigate, target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(source)?;
    let source_generation = runner.world_generation();
    let lifecycle_len = runner.lifecycle().len();

    let exited = advance(
        &mut runner,
        FIXED_STEP,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    assert!(exited.failure().is_none());
    assert!(exited.exit_requested());
    assert_eq!(exited.transitions_suppressed_by_exit(), 1);
    assert!(matches!(exited.transition(), FrameTransition::None));
    assert_eq!(target_builds.load(Ordering::SeqCst), 0);
    assert_eq!(runner.world_generation(), source_generation);
    assert_eq!(runner.active_world_name(), "exit-source");
    assert_eq!(runner.lifecycle().len(), lifecycle_len);

    let next = advance(&mut runner, FIXED_STEP, &[])?;
    assert!(next.failure().is_none());
    assert_eq!(next.fixed_ticks_attempted(), 1);
    assert!(!next.exit_requested());
    assert!(matches!(next.transition(), FrameTransition::None));
    assert_eq!(target_builds.load(Ordering::SeqCst), 0);
    assert_eq!(runner.world_generation(), source_generation);
    Ok(())
}

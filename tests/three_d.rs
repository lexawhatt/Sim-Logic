use std::time::Duration;

use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {
    Toggle,
    Replace,
}

fn box_visual() -> LogicResult<CuboidVisual3d> {
    Ok(CuboidVisual3d::new(
        Vec3::ZERO,
        Vec3::new(2.0, 1.0, 3.0)?,
        Color::WHITE,
    )?)
}

fn view() -> LogicResult<View3d> {
    Ok(View3d::new(Vec3::new(4.0, 6.0, 12.0)?, Vec3::ZERO)?)
}

fn app(limit: usize) -> LogicResult<Application<TestAction>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(Duration::from_millis(100), 4)?);
    config.set_three_d_render_limits(ThreeDRenderLimits::new(limit, limit * 12, 4096 * 4096));
    Ok(Application::new(config)?)
}

fn advance(
    runner: &mut HeadlessRunner<TestAction>,
    delta: Duration,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    match runner.advance_frame(FrameRequest::new(
        delta,
        events,
        LogicalViewport::new(1280.0, 720.0)?,
    )) {
        FrameOutcome::Advanced(report) => Ok(report),
        FrameOutcome::Rejected(error) => Err(error.into()),
    }
}

fn snapshot(runner: &HeadlessRunner<TestAction>) -> LogicResult<&ExtractedFrame> {
    runner
        .extracted_frame()
        .ok_or_else(|| "expected complete snapshot".into())
}

#[test]
fn missing_or_disabled_view_keeps_managed_cuboids_without_3d_work() -> LogicResult {
    assert_eq!(
        RenderLimits::default().three_d(),
        ThreeDRenderLimits::default()
    );
    for has_view in [false, true] {
        let mut app = app(0)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let visual = box_visual()?;
        let mut view = view()?;
        view.set_enabled(false);
        let world = app.register_world("2D", move |world| {
            world.spawn(camera)?;
            world.spawn(visual)?;
            if has_view {
                world.insert_resource(view)?;
            }
            Ok(())
        })?;
        let runner = app.build_headless(world)?;
        assert_eq!(runner.components::<CuboidVisual3d>().count(), 1);
        assert!(snapshot(&runner)?.three_d().is_none());
        assert!(snapshot(&runner)?.resolved_cuboids().is_empty());
    }
    Ok(())
}

#[test]
fn cuboid_is_standard_component_and_disabled_or_hidden_sources_do_not_count() -> LogicResult {
    let mut app = app(1)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = box_visual()?;
    let mut hidden = visual;
    hidden.set_visible(false);
    let view = view()?;
    let world = app.register_world("3D", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        world.spawn((visual, Disabled))?;
        world.spawn(hidden)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let runner = app.build_headless(world)?;
    let frame = snapshot(&runner)?;
    assert_eq!(frame.resolved_cuboids().len(), 1);
    assert_eq!(frame.three_d().ok_or("view")?.view(), view);
    assert_eq!(frame.resolved_cuboids()[0].visual(), visual);
    assert_eq!(
        frame.resolved_cuboids()[0].source().world_generation(),
        runner.world_generation()
    );
    assert_eq!(runner.components::<Transform2d>().count(), 0);
    Ok(())
}

#[test]
fn enabling_over_budget_3d_preserves_complete_prior_2d_snapshot_then_recovers() -> LogicResult {
    let mut app = app(0)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Toggle)?;
    app.add_fallible_frame_system(
        |input: FrameInput<TestAction>,
         mut view: ResMut<View3d>,
         mut panels: Query<&mut ScreenRectangleVisual>|
         -> LogicResult {
            if input.has_press_occurrence(TestAction::Toggle) {
                view.set_enabled(true);
                for mut panel in &mut panels {
                    panel.set_color(Color::BLACK)?;
                }
            }
            if input.has_release_occurrence(TestAction::Toggle) {
                view.set_enabled(false);
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = box_visual()?;
    let panel = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(20.0, 20.0),
        Color::WHITE,
    )?;
    let mut view = view()?;
    view.set_enabled(false);
    let world = app.register_world("atomic", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        world.spawn(panel)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(world)?;
    let failed = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(matches!(
        failed.failure(),
        Some(FrameFailure::Extraction(ExtractionError::ThreeD(
            ThreeDExtractionError::LimitExceeded {
                resource: ThreeDLimitResource::Cuboids,
                ..
            }
        )))
    ));
    assert!(snapshot(&runner)?.three_d().is_none());
    assert_eq!(
        snapshot(&runner)?.resolved_screen_rectangles()[0].color(),
        Color::WHITE
    );
    let recovered = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    assert!(recovered.failure().is_none());
    assert!(snapshot(&runner)?.three_d().is_none());
    assert_eq!(
        snapshot(&runner)?.resolved_screen_rectangles()[0].color(),
        Color::BLACK
    );
    Ok(())
}

#[test]
fn view_switch_samples_same_entity_and_frame_updated_geometry() -> LogicResult {
    let mut app = app(1)?;
    app.bind_key(PhysicalKeyCode::Space, TestAction::Toggle)?;
    app.add_fallible_frame_system(
        |input: FrameInput<TestAction>,
         mut view: ResMut<View3d>,
         mut boxes: Query<&mut CuboidVisual3d>|
         -> LogicResult {
            if input.has_press_occurrence(TestAction::Toggle) {
                view.set_enabled(false);
            }
            if input.has_release_occurrence(TestAction::Toggle) {
                view.set_enabled(true);
                for mut visual in &mut boxes {
                    visual.set_transform(Transform3d::new(
                        Vec3::new(1.0, 2.0, 3.0)?,
                        Rotation3d::IDENTITY,
                        Vec3::new(2.0, 1.0, 3.0)?,
                    )?)?;
                }
            }
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = box_visual()?;
    let view = view()?;
    let world = app.register_world("switch", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(world)?;
    let source = snapshot(&runner)?.resolved_cuboids()[0].source();
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(snapshot(&runner)?.three_d().is_none());
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Released,
        )],
    )?;
    let resolved = snapshot(&runner)?.resolved_cuboids()[0];
    assert_eq!(resolved.source(), source);
    assert_eq!(
        resolved.transform().translation(),
        Vec3::new(1.0, 2.0, 3.0)?
    );
    Ok(())
}

#[test]
fn failed_candidate_3d_does_not_publish_over_active_view() -> LogicResult {
    let mut app = app(1)?;
    app.bind_key(PhysicalKeyCode::Enter, TestAction::Replace)?;
    app.add_world_replacement_on_press_system();
    let camera = ActiveCamera2d::centered(1.0)?;
    let visual = box_visual()?;
    let view = view()?;
    let target = app.register_world("too-many", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        world.spawn(visual)?;
        world.insert_resource(view)?;
        Ok(())
    })?;
    let initial = app.register_world("initial", move |world| {
        world.spawn(camera)?;
        world.spawn(visual)?;
        world.insert_resource(view)?;
        world.insert_resource(WorldReplacementOnPress::new(TestAction::Replace, target))?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let generation = runner.world_generation();
    let source = snapshot(&runner)?.resolved_cuboids()[0].source();
    advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
    )?;
    let report = advance(&mut runner, Duration::from_millis(300), &[])?;
    assert!(matches!(
        report.transition(),
        FrameTransition::PreparationFailed { .. }
    ));
    assert_eq!(runner.world_generation(), generation);
    assert_eq!(snapshot(&runner)?.resolved_cuboids()[0].source(), source);
    Ok(())
}

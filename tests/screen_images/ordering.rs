use super::support::*;
use sim_engine::{Layer, SceneBudget};
use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Component)]
struct Migrated;
#[derive(Resource)]
struct Tie(LogicEntity);

#[test]
fn mixed_screen_plan_preserves_layers_signed_zero_ties_and_archetype_changes() -> LogicResult {
    let mut app = application(4)?;
    let id = app.register_image_rgba8(2, 2, &PIXELS)?;
    app.approve_component::<Migrated>()?;
    app.add_fallible_frame_system(|tie: Res<Tie>, mut commands: Commands| {
        commands.insert(tie.0, Migrated)
    });
    let camera = ActiveCamera2d::centered(48.0)?;
    let mut bottom = rectangle(10.0, 100.0)?;
    bottom.set_layer(Layer::new(-1));
    let dual_rectangle = rectangle(20.0, 0.0)?;
    let dual_image = image(id, 20.0, -0.0)?;
    let mut next = image(id, 30.0, 1.0)?;
    next.set_source_region(Some(ImageRegion::new(1, 0, 1, 2)?))?;
    next.set_filter(ImageFilter::Linear);
    next.set_tint(Color::rgba(0.5, 0.25, 1.0, 0.75))?;
    let top = rectangle(40.0, 2.0)?;
    let initial = app.register_world("mixed", move |world| {
        world.spawn(camera)?;
        world.spawn(top)?;
        world.spawn(next)?;
        let tie = world.spawn((dual_image, dual_rectangle))?;
        world.insert_resource(Tie(tie))?;
        world.spawn(bottom)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let expected = [
        ScreenDraw::Rectangles { run: 0 },
        ScreenDraw::Image { index: 0 },
        ScreenDraw::Image { index: 1 },
        ScreenDraw::Rectangles { run: 1 },
    ];
    for _ in 0..3 {
        let frame = snapshot(&runner)?;
        assert_eq!(frame.screen_draws(), expected);
        let first = frame.screen_rectangle_run_records(0).ok_or("first run")?;
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].position(), bottom.position());
        assert_eq!(
            first[1].source(),
            frame.resolved_screen_images()[0].source()
        );
        assert_eq!(
            frame.screen_rectangle_run_records(1).ok_or("last run")?[0].position(),
            top.position()
        );
        assert!(frame.screen_rectangle_run_records(2).is_none());
        let row = frame.resolved_screen_images()[1];
        assert_eq!(row.source_region(), next.source_region());
        assert_eq!(row.tint(), next.tint());
        assert_eq!(row.filter(), ImageFilter::Linear);
        assert!(
            advance(&mut runner, Duration::ZERO, &[])?
                .failure()
                .is_none()
        );
    }
    Ok(())
}

#[test]
fn rectangle_budget_is_aggregate_even_when_images_split_the_rectangles() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(2)
            .with_screen_scene_budget(SceneBudget::new(1, 0, 1000, 100000, 100000, 100000, 1000)),
    );
    let mut app = Application::<TestAction>::new(config)?;
    let id = app.register_image_rgba8(2, 2, &PIXELS)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let before = rectangle(0.0, 0.0)?;
    let between = image(id, 10.0, 1.0)?;
    let after = rectangle(20.0, 2.0)?;
    let initial = app.register_world("aggregate", move |world| {
        world.spawn(camera)?;
        world.spawn(before)?;
        world.spawn(between)?;
        world.spawn(after)?;
        Ok(())
    })?;
    assert!(matches!(
        app.build_headless(initial),
        Err(RunnerBuildError::InitialWorld(
            CandidateFailure::Extraction(ExtractionError::ScreenScene(_))
        ))
    ));
    Ok(())
}

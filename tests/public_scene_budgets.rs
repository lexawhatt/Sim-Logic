//! This same test also runs as an independent Logic-only consumer crate.
use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}
#[derive(Resource, Default)]
struct Tick(u8);
#[derive(Component)]
struct Extra;

#[test]
fn custom_public_budget_admits_ten_thousand_plus_and_keeps_last_complete_frame() -> LogicResult {
    const COUNT: usize = 10_001;
    let mut config = AppConfig::default();
    config.set_entity_limit(COUNT + 2)?;
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_rectangles(COUNT + 1)
            .with_screen_scene_budget(SceneBudget::new(
                COUNT,
                0,
                COUNT * 12,
                16 * 1024 * 1024,
                32 * 1024 * 1024,
                32 * 1024 * 1024,
                COUNT,
            )),
    );
    let mut app = Application::<Action>::new(config)?;
    app.approve_component::<Extra>()?;
    let rectangle = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(1.0, 1.0),
        LogicalScreenVector::new(2.0, 2.0),
        Color::WHITE,
    )?;
    app.add_fallible_frame_system(
        move |mut tick: ResMut<Tick>,
              extra: Query<LogicEntityRef, With<Extra>>,
              mut commands: Commands|
              -> LogicResult {
            if tick.0 == 0 {
                commands.spawn((rectangle, Extra))?;
            }
            if tick.0 == 1 {
                for entity in &extra {
                    commands.despawn(entity.handle())?;
                }
            }
            tick.0 += 1;
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("large-screen", move |world| {
        world.spawn(camera)?;
        for _ in 0..COUNT {
            world.spawn(rectangle)?;
        }
        world.insert_resource(Tick::default())?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let generation = runner.world_generation();
    assert_eq!(
        runner
            .extracted_frame()
            .unwrap()
            .screen_primitives()
            .count(),
        COUNT
    );
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let FrameOutcome::Advanced(rejected) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
    else {
        return Err("frame rejected before extraction".into());
    };
    assert!(matches!(
        rejected.failure(),
        Some(FrameFailure::Extraction(ExtractionError::ScreenScene(
            SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands,
                limit: COUNT,
                requested: 10_002
            }
        )))
    ));
    let retained = runner.extracted_frame().unwrap();
    assert_eq!(retained.world_generation(), generation);
    assert_eq!(retained.screen_primitives().count(), COUNT);
    let FrameOutcome::Advanced(recovered) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
    else {
        return Err("recovery rejected".into());
    };
    assert!(recovered.failure().is_none());
    assert_eq!(
        runner
            .extracted_frame()
            .unwrap()
            .screen_primitives()
            .count(),
        COUNT
    );
    Ok(())
}

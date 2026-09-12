use super::support::*;
use sim_engine::Layer;
use sim_logic::prelude::*;
use std::time::Duration;

#[derive(Component)]
struct Migrated;

#[derive(Resource)]
struct TiedEntity(LogicEntity);

fn rectangle(depth: f32) -> LogicResult<ScreenRectangleVisual> {
    let mut visual = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenVector::new(80.0, 40.0),
        Color::WHITE,
    )?;
    visual.set_draw_order_depth(depth)?;
    Ok(visual)
}

#[test]
fn triple_ties_and_interleaved_text_preserve_mixed_order_after_archetype_moves() -> LogicResult {
    let mut app = application(limits(4, 128, 64))?;
    let font = font(&mut app)?;
    let image_id = app.register_image_rgba8(1, 1, &[255; 4])?;
    app.approve_component::<Migrated>()?;
    app.add_fallible_frame_system(|target: Res<TiedEntity>, mut commands: Commands| {
        commands.insert(target.0, Migrated)
    });
    let mut bottom = rectangle(100.0)?;
    bottom.set_layer(Layer::new(-1));
    let tied_rectangle = rectangle(-0.0)?;
    let mut tied_image = ScreenImageVisual::new(
        image_id,
        LogicalScreenPosition::new(10.0, 10.0),
        LogicalScreenVector::new(80.0, 40.0),
    )?;
    tied_image.set_draw_order_depth(0.0)?;
    let tied_text = label(font.clone(), "Tie", 20.0)?;
    let mut later_text = label(font, "Later", 20.0)?;
    later_text.set_draw_order_depth(1.0)?;
    let top = rectangle(2.0)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("three-kinds", move |world| {
        world.spawn(camera)?;
        world.spawn(top)?;
        world.spawn(later_text.clone())?;
        let tied = world.spawn((tied_rectangle, tied_image, tied_text.clone()))?;
        world.insert_resource(TiedEntity(tied))?;
        world.spawn(bottom)?;
        Ok(())
    })?;
    let mut runner = app.build_headless(initial)?;
    let expected = [
        ScreenDraw::Rectangles { run: 0 },
        ScreenDraw::Image { index: 0 },
        ScreenDraw::Text { index: 0 },
        ScreenDraw::Text { index: 1 },
        ScreenDraw::Rectangles { run: 1 },
    ];
    for _ in 0..4 {
        let frame = snapshot(&runner)?;
        assert_eq!(frame.screen_draws(), expected);
        let first_run = frame.screen_rectangle_run_records(0).ok_or("first run")?;
        assert_eq!(first_run.len(), 2);
        assert_eq!(first_run[0].layer(), Layer::new(-1));
        assert_eq!(
            first_run[1].source(),
            frame.resolved_screen_images()[0].source()
        );
        assert_eq!(
            first_run[1].source(),
            frame.resolved_screen_texts()[0].source()
        );
        assert_eq!(frame.resolved_screen_texts()[0].text(), "Tie");
        assert_eq!(frame.resolved_screen_texts()[1].text(), "Later");
        assert_eq!(
            frame
                .screen_rectangle_run_records(1)
                .ok_or("last run")?
                .len(),
            1
        );
        assert!(frame.screen_rectangle_run_records(2).is_none());
        assert!(
            advance(&mut runner, Duration::ZERO, &[])?
                .failure()
                .is_none()
        );
    }
    Ok(())
}

#[test]
fn text_splits_rectangle_runs_even_when_no_images_exist() -> LogicResult {
    let mut app = application(limits(1, 128, 64))?;
    let font = font(&mut app)?;
    let mut visual = label(font, "Between", 20.0)?;
    visual.set_draw_order_depth(1.0)?;
    let before = rectangle(0.0)?;
    let after = rectangle(2.0)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("text-run-split", move |world| {
        world.spawn(camera)?;
        world.spawn(after)?;
        world.spawn(visual.clone())?;
        world.spawn(before)?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    let frame = snapshot(&runner)?;
    assert!(frame.resolved_screen_images().is_empty());
    assert_eq!(
        frame.screen_draws(),
        &[
            ScreenDraw::Rectangles { run: 0 },
            ScreenDraw::Text { index: 0 },
            ScreenDraw::Rectangles { run: 1 },
        ]
    );
    assert_eq!(
        frame.screen_rectangle_run_records(0).ok_or("before run")?[0].draw_order_depth(),
        0.0
    );
    assert_eq!(
        frame.screen_rectangle_run_records(1).ok_or("after run")?[0].draw_order_depth(),
        2.0
    );
    assert!(frame.screen_rectangle_run_records(2).is_none());
    Ok(())
}

#[test]
fn source_identity_precedes_visual_kind_except_on_exact_same_entity_ties() -> LogicResult {
    let mut app = application(limits(1, 128, 64))?;
    let font = font(&mut app)?;
    let image_id = app.register_image_rgba8(1, 1, &[255; 4])?;
    let image = ScreenImageVisual::new(
        image_id,
        LogicalScreenPosition::new(0.0, 0.0),
        LogicalScreenVector::new(10.0, 10.0),
    )?;
    let label = label(font, "First", 0.0)?;
    let rectangle = rectangle(0.0)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("source-before-kind", move |world| {
        world.spawn(camera)?;
        world.spawn(rectangle)?;
        world.spawn(image)?;
        world.spawn(label.clone())?;
        Ok(())
    })?;
    let runner = app.build_headless(initial)?;
    let frame = snapshot(&runner)?;
    assert_eq!(
        frame.screen_draws(),
        &[
            ScreenDraw::Text { index: 0 },
            ScreenDraw::Image { index: 0 },
            ScreenDraw::Rectangles { run: 0 },
        ]
    );
    Ok(())
}

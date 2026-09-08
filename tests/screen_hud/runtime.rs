use std::{error::Error, time::Duration};

use sim_engine::Layer;
use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

use super::support::*;

#[derive(Resource)]
struct VisualHandles {
    screen_only: LogicEntity,
    dual: LogicEntity,
}

#[test]
fn screen_geometry_is_independent_of_world_transforms_camera_and_partial_ticks()
-> Result<(), Box<dyn Error>> {
    for (zoom, rotation) in [(8.0, 0.0), (20.0, 0.75), (70.0, -0.4)] {
        let mut application = application(RenderLimits::default())?;
        application.approve_component::<Disabled>()?;
        application.add_fallible_fixed_system(
            |mut transforms: Query<&mut Transform2d>,
             mut cameras: Query<&mut ActiveCamera2d>|
             -> LogicResult {
                for mut transform in &mut transforms {
                    transform.translate_by(Vec2::new(2.0, 4.0))?;
                }
                for mut camera in &mut cameras {
                    camera.pan_by(Vec2::new(2.0, 4.0))?;
                }
                Ok(())
            },
        );
        let mut camera = Camera2d::new(Vec2::ZERO, zoom)?;
        camera.set_rotation(rotation)?;
        let screen = screen_rectangle(-20.0)?;
        let dual_screen = screen_rectangle(500.0)?;
        let transform = Transform2d::from_xy(4.0, 6.0)?;
        let circle = CircleVisual::new(1.0, Color::WHITE)?;
        let initial = application.register_world("independent-spaces", move |world| {
            world.spawn(ActiveCamera2d::new(camera))?;
            let screen_only = world.spawn(screen)?;
            let dual = world.spawn((dual_screen, transform, circle))?;
            world.spawn((Disabled, screen))?;
            world.insert_resource(VisualHandles { screen_only, dual })?;
            Ok(())
        })?;
        let mut runner = application.build_headless(initial)?;
        let handles = runner.resource::<VisualHandles>().ok_or("visual handles")?;
        let dual = handles.dual;
        assert!(matches!(
            runner.component::<Transform2d>(handles.screen_only),
            Err(QueryEntityError::DoesNotMatch { .. })
        ));
        assert_eq!(runner.components::<Transform2d>().count(), 1);
        let original_screen = snapshot(&runner)?.resolved_screen_rectangles().to_vec();
        assert_eq!(original_screen.len(), 2);

        let report = advance(&mut runner, Duration::from_millis(150), &[], 800.0)?;
        assert!(report.failure().is_none());
        assert_eq!(report.fixed_ticks_attempted(), 1);
        let extracted = snapshot(&runner)?;
        assert_eq!(extracted.resolved_screen_rectangles(), original_screen);
        assert_eq!(extracted.camera().center(), Vec2::new(1.0, 2.0));
        assert_eq!(extracted.camera().zoom(), zoom);
        assert_eq!(extracted.camera().rotation(), rotation);
        let [circle] = extracted.resolved_circles() else {
            return Err("only the dual-space entity has a world visual".into());
        };
        assert_eq!(circle.source(), dual);
        assert_eq!(circle.position(), Vec2::new(5.0, 8.0));
    }
    Ok(())
}

#[derive(Component)]
struct Migrated;

#[derive(Resource)]
struct TieEntity(LogicEntity);

#[test]
fn screen_layer_depth_and_identity_order_survives_archetype_migration() -> Result<(), Box<dyn Error>>
{
    let mut application = application(RenderLimits::default())?;
    application.approve_component::<Migrated>()?;
    application.add_fallible_frame_system(|tie: Res<TieEntity>, mut commands: Commands| {
        commands.insert(tie.0, Migrated)
    });
    let mut visuals = Vec::new();
    for (x, layer, depth) in [
        (50.0, 1, -100.0),
        (40.0, 0, 1.0),
        (10.0, -1, 100.0),
        (30.0, 0, 1.0),
        (20.0, 0, -1.0),
    ] {
        let mut visual = screen_rectangle(x)?;
        visual.set_layer(Layer::new(layer));
        visual.set_draw_order_depth(depth)?;
        visuals.push(visual);
    }
    let camera = ActiveCamera2d::centered(20.0)?;
    let mut world_circle = CircleVisual::new(1.0, Color::WHITE)?;
    world_circle.set_layer(Layer::new(1_000));
    let initial = application.register_world("screen-order", move |world| {
        world.spawn(camera)?;
        world.spawn(world_circle)?;
        for &visual in &visuals {
            let entity = world.spawn(visual)?;
            if visual.position().to_vec2().x() == 40.0 {
                world.insert_resource(TieEntity(entity))?;
            }
        }
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;
    let ordered = snapshot(&runner)?.resolved_screen_rectangles().to_vec();
    let positions: Vec<_> = ordered
        .iter()
        .map(|row| row.position().to_vec2().x())
        .collect();
    assert_eq!(positions[0..2], [10.0, 20.0]);
    assert!(positions[2..4].contains(&30.0));
    assert!(positions[2..4].contains(&40.0));
    assert_eq!(positions[4], 50.0);
    assert_eq!(ordered[0].layer(), Layer::new(-1));
    assert_eq!(ordered[1].draw_order_depth(), -1.0);
    assert_eq!(ordered[0].color(), Color::WHITE);
    assert_eq!(ordered[0].size(), LogicalScreenVector::new(100.0, 20.0));

    let report = advance(&mut runner, Duration::ZERO, &[], 800.0)?;
    assert!(report.failure().is_none());
    assert_eq!(snapshot(&runner)?.resolved_screen_rectangles(), ordered);
    assert_eq!(
        snapshot(&runner)?.resolved_circles()[0].layer(),
        Layer::new(1_000)
    );
    Ok(())
}

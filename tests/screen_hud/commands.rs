use std::error::Error;

use sim_logic::prelude::*;

use super::support::*;

#[derive(Component)]
struct Spawned;

#[derive(Component)]
struct Retained;

#[derive(Resource)]
struct Lifecycle {
    retained: LogicEntity,
    phase: usize,
    visual: ScreenRectangleVisual,
    stage_counts: Vec<usize>,
}

fn apply_next_command(
    mut lifecycle: ResMut<Lifecycle>,
    spawned: Query<LogicEntityRef, With<Spawned>>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    match lifecycle.phase {
        0 => {
            commands.spawn((Spawned, lifecycle.visual))?;
            commands.insert(lifecycle.retained, lifecycle.visual)?;
        }
        1 => commands.disable(lifecycle.retained)?,
        2 => commands.enable(lifecycle.retained)?,
        3 => commands.remove::<ScreenRectangleVisual>(lifecycle.retained)?,
        4 => commands.insert(lifecycle.retained, lifecycle.visual)?,
        5 => commands.despawn(lifecycle.retained)?,
        6 => {
            for entity in &spawned {
                commands.despawn(entity.handle())?;
            }
        }
        _ => {}
    }
    lifecycle.phase += 1;
    Ok(())
}

#[test]
fn commands_share_normal_stage_visibility_for_the_complete_screen_entity_lifecycle()
-> Result<(), Box<dyn Error>> {
    for stage in [Stage::FixedUpdate, Stage::FrameUpdate] {
        let mut application = application(RenderLimits::default())?;
        application.approve_components::<(Spawned, Retained)>()?;
        application.add_fallible_system(stage, apply_next_command);
        application.add_system(
            stage,
            |rectangles: Query<&ScreenRectangleVisual>, mut lifecycle: ResMut<Lifecycle>| {
                lifecycle.stage_counts.push(rectangles.iter().count());
            },
        );
        let camera = ActiveCamera2d::centered(20.0)?;
        let visual = screen_rectangle(24.0)?;
        let initial = application.register_world("screen-commands", move |world| {
            world.spawn(camera)?;
            let retained = world.spawn(Retained)?;
            world.insert_resource(Lifecycle {
                retained,
                phase: 0,
                visual,
                stage_counts: Vec::new(),
            })?;
            Ok(())
        })?;
        let mut runner = application.build_headless(initial)?;
        assert!(snapshot(&runner)?.resolved_screen_rectangles().is_empty());

        for expected in [2, 1, 2, 1, 2, 1, 0] {
            let report = advance(&mut runner, FIXED_STEP, &[], 800.0)?;
            assert!(report.failure().is_none());
            assert_eq!(
                snapshot(&runner)?.resolved_screen_rectangles().len(),
                expected
            );
        }
        assert_eq!(
            runner
                .resource::<Lifecycle>()
                .ok_or("command lifecycle")?
                .stage_counts,
            [0, 2, 1, 2, 1, 2, 1]
        );
        assert_eq!(runner.components::<Transform2d>().count(), 0);
        assert_eq!(runner.components::<ScreenRectangleVisual>().count(), 0);
    }
    Ok(())
}

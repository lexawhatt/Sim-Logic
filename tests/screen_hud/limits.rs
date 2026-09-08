use std::{error::Error, time::Duration};

use sim_engine::{SceneBudget, SceneBudgetResource, SceneError};
use sim_logic::prelude::*;

use super::support::*;

#[derive(Debug, Clone, Copy)]
enum LimitKind {
    Rectangles,
    Commands,
    Vertices,
}

fn limits(kind: LimitKind, rectangle_capacity: usize) -> RenderLimits {
    let defaults = RenderLimits::default();
    if matches!(kind, LimitKind::Rectangles) {
        return defaults.with_max_screen_rectangles(rectangle_capacity);
    }
    let scene = defaults.screen_scene_budget();
    defaults.with_screen_scene_budget(SceneBudget::new(
        if matches!(kind, LimitKind::Commands) {
            rectangle_capacity
        } else {
            scene.max_commands()
        },
        scene.max_points(),
        if matches!(kind, LimitKind::Vertices) {
            rectangle_capacity * 12
        } else {
            scene.max_tessellated_vertices()
        },
        scene.max_retained_bytes(),
        scene.max_allocation_bytes(),
        scene.max_upload_bytes(),
        scene.max_draw_batches(),
    ))
}

#[derive(Component)]
struct OverflowRectangle;

#[derive(Resource)]
struct OverflowState {
    phase: u8,
    visual: ScreenRectangleVisual,
}

fn overflow_then_recover(
    mut state: ResMut<OverflowState>,
    mut hud: Query<&mut ScreenRectangleVisual>,
    mut circles: Query<&mut CircleVisual>,
    overflow: Query<LogicEntityRef, With<OverflowRectangle>>,
    mut commands: Commands,
) -> LogicResult {
    match state.phase {
        0 => {
            for mut rectangle in &mut hud {
                rectangle.set_position(LogicalScreenPosition::new(50.0, 50.0))?;
            }
            for mut circle in &mut circles {
                circle.set_color(Color::BLACK)?;
            }
            commands.spawn((OverflowRectangle, state.visual))?;
        }
        1 => {
            for entity in &overflow {
                commands.despawn(entity.handle())?;
            }
        }
        _ => {}
    }
    state.phase += 1;
    Ok(())
}

#[test]
fn zero_exact_and_over_limit_screen_work_preserve_both_published_scenes_atomically()
-> Result<(), Box<dyn Error>> {
    for kind in [
        LimitKind::Rectangles,
        LimitKind::Commands,
        LimitKind::Vertices,
    ] {
        for capacity in [0, 1] {
            let mut application = application(limits(kind, capacity))?;
            application.approve_component::<OverflowRectangle>()?;
            application.add_fallible_frame_system(overflow_then_recover);
            let camera = ActiveCamera2d::centered(20.0)?;
            let circle = CircleVisual::new(1.0, Color::WHITE)?;
            let hud = screen_rectangle(24.0)?;
            let initial = application.register_world("bounded-hud", move |world| {
                world.spawn(camera)?;
                world.spawn(circle)?;
                for _ in 0..capacity {
                    world.spawn(hud)?;
                }
                world.insert_resource(OverflowState {
                    phase: 0,
                    visual: hud,
                })?;
                Ok(())
            })?;
            let mut runner = application.build_headless(initial)?;
            let old = snapshot(&runner)?;
            let old_generation = old.world_generation();
            let old_camera = old.camera();
            let old_circles = old.resolved_circles().to_vec();
            let old_screen = old.resolved_screen_rectangles().to_vec();
            assert_eq!(old_screen.len(), capacity);

            let failed = advance(&mut runner, Duration::ZERO, &[], 800.0)?;
            assert_eq!(failed.spawned(), 1);
            match (kind, failed.failure()) {
                (
                    LimitKind::Rectangles,
                    Some(FrameFailure::Extraction(ExtractionError::ScreenRectangleLimitExceeded {
                        limit,
                    })),
                ) => assert_eq!(*limit, capacity),
                (
                    LimitKind::Commands | LimitKind::Vertices,
                    Some(FrameFailure::Extraction(ExtractionError::ScreenScene(
                        SceneError::BudgetExceeded {
                            resource,
                            limit,
                            requested,
                        },
                    ))),
                ) => {
                    let (expected_resource, cost) = if matches!(kind, LimitKind::Commands) {
                        (SceneBudgetResource::Commands, 1)
                    } else {
                        (SceneBudgetResource::TessellatedVertices, 12)
                    };
                    assert_eq!(*resource, expected_resource);
                    assert_eq!(*limit, capacity * cost);
                    assert_eq!(*requested, (capacity + 1) * cost);
                }
                (_, failure) => panic!("unexpected {kind:?} limit failure: {failure:?}"),
            }
            assert_eq!(failed.extracted_generation(), None);
            assert_eq!(
                runner.components::<ScreenRectangleVisual>().count(),
                capacity + 1
            );
            let retained = snapshot(&runner)?;
            assert_eq!(retained.world_generation(), old_generation);
            assert_eq!(retained.camera(), old_camera);
            assert_eq!(retained.resolved_circles(), old_circles);
            assert_eq!(retained.resolved_screen_rectangles(), old_screen);

            let recovered = advance(&mut runner, Duration::ZERO, &[], 800.0)?;
            assert!(recovered.failure().is_none());
            assert_eq!(recovered.despawned(), 1);
            let current = snapshot(&runner)?;
            assert_eq!(current.resolved_screen_rectangles().len(), capacity);
            assert_eq!(current.resolved_circles()[0].color(), Color::BLACK);
            for rectangle in current.resolved_screen_rectangles() {
                assert_eq!(rectangle.position(), LogicalScreenPosition::new(50.0, 50.0));
            }
        }
    }
    Ok(())
}

#[test]
fn world_replacement_publishes_target_hud_or_retains_the_complete_source_snapshot()
-> Result<(), Box<dyn Error>> {
    for target_count in [1, 2] {
        let mut application = application(RenderLimits::default().with_max_screen_rectangles(1))?;
        application.bind_key(PhysicalKeyCode::Enter, TestAction::Replace)?;
        let camera = ActiveCamera2d::centered(20.0)?;
        let target_screen = screen_rectangle(300.0)?;
        let target_circle = CircleVisual::new(2.0, Color::BLACK)?;
        let target_transform = Transform2d::from_xy(8.0, 6.0)?;
        let target_background = WorldBackground::new(Color::WHITE)?;
        let target = application.register_world("target-hud", move |world| {
            world.insert_resource(target_background)?;
            world.spawn(camera)?;
            world.spawn((target_circle, target_transform))?;
            for _ in 0..target_count {
                world.spawn(target_screen)?;
            }
            Ok(())
        })?;
        application.add_fallible_frame_system(
            move |input: FrameInput<TestAction>, mut commands: Commands| {
                if let Some(edge) = input.pressed(TestAction::Replace).next() {
                    commands.replace_world(edge.intent(), target)?;
                }
                Ok::<_, CommandEnqueueError>(())
            },
        );
        let source_screen = screen_rectangle(24.0)?;
        let source_circle = CircleVisual::new(1.0, Color::WHITE)?;
        let initial = application.register_world("source-hud", move |world| {
            world.spawn(camera)?;
            world.spawn(source_circle)?;
            world.spawn(source_screen)?;
            Ok(())
        })?;
        let mut runner = application.build_headless(initial)?;
        let old = snapshot(&runner)?;
        let old_generation = old.world_generation();
        let old_background = old.background();
        let old_circles = old.resolved_circles().to_vec();
        let old_screen = old.resolved_screen_rectangles().to_vec();

        let report = advance(
            &mut runner,
            Duration::ZERO,
            &[InputEvent::key(
                PhysicalKeyCode::Enter,
                ButtonState::Pressed,
            )],
            800.0,
        )?;
        assert!(report.failure().is_none());
        let published = snapshot(&runner)?;
        if target_count == 1 {
            assert!(
                matches!(report.transition(), FrameTransition::Committed { target: committed, .. } if *committed == target)
            );
            assert_ne!(runner.world_generation(), old_generation);
            assert_eq!(
                report.extracted_generation(),
                Some(runner.world_generation())
            );
            assert_eq!(published.background(), Color::WHITE);
            assert_eq!(
                published.resolved_circles()[0].position(),
                Vec2::new(8.0, 6.0)
            );
            assert_eq!(
                published.resolved_screen_rectangles()[0].position(),
                LogicalScreenPosition::new(300.0, 24.0)
            );
        } else {
            assert!(matches!(
                report.transition(),
                FrameTransition::PreparationFailed {
                    target: rejected,
                    error: CandidateFailure::Extraction(ExtractionError::ScreenRectangleLimitExceeded { limit: 1 }),
                } if *rejected == target
            ));
            assert_eq!(runner.world_generation(), old_generation);
            assert_eq!(published.background(), old_background);
            assert_eq!(published.resolved_circles(), old_circles);
            assert_eq!(published.resolved_screen_rectangles(), old_screen);
        }
        for rectangle in published.resolved_screen_rectangles() {
            assert_eq!(
                rectangle.source().world_generation(),
                runner.world_generation()
            );
        }
        let continued = advance(&mut runner, Duration::ZERO, &[], 800.0)?;
        assert!(continued.failure().is_none());
        assert!(matches!(continued.transition(), FrameTransition::None));
        assert_eq!(snapshot(&runner)?.resolved_screen_rectangles().len(), 1);
    }
    Ok(())
}

use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, commands::CommandEnqueueError, prelude::*};

#[path = "../../examples/rectangle_room/game.rs"]
mod game;

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

#[derive(Clone, Copy, Resource)]
struct SpatialEntities {
    implicit: LogicEntity,
    explicit_before: LogicEntity,
    explicit_after: LogicEntity,
    disabled: LogicEntity,
    visual: LogicEntity,
}

#[test]
fn standard_rectangle_collider_is_auto_approved_and_independent_from_its_visual()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_component::<Disabled>()?;
    let collider = RectangleCollider2d::new(Vec2::new(2.0, 4.0))?;
    let first_position = Vec2::new(4.0, -2.0);
    let second_position = Vec2::new(-3.0, 5.0);
    let first_transform = Transform2d::new(first_position)?;
    let second_transform = Transform2d::new(second_position)?;
    let mut visual = RectangleVisual::new(Vec2::new(9.0, 7.0), Color::WHITE)?;
    visual.set_corner_radius(3.0)?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let initial = application.register_world("rectangle-collider-defaults", move |world| {
        world.spawn(camera)?;
        let implicit = world.spawn(collider)?;
        let explicit_before = world.spawn((first_transform, collider))?;
        let explicit_after = world.spawn((collider, second_transform))?;
        let disabled = world.spawn((Disabled, collider))?;
        let visual = world.spawn((visual, collider))?;
        world.insert_resource(SpatialEntities {
            implicit,
            explicit_before,
            explicit_after,
            disabled,
            visual,
        })?;
        Ok(())
    })?;

    let runner = application.build_headless(initial)?;
    let entities = *runner
        .resource::<SpatialEntities>()
        .ok_or("spatial entity handles should remain")?;
    assert_eq!(runner.components::<RectangleCollider2d>().count(), 5);
    let implicit_transform = runner.component::<Transform2d>(entities.implicit)?;
    assert_eq!(implicit_transform.translation(), Vec2::ZERO);
    assert_eq!(implicit_transform.previous_translation(), Vec2::ZERO);
    let explicit_before_transform = runner.component::<Transform2d>(entities.explicit_before)?;
    assert_eq!(explicit_before_transform.translation(), first_position);
    assert_eq!(
        explicit_before_transform.previous_translation(),
        first_position
    );
    let explicit_after_transform = runner.component::<Transform2d>(entities.explicit_after)?;
    assert_eq!(explicit_after_transform.translation(), second_position);
    assert_eq!(
        explicit_after_transform.previous_translation(),
        second_position
    );
    assert_eq!(
        runner
            .component::<Transform2d>(entities.disabled)?
            .translation(),
        Vec2::ZERO
    );
    assert_eq!(
        runner
            .component::<RectangleCollider2d>(entities.visual)?
            .size(),
        Vec2::new(2.0, 4.0)
    );
    assert_eq!(
        runner.component::<RectangleVisual>(entities.visual)?.size(),
        Vec2::new(9.0, 7.0)
    );
    let [resolved] = runner
        .extracted_frame()
        .ok_or("initial frame should be extracted")?
        .resolved_rectangles()
    else {
        return Err("only the independent visual should be extracted".into());
    };
    assert_eq!(resolved.source(), entities.visual);
    assert_eq!(resolved.corner_radius(), 3.0);
    Ok(())
}

#[derive(Component)]
struct BarrierWall;

#[derive(Component)]
struct UnrelatedCollider;

#[derive(Resource)]
struct BarrierScenario {
    phase: u8,
    original: LogicEntity,
    source: LogicEntity,
}

#[derive(Default, Resource)]
struct BarrierObservations(Vec<usize>);

fn change_walls(
    mut scenario: ResMut<BarrierScenario>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    match scenario.phase {
        0 => {
            let collider = RectangleCollider2d::new(Vec2::splat(1.0))?;
            commands.spawn((BarrierWall, collider))?;
            commands.spawn((
                BarrierWall,
                Transform2d::new(Vec2::new(0.25, 0.0))?,
                collider,
            ))?;
        }
        1 => commands.despawn(scenario.original)?,
        _ => return Ok(()),
    }
    scenario.phase += 1;
    Ok(())
}

fn observe_walls(
    scenario: Res<BarrierScenario>,
    sources: Query<(&Transform2d, &RectangleCollider2d), Without<BarrierWall>>,
    walls: RectangleOverlapEntities<With<BarrierWall>>,
    mut observations: ResMut<BarrierObservations>,
) -> Result<(), QueryEntityError> {
    let (source_transform, source_collider) = sources.get(scenario.source)?;
    observations.0.push(
        walls
            .iter_overlapping(scenario.source, source_transform, source_collider)?
            .count(),
    );
    Ok(())
}

#[test]
fn wall_queries_exclude_disabled_and_follow_the_structural_stage_barrier()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(BarrierWall, UnrelatedCollider, Disabled)>()?;
    application.add_fallible_frame_system(change_walls);
    application.add_fallible_frame_system(observe_walls);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = RectangleCollider2d::new(Vec2::splat(1.0))?;
    let initial = application.register_world("rectangle-collider-barrier", move |world| {
        world.spawn(camera)?;
        let original = world.spawn((BarrierWall, collider))?;
        world.spawn((BarrierWall, Disabled, collider))?;
        world.spawn(BarrierWall)?;
        world.spawn((UnrelatedCollider, collider))?;
        let source = world.spawn((Transform2d::default(), collider))?;
        world.insert_resource(BarrierScenario {
            phase: 0,
            original,
            source,
        })?;
        world.insert_resource(BarrierObservations::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    for _ in 0..3 {
        let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
        let FrameOutcome::Advanced(report) = outcome else {
            return Err("bounded barrier frame was rejected".into());
        };
        assert!(report.failure().is_none());
    }

    assert_eq!(
        runner
            .resource::<BarrierObservations>()
            .ok_or("barrier observations should remain")?
            .0,
        [1, 3, 2]
    );
    let mut collider_walls = 0;
    let mut origin_walls = 0;
    let mut explicit_command_transform = None;
    for (entity, _) in runner.components::<BarrierWall>() {
        if runner.component::<RectangleCollider2d>(entity).is_err() {
            continue;
        }
        collider_walls += 1;
        let transform = runner.component::<Transform2d>(entity)?;
        if transform.translation() == Vec2::ZERO {
            origin_walls += 1;
        } else if transform.translation() == Vec2::new(0.25, 0.0) {
            explicit_command_transform = Some(*transform);
        }
    }
    assert_eq!(
        collider_walls, 3,
        "two spawned walls and one disabled wall remain"
    );
    assert_eq!(origin_walls, 2, "implicit command spawn uses the origin");
    let explicit_command_transform =
        explicit_command_transform.ok_or("explicit command transform should remain")?;
    assert_eq!(
        explicit_command_transform.previous_translation(),
        explicit_command_transform.translation()
    );
    Ok(())
}

fn advance_game(
    runner: &mut HeadlessRunner<game::PlayerAction>,
    events: &[InputEvent],
) -> Result<(), Box<dyn Error>> {
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, events, viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded rectangle-room frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("rectangle-room frame failed: {failure}").into());
    }
    assert_eq!(report.fixed_ticks_attempted(), 1);
    Ok(())
}

#[test]
fn rectangle_room_commits_free_motion_and_rejects_a_wall_overlap_without_visual_drift()
-> Result<(), Box<dyn Error>> {
    let time = TimeConfig::new(FIXED_STEP, 1)?;
    let (application, initial) = game::build_application_with_time(time)?;
    let mut runner = application.build_headless(initial)?;
    let pressed = [InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed)];

    advance_game(&mut runner, &pressed)?;
    let player = runner
        .components::<game::Player>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("rectangle room should retain its player")?;
    assert_eq!(
        runner.component::<Transform2d>(player)?.translation(),
        Vec2::new(0.8, 0.0)
    );

    for _ in 0..7 {
        advance_game(&mut runner, &[])?;
    }
    let before_block = runner.component::<Transform2d>(player)?.translation();
    assert!((before_block.x() - 6.4).abs() < 0.000_01);
    advance_game(&mut runner, &[])?;

    let transform = runner.component::<Transform2d>(player)?;
    assert_eq!(transform.translation(), before_block);
    assert_eq!(transform.previous_translation(), before_block);
    let extracted = runner
        .extracted_frame()
        .ok_or("blocked frame should still extract")?;
    let body = extracted
        .resolved_circles()
        .iter()
        .find(|circle| circle.source() == player)
        .ok_or("player circle should remain visible")?;
    assert_eq!(body.position(), before_block);
    assert_eq!(runner.components::<game::Wall>().count(), 4);
    assert_eq!(runner.components::<RectangleCollider2d>().count(), 4);
    assert_eq!(runner.components::<CircleCollider2d>().count(), 1);
    Ok(())
}

#[derive(Component)]
struct OldWall;

#[derive(Component)]
struct NewWall;

#[derive(Resource)]
struct Route(WorldFactoryId);

#[derive(Default, Resource)]
struct NewLayoutObservation {
    invocations: u32,
    overlaps: usize,
}

#[derive(Resource)]
struct NewLayoutSource(LogicEntity);

fn replace_world(
    route: Option<Res<Route>>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    if let Some(route) = route {
        let intent = commands.new_transition_intent()?;
        commands.replace_world(intent, route.0)?;
    }
    Ok(())
}

fn observe_new_layout(
    source: Option<Res<NewLayoutSource>>,
    sources: Query<(&Transform2d, &CircleCollider2d), Without<NewWall>>,
    walls: RectangleOverlapEntities<With<NewWall>>,
    observation: Option<ResMut<NewLayoutObservation>>,
) -> Result<(), QueryEntityError> {
    let (Some(source), Some(mut observation)) = (source, observation) else {
        return Ok(());
    };
    let (source_transform, source_collider) = sources.get(source.0)?;
    observation.invocations += 1;
    observation.overlaps = walls
        .iter_overlapping_with_circle(source.0, source_transform, source_collider)?
        .count();
    Ok(())
}

#[test]
fn committed_replacement_exposes_only_the_new_world_collision_layout() -> Result<(), Box<dyn Error>>
{
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(OldWall, NewWall)>()?;
    application.add_fallible_frame_system(replace_world);
    application.add_fallible_frame_system(observe_new_layout);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let target_transform = Transform2d::new(Vec2::new(9.0, 0.0))?;
    let target_collider = RectangleCollider2d::new(Vec2::new(4.0, 2.0))?;
    let source_collider = CircleCollider2d::new(0.5)?;
    let target = application.register_world("new-collision-layout", move |world| {
        world.spawn(camera)?;
        world.spawn((NewWall, target_transform, target_collider))?;
        let source = world.spawn((target_transform, source_collider))?;
        world.insert_resource(NewLayoutSource(source))?;
        world.insert_resource(NewLayoutObservation::default())?;
        Ok(())
    })?;
    let old_collider = RectangleCollider2d::new(Vec2::splat(1.0))?;
    let initial = application.register_world("old-collision-layout", move |world| {
        world.spawn(camera)?;
        world.spawn((OldWall, old_collider))?;
        world.insert_resource(Route(target))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;
    let old_generation = runner.world_generation();
    let old = runner
        .components::<OldWall>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("old collision wall should exist")?;

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded replacement frame was rejected".into());
    };
    assert!(matches!(
        report.transition(),
        FrameTransition::Committed { .. }
    ));
    assert_ne!(runner.world_generation(), old_generation);
    assert!(runner.component::<RectangleCollider2d>(old).is_err());
    assert_eq!(runner.components::<OldWall>().count(), 0);
    let (new, _) = runner
        .components::<NewWall>()
        .next()
        .ok_or("new collision wall should exist")?;
    assert_eq!(
        runner.component::<Transform2d>(new)?.translation(),
        Vec2::new(9.0, 0.0)
    );
    assert_eq!(
        runner.component::<RectangleCollider2d>(new)?.size(),
        Vec2::new(4.0, 2.0)
    );

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded post-replacement frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert!(matches!(report.transition(), FrameTransition::None));
    let observation = runner
        .resource::<NewLayoutObservation>()
        .ok_or("new layout observation should remain")?;
    assert_eq!(observation.invocations, 1);
    assert_eq!(observation.overlaps, 1);
    Ok(())
}

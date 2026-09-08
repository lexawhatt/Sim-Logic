use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

const FIXED_STEP: Duration = Duration::from_millis(100);
const FIRST_POSITION: Vec2 = Vec2::new(4.0, -2.0);
const SECOND_POSITION: Vec2 = Vec2::new(-3.0, 5.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct Source;

#[derive(Component)]
struct Target;

#[derive(Clone, Copy, Resource)]
struct DirectEntities {
    camera: LogicEntity,
    circle_only: LogicEntity,
    rectangle_only: LogicEntity,
    source: LogicEntity,
    target: LogicEntity,
    disabled: LogicEntity,
    circle_explicit: LogicEntity,
    combined_explicit: LogicEntity,
    combined_implicit: LogicEntity,
}

#[derive(Resource, Default)]
struct OverlapObservation(Vec<LogicEntity>);

fn observe_origin_overlap(
    source: Query<(LogicEntityRef, &Transform2d, &CircleCollider2d), With<Source>>,
    targets: CircleOverlapEntities<With<Target>>,
    mut observation: ResMut<OverlapObservation>,
) -> Result<(), Box<dyn Error>> {
    let (source, transform, collider) = source.single()?;
    observation.0 = targets
        .iter_overlapping(source.handle(), transform, collider)?
        .collect();
    Ok(())
}

fn viewport() -> Result<LogicalViewport, sim_engine::LogicalViewportError> {
    LogicalViewport::new(800.0, 600.0)
}

#[test]
fn standard_spatial_components_supply_one_transform_and_preserve_explicit_translations()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_components::<(Source, Target, Disabled)>()?;
    application.add_fallible_system(Stage::FixedUpdate, observe_origin_overlap);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let circle = CircleVisual::new(0.5, Color::WHITE)?;
    let rectangle = RectangleVisual::new(Vec2::new(1.0, 2.0), Color::WHITE)?;
    let collider = CircleCollider2d::new(0.75)?;
    let first_transform = Transform2d::new(FIRST_POSITION)?;
    let second_transform = Transform2d::new(SECOND_POSITION)?;
    let initial = application.register_world("spatial-defaults", move |world| {
        let camera = world.spawn(camera)?;
        let circle_only = world.spawn(circle)?;
        let rectangle_only = world.spawn(rectangle)?;
        let source = world.spawn((Source, collider))?;
        let target = world.spawn((Target, collider))?;
        let disabled = world.spawn((Target, Disabled, circle, rectangle, collider))?;
        let circle_explicit = world.spawn((circle, first_transform))?;
        let combined_explicit = world.spawn((second_transform, rectangle, collider))?;
        let combined_implicit = world.spawn((circle.with_matching_collider(), rectangle))?;
        world.insert_resource(DirectEntities {
            camera,
            circle_only,
            rectangle_only,
            source,
            target,
            disabled,
            circle_explicit,
            combined_explicit,
            combined_implicit,
        })?;
        world.insert_resource(OverlapObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let entities = *runner
        .resource::<DirectEntities>()
        .ok_or("direct entity handles should remain")?;
    assert_eq!(runner.components::<Transform2d>().count(), 8);
    assert!(runner.component::<Transform2d>(entities.camera).is_err());
    for entity in [
        entities.circle_only,
        entities.rectangle_only,
        entities.source,
        entities.target,
        entities.disabled,
        entities.combined_implicit,
    ] {
        let transform = runner.component::<Transform2d>(entity)?;
        assert_eq!(transform.previous_translation(), Vec2::ZERO);
        assert_eq!(transform.translation(), Vec2::ZERO);
    }
    assert_eq!(
        runner
            .component::<Transform2d>(entities.circle_explicit)?
            .translation(),
        FIRST_POSITION
    );
    assert_eq!(
        runner
            .component::<Transform2d>(entities.combined_explicit)?
            .translation(),
        SECOND_POSITION
    );

    let extracted = runner
        .extracted_frame()
        .ok_or("initial spatial frame should be published")?;
    assert_eq!(extracted.resolved_circles().len(), 3);
    assert_eq!(extracted.resolved_rectangles().len(), 3);
    assert!(
        extracted.resolved_circles().iter().any(
            |visual| visual.source() == entities.circle_only && visual.position() == Vec2::ZERO
        )
    );
    assert!(
        extracted
            .resolved_rectangles()
            .iter()
            .any(|visual| visual.source() == entities.rectangle_only
                && visual.position() == Vec2::ZERO)
    );
    assert!(
        extracted
            .resolved_circles()
            .iter()
            .all(|visual| visual.source() != entities.disabled)
    );
    assert!(
        extracted
            .resolved_rectangles()
            .iter()
            .all(|visual| visual.source() != entities.disabled)
    );

    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded overlap frame was rejected".into());
    };
    assert!(report.failure().is_none());
    let observation = runner
        .resource::<OverlapObservation>()
        .ok_or("overlap observation should remain")?;
    assert_eq!(observation.0, [entities.target]);
    Ok(())
}

#[derive(Component)]
struct ImplicitCommandSpawn;

#[derive(Component)]
struct ExplicitCommandSpawn;

#[derive(Resource, Default)]
struct QueueOnce(bool);

#[derive(Resource, Default)]
struct BarrierObservation(usize);

fn queue_spatial_entities(
    mut once: ResMut<QueueOnce>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    if once.0 {
        return Ok(());
    }
    let circle = CircleVisual::new(0.5, Color::WHITE)?;
    commands.spawn((ImplicitCommandSpawn, circle.with_matching_collider()))?;

    let rectangle = RectangleVisual::new(Vec2::ONE, Color::WHITE)?;
    let mut transform = Transform2d::default();
    transform.set_translation(FIRST_POSITION)?;
    commands.spawn((ExplicitCommandSpawn, rectangle, transform))?;
    once.0 = true;
    Ok(())
}

fn observe_before_barrier(
    implicit: Query<LogicEntityRef, With<ImplicitCommandSpawn>>,
    explicit: Query<LogicEntityRef, With<ExplicitCommandSpawn>>,
    mut observation: ResMut<BarrierObservation>,
) {
    observation.0 = implicit.iter().count() + explicit.iter().count();
}

#[test]
fn deferred_spatial_requirements_materialize_and_snap_at_the_stage_barrier()
-> Result<(), Box<dyn Error>> {
    let mut application = Application::<TestAction>::new(AppConfig::default())?;
    application.approve_components::<(ImplicitCommandSpawn, ExplicitCommandSpawn)>()?;
    application.add_fallible_system(Stage::FrameUpdate, queue_spatial_entities);
    application.add_system(Stage::FrameUpdate, observe_before_barrier);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let initial = application.register_world("spatial-command", move |world| {
        world.spawn(camera)?;
        world.insert_resource(QueueOnce::default())?;
        world.insert_resource(BarrierObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport()?));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded command frame was rejected".into());
    };
    assert!(report.failure().is_none());
    assert_eq!(report.spawned(), 2);
    assert_eq!(
        runner
            .resource::<BarrierObservation>()
            .map(|observation| observation.0),
        Some(0)
    );

    let implicit = runner
        .components::<ImplicitCommandSpawn>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("implicit command entity should exist after the barrier")?;
    let explicit = runner
        .components::<ExplicitCommandSpawn>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("explicit command entity should exist after the barrier")?;
    let implicit_transform = runner.component::<Transform2d>(implicit)?;
    assert_eq!(implicit_transform.previous_translation(), Vec2::ZERO);
    assert_eq!(implicit_transform.translation(), Vec2::ZERO);
    let explicit_transform = runner.component::<Transform2d>(explicit)?;
    assert_eq!(explicit_transform.previous_translation(), FIRST_POSITION);
    assert_eq!(explicit_transform.translation(), FIRST_POSITION);

    let extracted = runner
        .extracted_frame()
        .ok_or("command frame should publish both spatial entities")?;
    assert!(
        extracted
            .resolved_circles()
            .iter()
            .any(|visual| visual.source() == implicit && visual.position() == Vec2::ZERO)
    );
    assert!(
        extracted
            .resolved_rectangles()
            .iter()
            .any(|visual| visual.source() == explicit && visual.position() == FIRST_POSITION)
    );
    Ok(())
}

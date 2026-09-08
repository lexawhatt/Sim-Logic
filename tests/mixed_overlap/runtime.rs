use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct Source;

#[derive(Component)]
struct RectangleCandidate;

#[derive(Component)]
struct CircleCandidate;

#[derive(Resource)]
struct KnownEntities {
    source: LogicEntity,
    dual: LogicEntity,
    rectangle_tangent: LogicEntity,
    rectangle_moved: LogicEntity,
    rectangle_outside: LogicEntity,
    rectangle_unrelated: LogicEntity,
    rectangle_disabled: LogicEntity,
    circle_tangent: LogicEntity,
    circle_moved: LogicEntity,
    circle_outside: LogicEntity,
    circle_unrelated: LogicEntity,
    circle_disabled: LogicEntity,
}

fn move_mixed_candidates(
    known: Res<KnownEntities>,
    mut transforms: Query<&mut Transform2d>,
) -> Result<(), Box<dyn Error>> {
    transforms
        .get_mut(known.rectangle_moved)?
        .set_translation(Vec2::new(2.0, 0.0))?;
    transforms
        .get_mut(known.circle_moved)?
        .set_translation(Vec2::new(2.0, 0.0))?;
    Ok(())
}

#[derive(Resource, Default)]
struct MixedObservation {
    rectangles: Vec<LogicEntity>,
    circles: Vec<LogicEntity>,
    has_rectangle: bool,
    has_circle: bool,
    single_rectangle_count: usize,
    single_circle_count: usize,
    has_single_rectangle: bool,
    has_single_circle: bool,
    empty_rectangle_count: usize,
    empty_circle_count: usize,
    has_empty_rectangle: bool,
    has_empty_circle: bool,
}

fn observe_mixed_candidates(
    source: Single<
        (
            LogicEntityRef,
            &Transform2d,
            &CircleCollider2d,
            &RectangleCollider2d,
        ),
        With<Source>,
    >,
    rectangles: RectangleOverlapEntities<With<RectangleCandidate>>,
    circles: CircleOverlapEntities<With<CircleCandidate>>,
    mut observation: ResMut<MixedObservation>,
) -> LogicResult {
    let (source, transform, circle, rectangle) = source.into_inner();
    observation.has_rectangle =
        rectangles.has_overlap_with_circle(source.handle(), transform, circle)?;
    observation.rectangles = rectangles
        .iter_overlapping_with_circle(source.handle(), transform, circle)?
        .collect();
    observation.has_circle =
        circles.has_overlap_with_rectangle(source.handle(), transform, rectangle)?;
    observation.circles = circles
        .iter_overlapping_with_rectangle(source.handle(), transform, rectangle)?
        .collect();
    let single_transform = Transform2d::from_xy(-2.0, 0.0)?;
    observation.has_single_rectangle =
        rectangles.has_overlap_with_circle(source.handle(), &single_transform, circle)?;
    observation.single_rectangle_count = rectangles
        .iter_overlapping_with_circle(source.handle(), &single_transform, circle)?
        .count();
    observation.has_single_circle =
        circles.has_overlap_with_rectangle(source.handle(), &single_transform, rectangle)?;
    observation.single_circle_count = circles
        .iter_overlapping_with_rectangle(source.handle(), &single_transform, rectangle)?
        .count();
    let empty_transform = Transform2d::from_xy(-10.0, 0.0)?;
    observation.has_empty_rectangle =
        rectangles.has_overlap_with_circle(source.handle(), &empty_transform, circle)?;
    observation.empty_rectangle_count = rectangles
        .iter_overlapping_with_circle(source.handle(), &empty_transform, circle)?
        .count();
    observation.has_empty_circle =
        circles.has_overlap_with_rectangle(source.handle(), &empty_transform, rectangle)?;
    observation.empty_circle_count = circles
        .iter_overlapping_with_rectangle(source.handle(), &empty_transform, rectangle)?
        .count();
    Ok(())
}

fn advance(runner: &mut HeadlessRunner<TestAction>) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded mixed-overlap frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("mixed-overlap frame failed: {failure}").into());
    }
    Ok(report)
}

#[test]
fn both_query_orientations_filter_candidates_and_exclude_self_and_disabled()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_components::<(Source, RectangleCandidate, CircleCandidate, Disabled)>()?;
    application.add_fallible_fixed_system(move_mixed_candidates);
    application.add_fallible_fixed_system(observe_mixed_candidates);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let circle = CircleCollider2d::new(1.0)?;
    let rectangle = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let tangent = Transform2d::from_xy(2.0, 0.0)?;
    let initially_distant = Transform2d::from_xy(20.0, 0.0)?;
    let outside = Transform2d::from_xy(2.01, 0.0)?;
    let initial = application.register_world("mixed-query", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((
            Source,
            RectangleCandidate,
            CircleCandidate,
            circle,
            rectangle,
        ))?;
        let dual = world.spawn((RectangleCandidate, CircleCandidate, circle, rectangle))?;
        let rectangle_tangent = world.spawn((RectangleCandidate, tangent, rectangle))?;
        let rectangle_moved = world.spawn((RectangleCandidate, initially_distant, rectangle))?;
        let rectangle_outside = world.spawn((RectangleCandidate, outside, rectangle))?;
        let rectangle_unrelated = world.spawn(rectangle)?;
        let rectangle_disabled = world.spawn((RectangleCandidate, Disabled, rectangle))?;
        let circle_tangent = world.spawn((CircleCandidate, tangent, circle))?;
        let circle_moved = world.spawn((CircleCandidate, initially_distant, circle))?;
        let circle_outside = world.spawn((CircleCandidate, outside, circle))?;
        let circle_unrelated = world.spawn(circle)?;
        let circle_disabled = world.spawn((CircleCandidate, Disabled, circle))?;
        world.insert_resource(KnownEntities {
            source,
            dual,
            rectangle_tangent,
            rectangle_moved,
            rectangle_outside,
            rectangle_unrelated,
            rectangle_disabled,
            circle_tangent,
            circle_moved,
            circle_outside,
            circle_unrelated,
            circle_disabled,
        })?;
        world.insert_resource(MixedObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    advance(&mut runner)?;
    let known = runner
        .resource::<KnownEntities>()
        .ok_or("known mixed entities should remain")?;
    let observation = runner
        .resource::<MixedObservation>()
        .ok_or("mixed observation should remain")?;
    assert_eq!(observation.rectangles.len(), 3);
    assert_eq!(observation.circles.len(), 3);
    assert_eq!(
        observation.has_rectangle,
        !observation.rectangles.is_empty()
    );
    assert_eq!(observation.has_circle, !observation.circles.is_empty());
    assert!(observation.has_single_rectangle);
    assert_eq!(observation.single_rectangle_count, 1);
    assert!(observation.has_single_circle);
    assert_eq!(observation.single_circle_count, 1);
    assert!(!observation.has_empty_rectangle);
    assert_eq!(observation.empty_rectangle_count, 0);
    assert!(!observation.has_empty_circle);
    assert_eq!(observation.empty_circle_count, 0);
    assert!(observation.rectangles.contains(&known.dual));
    assert!(observation.rectangles.contains(&known.rectangle_tangent));
    assert!(observation.rectangles.contains(&known.rectangle_moved));
    assert!(observation.circles.contains(&known.dual));
    assert!(observation.circles.contains(&known.circle_tangent));
    assert!(observation.circles.contains(&known.circle_moved));
    assert!(!observation.rectangles.contains(&known.source));
    assert!(!observation.rectangles.contains(&known.rectangle_outside));
    assert!(!observation.rectangles.contains(&known.rectangle_unrelated));
    assert!(!observation.rectangles.contains(&known.rectangle_disabled));
    assert!(!observation.circles.contains(&known.source));
    assert!(!observation.circles.contains(&known.circle_outside));
    assert!(!observation.circles.contains(&known.circle_unrelated));
    assert!(!observation.circles.contains(&known.circle_disabled));
    Ok(())
}

#[derive(Resource)]
struct InvalidSources {
    geometry: LogicEntity,
    foreign: LogicEntity,
    missing: LogicEntity,
    disabled: LogicEntity,
}

#[derive(Resource, Default)]
struct InvalidSourceObservation {
    foreign: [Option<QueryEntityError>; 4],
    missing: [Option<QueryEntityError>; 4],
    disabled: [Option<QueryEntityError>; 4],
}

fn record_invalid_sources(
    sources: Res<InvalidSources>,
    geometry: Query<(&Transform2d, &CircleCollider2d, &RectangleCollider2d)>,
    rectangles: RectangleOverlapEntities,
    circles: CircleOverlapEntities,
    mut observation: ResMut<InvalidSourceObservation>,
) -> Result<(), QueryEntityError> {
    let (transform, circle, rectangle) = geometry.get(sources.geometry)?;
    let collect_errors = |source| {
        [
            rectangles
                .iter_overlapping_with_circle(source, transform, circle)
                .err(),
            rectangles
                .has_overlap_with_circle(source, transform, circle)
                .err(),
            circles
                .iter_overlapping_with_rectangle(source, transform, rectangle)
                .err(),
            circles
                .has_overlap_with_rectangle(source, transform, rectangle)
                .err(),
        ]
    };
    observation.foreign = collect_errors(sources.foreign);
    observation.missing = collect_errors(sources.missing);
    observation.disabled = collect_errors(sources.disabled);
    Ok(())
}

#[test]
fn both_mixed_queries_reject_foreign_missing_and_disabled_source_handles()
-> Result<(), Box<dyn Error>> {
    let mut foreign_application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let foreign_circle = CircleCollider2d::new(1.0)?;
    let foreign_world =
        foreign_application.register_world("foreign-mixed-source", move |world| {
            world.spawn(camera)?;
            world.spawn(foreign_circle)?;
            Ok(())
        })?;
    let foreign_runner = foreign_application.build_headless(foreign_world)?;
    let foreign = foreign_runner
        .components::<CircleCollider2d>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("foreign source should exist")?;

    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Disabled>()?;
    application.add_fallible_fixed_system(record_invalid_sources);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let circle = CircleCollider2d::new(1.0)?;
    let rectangle = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let initial = application.register_world("invalid-mixed-sources", move |world| {
        world.spawn(camera)?;
        let geometry = world.spawn((circle, rectangle))?;
        let disabled = world.spawn((Disabled, circle, rectangle))?;
        let missing = world.spawn((circle, rectangle))?;
        world.despawn(missing)?;
        world.insert_resource(InvalidSources {
            geometry,
            foreign,
            missing,
            disabled,
        })?;
        world.insert_resource(InvalidSourceObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    advance(&mut runner)?;
    let observation = runner
        .resource::<InvalidSourceObservation>()
        .ok_or("invalid mixed-source observation should remain")?;
    assert!(
        observation
            .foreign
            .iter()
            .all(|error| error == &observation.foreign[0])
    );
    assert!(
        observation
            .missing
            .iter()
            .all(|error| error == &observation.missing[0])
    );
    assert!(
        observation
            .disabled
            .iter()
            .all(|error| error == &observation.disabled[0])
    );
    assert!(matches!(
        observation.foreign[0],
        Some(QueryEntityError::ForeignWorld { entity, .. }) if entity == foreign
    ));
    assert!(matches!(
        observation.missing[0],
        Some(QueryEntityError::DoesNotMatch { .. })
    ));
    assert!(matches!(
        observation.disabled[0],
        Some(QueryEntityError::DoesNotMatch { .. })
    ));
    Ok(())
}

#[derive(Resource, Default)]
struct ProposedObservation {
    stored_position: Vec2,
    proposed_overlap: bool,
    proposed_count: usize,
}

type MutableCircleSource<'world, 'state> = Single<
    'world,
    'state,
    (
        LogicEntityRef,
        &'static mut Transform2d,
        &'static CircleCollider2d,
    ),
    (With<Source>, Without<RectangleCandidate>),
>;

fn test_proposed_circle_position(
    source: MutableCircleSource,
    rectangles: RectangleOverlapEntities<(With<RectangleCandidate>, Without<Source>)>,
    mut observation: ResMut<ProposedObservation>,
) -> Result<(), Box<dyn Error>> {
    let (source, transform, collider) = source.into_inner();
    let proposed = transform.translated_by(Vec2::new(-10.0, 0.0))?;
    observation.stored_position = transform.translation();
    observation.proposed_overlap =
        rectangles.has_overlap_with_circle(source.handle(), &proposed, collider)?;
    observation.proposed_count = rectangles
        .iter_overlapping_with_circle(source.handle(), &proposed, collider)?
        .count();
    Ok(())
}

#[test]
fn mutable_source_can_test_a_proposed_transform_against_disjoint_rectangles()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_components::<(Source, RectangleCandidate)>()?;
    application.add_fallible_fixed_system(test_proposed_circle_position);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let circle = CircleCollider2d::new(1.0)?;
    let rectangle = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let source_transform = Transform2d::from_xy(10.0, 0.0)?;
    let initial = application.register_world("mixed-proposed-transform", move |world| {
        world.spawn(camera)?;
        world.spawn((Source, source_transform, circle))?;
        world.spawn((RectangleCandidate, rectangle))?;
        world.insert_resource(ProposedObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    advance(&mut runner)?;
    let observation = runner
        .resource::<ProposedObservation>()
        .ok_or("proposed-transform observation should remain")?;
    assert_eq!(observation.stored_position, Vec2::new(10.0, 0.0));
    assert!(observation.proposed_overlap);
    assert_eq!(observation.proposed_count, 1);
    Ok(())
}

#[derive(Resource)]
struct BarrierEntities {
    source: LogicEntity,
    rectangle: LogicEntity,
    replaced: bool,
}

#[derive(Resource, Clone, Copy)]
struct RectangleTemplate(RectangleCollider2d);

#[derive(Resource, Default)]
struct BarrierObservation(Vec<usize>);

fn replace_rectangle_candidate(
    mut entities: ResMut<BarrierEntities>,
    template: Res<RectangleTemplate>,
    mut commands: Commands,
) -> LogicResult {
    if entities.replaced {
        return Ok(());
    }
    commands.despawn(entities.rectangle)?;
    commands.spawn((RectangleCandidate, template.0))?;
    entities.replaced = true;
    Ok(())
}

fn observe_before_barrier(
    entities: Res<BarrierEntities>,
    sources: Query<(&Transform2d, &CircleCollider2d)>,
    rectangles: RectangleOverlapEntities<With<RectangleCandidate>>,
    mut observation: ResMut<BarrierObservation>,
) -> Result<(), QueryEntityError> {
    let (transform, collider) = sources.get(entities.source)?;
    observation.0.push(
        rectangles
            .iter_overlapping_with_circle(entities.source, transform, collider)?
            .count(),
    );
    Ok(())
}

#[test]
fn queued_mixed_candidate_changes_cross_only_the_stage_barrier() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<RectangleCandidate>()?;
    application.add_fallible_fixed_system(replace_rectangle_candidate);
    application.add_fallible_fixed_system(observe_before_barrier);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let circle = CircleCollider2d::new(1.0)?;
    let rectangle = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let initial = application.register_world("mixed-command-barrier", move |world| {
        world.spawn(camera)?;
        let source = world.spawn(circle)?;
        let rectangle_entity = world.spawn((RectangleCandidate, rectangle))?;
        world.insert_resource(BarrierEntities {
            source,
            rectangle: rectangle_entity,
            replaced: false,
        })?;
        world.insert_resource(RectangleTemplate(rectangle))?;
        world.insert_resource(BarrierObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let report = advance(&mut runner)?;
    assert_eq!(report.despawned(), 1);
    assert_eq!(report.spawned(), 1);
    assert_eq!(
        runner
            .resource::<BarrierObservation>()
            .map(|value| value.0.as_slice()),
        Some([1].as_slice())
    );
    let old_rectangle = runner
        .resource::<BarrierEntities>()
        .ok_or("barrier handles should remain")?
        .rectangle;
    assert!(matches!(
        runner.component::<RectangleCollider2d>(old_rectangle),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(runner.components::<RectangleCandidate>().count(), 1);

    let report = advance(&mut runner)?;
    assert_eq!(report.despawned(), 0);
    assert_eq!(report.spawned(), 0);
    assert_eq!(
        runner
            .resource::<BarrierObservation>()
            .map(|value| value.0.as_slice()),
        Some([1, 1].as_slice()),
        "the next stage invocation must discover the spawned rectangle"
    );
    Ok(())
}

use std::{error::Error, time::Duration};

use sim_logic::{bevy_ecs::entity_disabling::Disabled, prelude::*};

const FIXED_STEP: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct Source;

#[derive(Component)]
struct Candidate;

#[derive(Component)]
struct Unrelated;

#[derive(Resource)]
struct KnownEntities {
    source: LogicEntity,
    edge_tangent: LogicEntity,
    corner_tangent: LogicEntity,
    moved: LogicEntity,
    outside: LogicEntity,
    unrelated: LogicEntity,
    disabled: LogicEntity,
}

#[derive(Resource, Default)]
struct Observation {
    hits: Vec<LogicEntity>,
    has_overlap: bool,
    has_self_overlap: bool,
    has_disabled_overlap: bool,
}

fn move_candidate(
    known: Res<KnownEntities>,
    mut transforms: Query<&mut Transform2d, With<Candidate>>,
) -> Result<(), QueryEntityError> {
    transforms
        .get_mut(known.moved)?
        .set_translation(Vec2::new(0.5, 0.5))
        .expect("test translation should be valid");
    Ok(())
}

fn observe_filtered_overlaps(
    known: Res<KnownEntities>,
    sources: Query<(&Transform2d, &RectangleCollider2d)>,
    overlaps: RectangleOverlapEntities<With<Candidate>>,
    self_only: RectangleOverlapEntities<With<Source>>,
    disabled_only: RectangleOverlapEntities<With<Disabled>>,
    mut observation: ResMut<Observation>,
) -> Result<(), QueryEntityError> {
    let (transform, collider) = sources.get(known.source)?;
    observation.has_overlap = overlaps.has_overlap(known.source, transform, collider)?;
    observation.has_self_overlap = self_only.has_overlap(known.source, transform, collider)?;
    observation.has_disabled_overlap =
        disabled_only.has_overlap(known.source, transform, collider)?;
    observation.hits = overlaps
        .iter_overlapping(known.source, transform, collider)?
        .collect();
    Ok(())
}

fn advance(runner: &mut HeadlessRunner<TestAction>) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(800.0, 600.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err("bounded rectangle-overlap frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("rectangle-overlap frame failed: {failure}").into());
    }
    Ok(report)
}

#[test]
fn query_observes_live_state_and_filters_self_unrelated_and_disabled_entities()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_components::<(Source, Candidate, Unrelated, Disabled)>()?;
    application.add_fallible_system(Stage::FixedUpdate, move_candidate);
    application.add_fallible_system(Stage::FixedUpdate, observe_filtered_overlaps);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let edge_tangent = Transform2d::new(Vec2::new(2.0, 0.0))?;
    let corner_tangent = Transform2d::new(Vec2::new(2.0, 2.0))?;
    let moved_transform = Transform2d::new(Vec2::new(10.0, 10.0))?;
    let outside_transform = Transform2d::new(Vec2::new(2.001, 0.0))?;
    let initial = application.register_world("rectangle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((Source, Candidate, Transform2d::default(), collider))?;
        let edge_tangent = world.spawn((Candidate, edge_tangent, collider))?;
        let corner_tangent = world.spawn((Candidate, corner_tangent, collider))?;
        let moved = world.spawn((Candidate, moved_transform, collider))?;
        let outside = world.spawn((Candidate, outside_transform, collider))?;
        let unrelated = world.spawn((Unrelated, Transform2d::default(), collider))?;
        let disabled = world.spawn((Candidate, Disabled, Transform2d::default(), collider))?;
        world.insert_resource(KnownEntities {
            source,
            edge_tangent,
            corner_tangent,
            moved,
            outside,
            unrelated,
            disabled,
        })?;
        world.insert_resource(Observation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let report = advance(&mut runner)?;
    assert_eq!(report.fixed_ticks_attempted(), 1);
    let known = runner
        .resource::<KnownEntities>()
        .ok_or("known entity resource should remain")?;
    let observation = runner
        .resource::<Observation>()
        .ok_or("overlap observation should remain")?;
    let hits = &observation.hits;
    assert_eq!(hits.len(), 3);
    assert_eq!(observation.has_overlap, !hits.is_empty());
    assert!(!observation.has_self_overlap);
    assert!(!observation.has_disabled_overlap);
    assert!(hits.contains(&known.edge_tangent));
    assert!(hits.contains(&known.corner_tangent));
    assert!(hits.contains(&known.moved));
    assert!(!hits.contains(&known.source));
    assert!(!hits.contains(&known.outside));
    assert!(!hits.contains(&known.unrelated));
    assert!(!hits.contains(&known.disabled));
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
    foreign: [Option<QueryEntityError>; 2],
    missing: [Option<QueryEntityError>; 2],
    disabled: [Option<QueryEntityError>; 2],
}

fn record_invalid_sources(
    sources: Res<InvalidSources>,
    geometry: Query<(&Transform2d, &RectangleCollider2d)>,
    overlaps: RectangleOverlapEntities,
    mut observation: ResMut<InvalidSourceObservation>,
) -> Result<(), QueryEntityError> {
    let (transform, collider) = geometry.get(sources.geometry)?;
    observation.foreign = [
        overlaps
            .iter_overlapping(sources.foreign, transform, collider)
            .err(),
        overlaps
            .has_overlap(sources.foreign, transform, collider)
            .err(),
    ];
    observation.missing = [
        overlaps
            .iter_overlapping(sources.missing, transform, collider)
            .err(),
        overlaps
            .has_overlap(sources.missing, transform, collider)
            .err(),
    ];
    observation.disabled = [
        overlaps
            .iter_overlapping(sources.disabled, transform, collider)
            .err(),
        overlaps
            .has_overlap(sources.disabled, transform, collider)
            .err(),
    ];
    Ok(())
}

#[test]
fn query_rejects_foreign_missing_and_disabled_source_handles() -> Result<(), Box<dyn Error>> {
    let mut foreign_application = Application::<TestAction>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let foreign_world = foreign_application.register_world("foreign", move |world| {
        world.spawn(camera)?;
        world.spawn((Transform2d::default(), collider))?;
        Ok(())
    })?;
    let foreign_runner = foreign_application.build_headless(foreign_world)?;
    let foreign = foreign_runner
        .components::<RectangleCollider2d>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or("foreign collider should exist")?;

    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Disabled>()?;
    application.add_fallible_system(Stage::FixedUpdate, record_invalid_sources);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let initial = application.register_world("invalid-sources", move |world| {
        world.spawn(camera)?;
        let geometry = world.spawn((Transform2d::default(), collider))?;
        let disabled = world.spawn((Disabled, Transform2d::default(), collider))?;
        let missing = world.spawn((Transform2d::default(), collider))?;
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
        .ok_or("invalid-source observation should remain")?;
    assert_eq!(observation.foreign[0], observation.foreign[1]);
    assert_eq!(observation.missing[0], observation.missing[1]);
    assert_eq!(observation.disabled[0], observation.disabled[1]);
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

#[derive(Resource)]
struct BarrierEntities {
    source: LogicEntity,
    removed: LogicEntity,
    inserted: LogicEntity,
    queued: bool,
}

#[derive(Resource, Default)]
struct BarrierObservation(Vec<Vec<LogicEntity>>);

fn queue_component_changes(
    mut entities: ResMut<BarrierEntities>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    if !entities.queued {
        let collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
        commands.remove::<RectangleCollider2d>(entities.removed)?;
        commands.insert(entities.inserted, collider)?;
        entities.queued = true;
    }
    Ok(())
}

fn observe_component_changes(
    entities: Res<BarrierEntities>,
    sources: Query<(&Transform2d, &RectangleCollider2d), Without<Candidate>>,
    overlaps: RectangleOverlapEntities<With<Candidate>>,
    mut observation: ResMut<BarrierObservation>,
) -> Result<(), QueryEntityError> {
    let (transform, collider) = sources.get(entities.source)?;
    observation.0.push(
        overlaps
            .iter_overlapping(entities.source, transform, collider)?
            .collect(),
    );
    Ok(())
}

#[test]
fn queued_insert_and_remove_cross_only_the_stage_barrier() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Candidate>()?;
    application.add_fallible_system(Stage::FixedUpdate, queue_component_changes);
    application.add_fallible_system(Stage::FixedUpdate, observe_component_changes);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let initial = application.register_world("rectangle-overlap-barrier", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((Transform2d::default(), collider))?;
        let removed = world.spawn((Candidate, Transform2d::default(), collider))?;
        let inserted = world.spawn((Candidate, Transform2d::default()))?;
        world.insert_resource(BarrierEntities {
            source,
            removed,
            inserted,
            queued: false,
        })?;
        world.insert_resource(BarrierObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    advance(&mut runner)?;
    advance(&mut runner)?;
    let entities = runner
        .resource::<BarrierEntities>()
        .ok_or("barrier entities should remain")?;
    let observations = &runner
        .resource::<BarrierObservation>()
        .ok_or("barrier observations should remain")?
        .0;
    assert_eq!(
        observations,
        &[vec![entities.removed], vec![entities.inserted]]
    );
    assert!(matches!(
        runner.component::<RectangleCollider2d>(entities.removed),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(
        runner
            .component::<RectangleCollider2d>(entities.inserted)?
            .size(),
        Vec2::splat(2.0)
    );
    Ok(())
}

#[derive(Resource, Default)]
struct DisjointObservation {
    has_overlap: bool,
    overlap_count: usize,
}

type MutableSourceData = (
    LogicEntityRef,
    &'static mut Transform2d,
    &'static RectangleCollider2d,
);
type MutableSourceFilter = (With<Source>, Without<Candidate>);
type DisjointCandidateFilter = (With<Candidate>, Without<Source>);

fn move_source_and_query_disjoint_candidates(
    mut source: Query<MutableSourceData, MutableSourceFilter>,
    candidates: RectangleOverlapEntities<DisjointCandidateFilter>,
    mut observation: ResMut<DisjointObservation>,
) -> Result<(), Box<dyn Error>> {
    let (entity, mut transform, collider) = source.single_mut()?;
    transform.set_translation(Vec2::ZERO)?;
    observation.has_overlap = candidates.has_overlap(entity.handle(), &transform, collider)?;
    observation.overlap_count = candidates
        .iter_overlapping(entity.handle(), &transform, collider)?
        .count();
    Ok(())
}

#[test]
fn disjoint_filters_allow_moving_a_source_and_querying_in_one_system() -> Result<(), Box<dyn Error>>
{
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_components::<(Source, Candidate)>()?;
    application.add_fallible_system(
        Stage::FixedUpdate,
        move_source_and_query_disjoint_candidates,
    );
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let source_transform = Transform2d::new(Vec2::new(10.0, 0.0))?;
    let initial = application.register_world("disjoint-rectangle-overlap", move |world| {
        world.spawn(camera)?;
        world.spawn((Source, source_transform, collider))?;
        world.spawn((Candidate, Transform2d::default(), collider))?;
        world.insert_resource(DisjointObservation::default())?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    advance(&mut runner)?;
    let observation = runner
        .resource::<DisjointObservation>()
        .ok_or("disjoint observation should remain")?;
    assert!(observation.has_overlap);
    assert_eq!(observation.has_overlap, observation.overlap_count != 0);
    assert_eq!(observation.overlap_count, 1);
    Ok(())
}

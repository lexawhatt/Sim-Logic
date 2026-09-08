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
    tangent: LogicEntity,
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
        .set_translation(Vec2::new(0.5, 0.0))
        .expect("test translation should be valid");
    Ok(())
}

fn observe_filtered_overlaps(
    known: Res<KnownEntities>,
    sources: Query<(&Transform2d, &CircleCollider2d)>,
    overlaps: CircleOverlapEntities<With<Candidate>>,
    self_only: CircleOverlapEntities<With<Source>>,
    disabled_only: CircleOverlapEntities<With<Disabled>>,
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
        return Err("bounded overlap frame was rejected".into());
    };
    if let Some(failure) = report.failure() {
        return Err(format!("overlap frame failed: {failure}").into());
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
    let collider = CircleCollider2d::new(1.0)?;
    let tangent_transform = Transform2d::new(Vec2::new(2.0, 0.0))?;
    let moved_transform = Transform2d::new(Vec2::new(10.0, 0.0))?;
    let outside_transform = Transform2d::new(Vec2::new(2.01, 0.0))?;
    let initial = application.register_world("circle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((Source, Candidate, Transform2d::default(), collider))?;
        let tangent = world.spawn((Candidate, tangent_transform, collider))?;
        let moved = world.spawn((Candidate, moved_transform, collider))?;
        let outside = world.spawn((Candidate, outside_transform, collider))?;
        let unrelated = world.spawn((Unrelated, Transform2d::default(), collider))?;
        let disabled = world.spawn((Candidate, Disabled, Transform2d::default(), collider))?;
        world.insert_resource(KnownEntities {
            source,
            tangent,
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
        .ok_or("observation resource should remain")?;
    assert_eq!(observation.hits.len(), 2);
    assert_eq!(observation.has_overlap, !observation.hits.is_empty());
    assert!(!observation.has_self_overlap);
    assert!(!observation.has_disabled_overlap);
    assert!(observation.hits.contains(&known.tangent));
    assert!(observation.hits.contains(&known.moved));
    assert!(!observation.hits.contains(&known.source));
    assert!(!observation.hits.contains(&known.outside));
    assert!(!observation.hits.contains(&known.unrelated));
    assert!(!observation.hits.contains(&known.disabled));
    Ok(())
}

#[derive(Resource)]
struct BarrierEntities {
    source: LogicEntity,
    target: LogicEntity,
}

#[derive(Resource, Default)]
struct BarrierObservation {
    overlaps_before_barrier: usize,
}

#[derive(Resource, Clone, Copy)]
struct SpawnTemplate(CircleCollider2d);

fn queue_target_despawn(
    entities: Res<BarrierEntities>,
    template: Res<SpawnTemplate>,
    mut commands: Commands,
) -> Result<(), Box<dyn Error>> {
    commands.despawn(entities.target)?;
    commands.spawn((Candidate, Transform2d::default(), template.0))?;
    Ok(())
}

fn observe_before_barrier(
    entities: Res<BarrierEntities>,
    sources: Query<(&Transform2d, &CircleCollider2d)>,
    overlaps: CircleOverlapEntities<With<Candidate>>,
    mut observation: ResMut<BarrierObservation>,
) -> Result<(), QueryEntityError> {
    let (transform, collider) = sources.get(entities.source)?;
    observation.overlaps_before_barrier = overlaps
        .iter_overlapping(entities.source, transform, collider)?
        .count();
    Ok(())
}

#[test]
fn queued_spawn_and_despawn_cross_only_the_stage_barrier() -> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_time(TimeConfig::new(FIXED_STEP, 1)?);
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Candidate>()?;
    application.add_fallible_system(Stage::FixedUpdate, queue_target_despawn);
    application.add_fallible_system(Stage::FixedUpdate, observe_before_barrier);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let collider = CircleCollider2d::new(1.0)?;
    let initial = application.register_world("overlap-barrier", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((Transform2d::default(), collider))?;
        let target = world.spawn((Candidate, Transform2d::default(), collider))?;
        world.insert_resource(BarrierEntities { source, target })?;
        world.insert_resource(BarrierObservation::default())?;
        world.insert_resource(SpawnTemplate(collider))?;
        Ok(())
    })?;
    let mut runner = application.build_headless(initial)?;

    let report = advance(&mut runner)?;
    assert_eq!(report.despawned(), 1);
    assert_eq!(report.spawned(), 1);
    assert_eq!(
        runner
            .resource::<BarrierObservation>()
            .map(|observation| observation.overlaps_before_barrier),
        Some(1),
        "the old target must remain visible and the queued spawn must remain hidden"
    );
    let target = runner
        .resource::<BarrierEntities>()
        .ok_or("barrier entities should remain")?
        .target;
    assert!(matches!(
        runner.component::<CircleCollider2d>(target),
        Err(QueryEntityError::DoesNotMatch { .. })
    ));
    assert_eq!(runner.components::<Candidate>().count(), 1);
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
    geometry: Query<(&Transform2d, &CircleCollider2d)>,
    overlaps: CircleOverlapEntities,
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
    let collider = CircleCollider2d::new(1.0)?;
    let foreign_world = foreign_application.register_world("foreign", move |world| {
        world.spawn(camera)?;
        world.spawn((Transform2d::default(), collider))?;
        Ok(())
    })?;
    let foreign_runner = foreign_application.build_headless(foreign_world)?;
    let foreign = foreign_runner
        .components::<CircleCollider2d>()
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

#[derive(Resource, Default)]
struct DisjointObservation {
    has_overlap: bool,
    overlap_count: usize,
}

type MutableSourceData = (
    LogicEntityRef,
    &'static mut Transform2d,
    &'static CircleCollider2d,
);
type MutableSourceFilter = (With<Source>, Without<Candidate>);
type DisjointCandidateFilter = (With<Candidate>, Without<Source>);

fn move_source_and_query_disjoint_candidates(
    mut source: Query<MutableSourceData, MutableSourceFilter>,
    candidates: CircleOverlapEntities<DisjointCandidateFilter>,
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
    let collider = CircleCollider2d::new(1.0)?;
    let source_transform = Transform2d::new(Vec2::new(10.0, 0.0))?;
    let initial = application.register_world("disjoint-overlap", move |world| {
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

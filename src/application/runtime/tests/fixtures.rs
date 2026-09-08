//! Shared application objects and observation Systems for runtime tests.

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum TestAction {
    MoveLeft,
    MoveRight,
    MoveDown,
    MoveUp,
    Enter,
}

pub(super) const TEST_MOVEMENT: crate::input::DigitalAxis2d<TestAction> =
    crate::input::DigitalAxis2d::new(
        TestAction::MoveLeft,
        TestAction::MoveRight,
        TestAction::MoveDown,
        TestAction::MoveUp,
    );

#[derive(Component)]
pub(super) struct Ball;

#[derive(Component)]
pub(super) struct Ephemeral;

#[derive(Component)]
pub(super) struct SameStageValue(pub(super) u8);

#[derive(Resource)]
pub(super) struct NextWorld(pub(super) crate::identity::WorldFactoryId);

#[derive(Resource)]
pub(super) struct MissingFixedResource;

#[derive(Resource)]
pub(super) struct ResourceOnlyComponent;

#[derive(Resource, Default)]
pub(super) struct ManagedQueryCounts {
    pub(super) all: usize,
    pub(super) resource_components: usize,
}

#[derive(Resource, Default)]
pub(super) struct SameStageObservation(pub(super) Option<u8>);

#[derive(Debug, Clone, Copy)]
pub(super) struct PrimaryEvent(pub(super) u8);

#[derive(Debug, Clone, Copy)]
pub(super) struct SecondaryEvent(pub(super) u8);

#[derive(Resource, Default)]
pub(super) struct EventObservation {
    pub(super) startup_first: Vec<u8>,
    pub(super) startup_second: Vec<u8>,
    pub(super) secondary: Vec<u8>,
    pub(super) fixed_before_writer: Vec<usize>,
    pub(super) fixed_after_writer: Vec<Vec<u8>>,
    pub(super) frame_counts: Vec<usize>,
}

#[derive(Resource, Default)]
pub(super) struct EventFailureObservation {
    pub(super) attempts: usize,
    pub(super) later_runs: usize,
}

#[derive(Resource, Default)]
pub(super) struct StartupEventCount(pub(super) usize);

pub(super) fn count_managed_query_entities(
    all: Query<()>,
    resource_components: Query<&ResourceOnlyComponent>,
    mut counts: ResMut<ManagedQueryCounts>,
) {
    counts.all = all.iter().count();
    counts.resource_components = resource_components.iter().count();
}

pub(super) fn write_same_stage_component(mut values: Query<&mut SameStageValue>) {
    for mut value in &mut values {
        value.0 = 7;
    }
}

pub(super) fn observe_same_stage_component(
    values: Query<&SameStageValue>,
    mut observation: ResMut<SameStageObservation>,
) {
    observation.0 = values.iter().next().map(|value| value.0);
}

pub(super) fn send_startup_events(
    mut primary: EventWriter<PrimaryEvent>,
    mut secondary: EventWriter<SecondaryEvent>,
) {
    assert!(primary.send(PrimaryEvent(3)).is_ok());
    assert_eq!(primary.iter().map(|event| event.0).collect::<Vec<_>>(), [3]);
    assert!(primary.send(PrimaryEvent(4)).is_ok());
    assert!(secondary.send(SecondaryEvent(9)).is_ok());
}

pub(super) fn read_startup_events_first(
    primary: EventReader<PrimaryEvent>,
    secondary: EventReader<SecondaryEvent>,
    mut observation: ResMut<EventObservation>,
) {
    observation.startup_first = primary.iter().map(|event| event.0).collect();
    observation.secondary = secondary.iter().map(|event| event.0).collect();
}

pub(super) fn read_startup_events_second(
    primary: EventReader<PrimaryEvent>,
    mut observation: ResMut<EventObservation>,
) {
    observation.startup_second = (&primary).into_iter().map(|event| event.0).collect();
}

pub(super) fn read_fixed_before_writer(
    events: EventReader<PrimaryEvent>,
    mut observation: ResMut<EventObservation>,
) {
    observation.fixed_before_writer.push(events.len());
}

pub(super) fn send_fixed_event(time: FixedTime, mut events: EventWriter<PrimaryEvent>) {
    let value = u8::try_from(time.tick_index()).expect("test tick index should fit in u8");
    assert!(events.send(PrimaryEvent(value)).is_ok());
}

pub(super) fn read_fixed_after_writer(
    events: EventReader<PrimaryEvent>,
    mut observation: ResMut<EventObservation>,
) {
    observation
        .fixed_after_writer
        .push(events.iter().map(|event| event.0).collect());
}

pub(super) fn read_frame_events(
    events: EventReader<PrimaryEvent>,
    mut observation: ResMut<EventObservation>,
) {
    observation.frame_counts.push(events.len());
}

#[derive(Resource)]
pub(super) struct TargetWorld;

#[derive(Default, Resource)]
pub(super) struct EnabledBallCount(pub(super) usize);

#[derive(Resource)]
pub(super) struct StoredFrameIntent(pub(super) Option<TransitionIntentToken>);

#[derive(Resource)]
pub(super) struct StartupIntentObservation {
    pub(super) rejected_as_unavailable: bool,
}

#[derive(Resource)]
pub(super) struct StartupExitObservation {
    pub(super) rejected_as_unavailable: bool,
}

#[derive(Resource)]
pub(super) struct StartupPauseObservation {
    pub(super) rejected_as_unavailable: bool,
}

#[derive(Default, Resource)]
pub(super) struct SpawnThenDespawn(pub(super) bool);

#[derive(Default, Resource)]
pub(super) struct RejectSecondFixedBatch(pub(super) u8);

#[derive(Default, Resource)]
pub(super) struct CameraRepairPhase(pub(super) u8);

#[derive(Resource)]
pub(super) struct InsertTarget(pub(super) LogicEntity);

#[derive(Default, Resource)]
pub(super) struct InsertObservation {
    pub(super) queued: bool,
    pub(super) same_stage_counts: Vec<usize>,
}

#[derive(Default, Resource)]
pub(super) struct RemoveObservation {
    pub(super) queued: bool,
    pub(super) same_stage_counts: Vec<usize>,
}

#[derive(Default, Resource)]
pub(super) struct StageInputObservation {
    pub(super) fixed_actual_edges: usize,
    pub(super) fixed_foreign_edges: usize,
    pub(super) fixed_foreign_held: bool,
    pub(super) frame_actual_edges: usize,
    pub(super) frame_foreign_edges: usize,
    pub(super) frame_foreign_held: bool,
}

#[derive(Default, Resource)]
pub(super) struct RetainedEdgeObservation {
    pub(super) pressed: usize,
    pub(super) released: usize,
    pub(super) held: bool,
}

#[derive(Default, Resource)]
pub(super) struct FallibleObservation {
    pub(super) attempts: usize,
    pub(super) later_runs: usize,
    pub(super) frame_runs: usize,
}

#[derive(Default, Resource)]
pub(super) struct FloatTimeObservation {
    pub(super) fixed_bits: Option<u32>,
    pub(super) frame_bits: Option<u32>,
}

#[derive(Resource)]
pub(super) struct RejectStartup;

pub(super) fn move_ball(
    input: FixedInput<TestAction>,
    time: FixedTime,
    mut balls: Query<&mut Transform2d, With<Ball>>,
) {
    let direction = input.normalized_digital_axis(TEST_MOVEMENT);
    let delta = direction * (60.0 * time.seconds_f32());
    for mut transform in &mut balls {
        assert!(transform.translate_by(delta).is_ok());
    }
}

pub(super) fn move_insert_target(mut target: Single<&mut Transform2d, With<Ball>>) {
    assert!(target.translate_by(Vec2::new(2.0, 0.0)).is_ok());
}

pub(super) fn observe_insert_visibility(
    visuals: Query<&CircleVisual, With<Ball>>,
    mut observation: ResMut<InsertObservation>,
) {
    observation.same_stage_counts.push(visuals.iter().count());
}

pub(super) fn follow_ball(
    balls: Query<&Transform2d, With<Ball>>,
    mut cameras: Query<&mut ActiveCamera2d>,
) -> Result<(), Box<dyn Error>> {
    let target = balls.single()?.translation();
    cameras.single_mut()?.set_center(target)?;
    Ok(())
}

pub(super) fn request_next_world(
    input: FixedInput<TestAction>,
    next: Option<Res<NextWorld>>,
    mut commands: LogicCommands,
) {
    let Some(next) = next else {
        return;
    };
    for edge in input.pressed(TestAction::Enter) {
        assert!(commands.replace_world(edge.intent(), next.0).is_ok());
    }
}

pub(super) fn require_missing_fixed_resource(_missing: Res<MissingFixedResource>) {}

pub(super) fn fixed_reads_frame_time(_time: FrameTime) {}

pub(super) fn fixed_reads_frame_viewport(_viewport: FrameViewport) {}

pub(super) fn frame_reads_fixed_time(_time: FixedTime) {}

pub(super) fn startup_reads_frame_input(_input: FrameInput<TestAction>) {}

pub(super) fn startup_reads_fixed_time(_time: FixedTime) {}

pub(super) fn observe_startup_intent(
    mut observation: ResMut<StartupIntentObservation>,
    mut commands: LogicCommands,
) {
    observation.rejected_as_unavailable = matches!(
        commands.new_transition_intent(),
        Err(CommandEnqueueError::IntentUnavailable)
    );
}

pub(super) fn observe_startup_exit(
    mut observation: ResMut<StartupExitObservation>,
    mut commands: LogicCommands,
) {
    observation.rejected_as_unavailable = matches!(
        commands.request_exit(),
        Err(CommandEnqueueError::ExitUnavailable)
    );
}

pub(super) fn observe_startup_pause(
    mut observation: ResMut<StartupPauseObservation>,
    mut commands: LogicCommands,
) {
    observation.rejected_as_unavailable = matches!(
        commands.set_paused(true),
        Err(CommandEnqueueError::PauseUnavailable)
    );
}

pub(super) fn observe_fixed_stage_input(
    fixed: FixedInput<TestAction>,
    frame: FrameInput<TestAction>,
    mut observation: ResMut<StageInputObservation>,
) {
    observation.fixed_actual_edges = fixed.pressed(TestAction::MoveRight).count();
    observation.fixed_foreign_edges = frame.pressed(TestAction::MoveRight).count();
    observation.fixed_foreign_held = frame.held(TestAction::MoveRight);
}

pub(super) fn observe_frame_stage_input(
    frame: FrameInput<TestAction>,
    fixed: FixedInput<TestAction>,
    mut observation: ResMut<StageInputObservation>,
) {
    observation.frame_actual_edges = frame.pressed(TestAction::MoveRight).count();
    observation.frame_foreign_edges = fixed.pressed(TestAction::MoveRight).count();
    observation.frame_foreign_held = fixed.held(TestAction::MoveRight);
}

pub(super) fn observe_retained_edges(
    input: FixedInput<TestAction>,
    mut observation: ResMut<RetainedEdgeObservation>,
) {
    observation.pressed = input.pressed(TestAction::MoveRight).count();
    observation.released = input.released(TestAction::MoveRight).count();
    observation.held = input.held(TestAction::MoveRight);
}

pub(super) fn reject_marked_startup(
    reject: Option<Res<RejectStartup>>,
) -> Result<(), &'static str> {
    if reject.is_some() {
        Err("candidate startup rejected")
    } else {
        Ok(())
    }
}

pub(super) fn spawn_then_despawn(
    mut phase: ResMut<SpawnThenDespawn>,
    entities: Query<LogicEntityRef, With<Ephemeral>>,
    mut commands: LogicCommands,
) {
    if !phase.0 {
        assert!(commands.spawn(Ephemeral).is_ok());
        phase.0 = true;
        return;
    }

    for entity in &entities {
        assert!(commands.despawn(entity.handle()).is_ok());
    }
}

pub(super) fn reject_second_fixed_batch(
    mut phase: ResMut<RejectSecondFixedBatch>,
    mut commands: LogicCommands,
) {
    if phase.0 == 0 {
        phase.0 = 1;
        return;
    }
    if phase.0 > 1 {
        return;
    }
    phase.0 = 2;
    assert!(commands.spawn(Ephemeral).is_ok());
    assert!(matches!(
        commands.spawn(Ephemeral),
        Err(CommandEnqueueError::LimitExceeded { limit: 1 })
    ));
}

pub(super) fn viewport() -> Result<LogicalViewport, Box<dyn Error>> {
    Ok(LogicalViewport::new(800.0, 600.0)?)
}

pub(super) fn camera() -> Result<ActiveCamera2d, Box<dyn Error>> {
    Ok(ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?))
}

pub(super) fn circle(color: Color) -> Result<CircleVisual, Box<dyn Error>> {
    Ok(CircleVisual::new(1.0, color)?)
}

pub(super) fn read_transform_for_empty_stage_boundary(transforms: Query<&Transform2d, With<Ball>>) {
    assert_eq!(transforms.iter().count(), 1);
}

use std::error::Error;

use bevy_ecs::world::World;

use super::*;
use crate::{
    extraction::{ExtractionBuffers, ExtractionParameters},
    identity::ApplicationId,
    visual::ActiveCamera2d,
};

fn source(
    world: &mut World,
    application: u64,
    generation: u64,
) -> Result<ScreenRectangleSource, Box<dyn Error>> {
    let application = ApplicationId::from_raw(application);
    let generation = WorldGeneration::new(application, generation);
    let entity = LogicEntity::new(application, generation, world.spawn_empty().id());
    let visual = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(-8.0, 16.0),
        LogicalScreenVector::new(24.0, 10.0),
        Color::WHITE,
    )?;
    Ok(ScreenRectangleSource::new(entity, &visual))
}

#[test]
fn failed_screen_candidate_cannot_publish_a_partial_frame_and_retry_clears_its_prefix()
-> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let old = source(&mut world, 1, 1)?;
    let candidate = source(&mut world, 1, 2)?;
    let foreign = source(&mut world, 2, 2)?;
    let old_generation = old.entity.world_generation();
    let next_generation = candidate.entity.world_generation();
    let limits = RenderLimits::default();
    let camera = ActiveCamera2d::centered(32.0)?;
    let mut buffers = ExtractionBuffers::new();
    buffers.stage(
        ExtractionParameters::new(old_generation, Color::BLACK, 0.0, limits),
        [camera],
        [],
        [],
        [],
        [old.into()],
    )?;
    assert_eq!(buffers.publish(), Some(old_generation));
    let original = buffers
        .published()
        .ok_or("initial snapshot")?
        .resolved_screen_rectangles()[0];

    let failed = buffers.stage(
        ExtractionParameters::new(next_generation, Color::WHITE, 0.0, limits),
        [camera],
        [],
        [],
        [],
        [candidate.into(), foreign.into()],
    );
    assert!(
        matches!(failed, Err(ExtractionError::ForeignEntity { entity, expected })
        if entity == foreign.entity && expected == next_generation)
    );
    assert_eq!(buffers.publish(), None);
    let retained = buffers.published().ok_or("retained snapshot")?;
    assert_eq!(retained.world_generation(), old_generation);
    assert_eq!(retained.background(), Color::BLACK);
    assert_eq!(retained.resolved_screen_rectangles(), [original]);

    buffers.stage(
        ExtractionParameters::new(next_generation, Color::WHITE, 0.0, limits),
        [camera],
        [],
        [],
        [],
        [candidate.into()],
    )?;
    assert_eq!(buffers.publish(), Some(next_generation));
    let replaced = buffers.published().ok_or("replacement snapshot")?;
    assert_eq!(replaced.resolved_screen_rectangles().len(), 1);
    assert_eq!(
        replaced.resolved_screen_rectangles()[0].source(),
        candidate.entity
    );
    assert_eq!(replaced.background(), Color::WHITE);
    Ok(())
}

#[test]
fn screen_clear_reuses_record_and_scene_storage_without_stale_commands()
-> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let first = source(&mut world, 1, 1)?;
    let second = source(&mut world, 1, 1)?;
    let generation = first.entity.world_generation();
    let limits = RenderLimits::default();
    let mut buffer = ScreenExtractionBuffer::new(limits)?;
    buffer.extract(generation, limits, [first.into(), second.into()])?;
    let address = buffer.resolved.as_ptr();
    let capacity = buffer.resolved.capacity();
    let scene_bytes = buffer.scene.allocation_bytes();
    assert_eq!(buffer.scene.command_count(), 2);

    buffer.clear();
    assert!(buffer.resolved.is_empty());
    assert_eq!(buffer.scene.command_count(), 0);
    assert_eq!(buffer.scene.statistics().accepted_commands(), 0);
    buffer.extract(generation, limits, [second.into()])?;
    assert_eq!(buffer.resolved.len(), 1);
    assert_eq!(buffer.resolved[0].source(), second.entity);
    assert_eq!(buffer.scene.command_count(), 1);
    assert_eq!(buffer.resolved.capacity(), capacity);
    assert_eq!(buffer.resolved.as_ptr(), address);
    assert_eq!(buffer.scene.allocation_bytes(), scene_bytes);
    Ok(())
}

fn line_source(entity: LogicEntity) -> Result<ScreenSource, Box<dyn Error>> {
    Ok(ScreenSource::Line(
        entity,
        crate::screen::ScreenLineVisual::new(
            LogicalScreenPosition::new(10.0, 10.0),
            LogicalScreenPosition::new(80.0, 20.0),
            2.0,
            Color::WHITE,
        )?,
    ))
}

#[test]
fn aggregate_command_limit_precedes_vector_allocation() -> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let rectangle = source(&mut world, 1, 1)?;
    let generation = rectangle.entity.world_generation();
    let default = RenderLimits::default().screen_scene_budget();
    for commands in [0, 1] {
        let budget = sim_engine::SceneBudget::new(
            commands,
            0,
            default.max_tessellated_vertices(),
            default.max_retained_bytes(),
            default.max_allocation_bytes(),
            default.max_upload_bytes(),
            default.max_draw_batches(),
        );
        let limits = RenderLimits::default().with_screen_scene_budget(budget);
        let mut buffer = ScreenExtractionBuffer::new(limits)?;
        let sources = (commands == 1)
            .then_some(ScreenSource::Rectangle(rectangle))
            .into_iter()
            .chain([line_source(rectangle.entity)?]);
        assert!(matches!(
            buffer.extract(generation, limits, sources),
            Err(ExtractionError::ScreenScene(SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands, limit, requested,
            })) if limit == commands && requested == commands + 1
        ));
        assert_eq!(buffer.primitives.capacity(), 0);
        assert_eq!(buffer.scene.command_count(), 0);
    }
    Ok(())
}

#[test]
fn failed_vector_candidate_preserves_snapshot_and_clears_prefix_on_retry()
-> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let old = source(&mut world, 1, 1)?;
    let next = source(&mut world, 1, 2)?;
    let foreign = source(&mut world, 2, 2)?;
    let limits = RenderLimits::default();
    let camera = ActiveCamera2d::centered(32.0)?;
    let mut buffers = ExtractionBuffers::new();
    let parameters = |generation| ExtractionParameters::new(generation, Color::BLACK, 0.0, limits);
    buffers.stage(
        parameters(old.entity.world_generation()),
        [camera],
        [],
        [],
        [],
        [line_source(old.entity)?],
    )?;
    assert_eq!(buffers.publish(), Some(old.entity.world_generation()));
    let original: Vec<_> = buffers
        .published()
        .ok_or("snapshot")?
        .screen_primitives()
        .collect();
    assert!(matches!(
        buffers.stage(
            parameters(next.entity.world_generation()),
            [camera],
            [],
            [],
            [],
            [line_source(next.entity)?, line_source(foreign.entity)?],
        ),
        Err(ExtractionError::ForeignEntity { .. })
    ));
    assert_eq!(buffers.publish(), None);
    let snapshot = buffers.published().ok_or("retained snapshot")?;
    assert_eq!(snapshot.screen_primitives().collect::<Vec<_>>(), original);
    assert_eq!(snapshot.world_generation(), old.entity.world_generation());
    buffers.stage(
        parameters(next.entity.world_generation()),
        [camera],
        [],
        [],
        [],
        [line_source(next.entity)?],
    )?;
    assert_eq!(buffers.publish(), Some(next.entity.world_generation()));
    let snapshot = buffers.published().ok_or("replacement snapshot")?;
    let run: Vec<_> = snapshot
        .screen_primitive_run_records(0)
        .ok_or("vector run")?
        .collect();
    assert_eq!(run.len(), 1);
    assert_eq!(run[0].source(), next.entity);
    assert!(
        snapshot
            .screen_rectangle_run_records(0)
            .ok_or("rectangle subset")?
            .is_empty()
    );
    assert!(snapshot.screen_primitive_run_records(1).is_none());
    Ok(())
}

#[test]
fn warmed_vector_extraction_reuses_storage_without_stale_geometry() -> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let source = source(&mut world, 1, 1)?;
    let limits = RenderLimits::default();
    let generation = source.entity.world_generation();
    let mut buffer = ScreenExtractionBuffer::new(limits)?;
    buffer.extract(generation, limits, [line_source(source.entity)?])?;
    let address = buffer.primitives.as_ptr();
    let capacity = buffer.primitives.capacity();
    let scene_bytes = buffer.scene.allocation_bytes();
    for _ in 0..10 {
        buffer.clear();
        buffer.extract(generation, limits, [line_source(source.entity)?])?;
        assert_eq!(buffer.primitives.as_ptr(), address);
        assert_eq!(buffer.primitives.capacity(), capacity);
        assert_eq!(buffer.scene.allocation_bytes(), scene_bytes);
        assert_eq!(buffer.scene.command_count(), 1);
    }
    buffer.clear();
    buffer.extract(generation, limits, [ScreenSource::Rectangle(source)])?;
    assert_eq!(buffer.draws(), &[ScreenDraw::Rectangles { run: 0 }]);
    assert_eq!(buffer.geometry_len(), 1);
    assert_eq!(buffer.scene.command_count(), 1);
    Ok(())
}

#[test]
fn whole_path_is_one_engine_command_and_foreign_hidden_paths_are_rejected()
-> Result<(), Box<dyn Error>> {
    let mut world = World::new();
    let source = source(&mut world, 1, 1)?;
    let points: Vec<_> = (0..10_000)
        .map(|i| LogicalScreenPosition::new(i as f32 * 0.25, 30.5))
        .collect();
    let mut path = crate::screen::ScreenPolylineVisual::new(
        &points,
        sim_engine::StrokeStyle2d::new(1.5, Color::WHITE),
    )?;
    let limits = RenderLimits::default().with_screen_scene_budget(sim_engine::SceneBudget::new(
        256,
        10_000,
        2_000_000,
        8 * 1024 * 1024,
        16 * 1024 * 1024,
        128 * 1024 * 1024,
        256,
    ));
    let generation = source.entity.world_generation();
    let mut buffer = ScreenExtractionBuffer::new(limits)?;
    buffer.extract(
        generation,
        limits,
        [ScreenSource::Polyline(source.entity, path.clone())],
    )?;
    assert_eq!(buffer.scene.command_count(), 1);
    assert_eq!(buffer.scene.statistics().retained_points(), 10_000);
    buffer.clear();
    path.set_clip(crate::screen::ScreenClip::Empty);
    buffer.extract(
        generation,
        limits,
        [ScreenSource::Polyline(source.entity, path.clone())],
    )?;
    assert_eq!(buffer.scene.command_count(), 0);
    assert_eq!(buffer.geometry_len(), 1);
    buffer.clear();
    let foreign = WorldGeneration::new(ApplicationId::from_raw(2), 1);
    assert!(matches!(
        buffer.extract(
            foreign,
            limits,
            [ScreenSource::Polyline(source.entity, path)]
        ),
        Err(ExtractionError::ForeignEntity { .. })
    ));
    Ok(())
}

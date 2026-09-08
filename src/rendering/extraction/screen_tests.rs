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
        [old],
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
        [candidate, foreign],
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
        [candidate],
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
    buffer.extract(generation, limits, [first, second])?;
    let address = buffer.resolved.as_ptr();
    let capacity = buffer.resolved.capacity();
    let scene_bytes = buffer.scene.allocation_bytes();
    assert_eq!(buffer.scene.command_count(), 2);

    buffer.clear();
    assert!(buffer.resolved.is_empty());
    assert_eq!(buffer.scene.command_count(), 0);
    assert_eq!(buffer.scene.statistics().accepted_commands(), 0);
    buffer.extract(generation, limits, [second])?;
    assert_eq!(buffer.resolved.len(), 1);
    assert_eq!(buffer.resolved[0].source(), second.entity);
    assert_eq!(buffer.scene.command_count(), 1);
    assert_eq!(buffer.resolved.capacity(), capacity);
    assert_eq!(buffer.resolved.as_ptr(), address);
    assert_eq!(buffer.scene.allocation_bytes(), scene_bytes);
    Ok(())
}

use super::*;
use crate::{
    assets::{ImageAssetLimits, ImageAssetRegistry},
    identity::ApplicationId,
    prelude::*,
};
use bevy_ecs::world::World;

struct Fixture {
    application: ApplicationId,
    generation: WorldGeneration,
    limits: RenderLimits,
    sources: Vec<ScreenSource>,
    registry: ImageAssetRegistry,
}

fn fixture() -> LogicResult<Fixture> {
    let application = ApplicationId::issue()?;
    let generation = WorldGeneration::new(application, 1);
    let mut world = World::new();
    let mut registry = ImageAssetRegistry::new(application, ImageAssetLimits::default());
    let image = registry.register(1, 1, &[255; 4])?;
    let mut sources = Vec::new();
    for layer in [10, 20, 30, 40, 50] {
        let entity = LogicEntity::new(application, generation, world.spawn_empty().id());
        if layer % 20 == 0 {
            let mut visual = ScreenImageVisual::new(
                image,
                LogicalScreenPosition::new(2.0, 2.0),
                LogicalScreenVector::new(8.0, 8.0),
            )?;
            visual.set_layer(Layer::new(layer));
            sources.push(ScreenSource::Image(ScreenImageSource::new(
                entity, &visual, &registry,
            )));
        } else {
            let mut visual = ScreenRectangleVisual::new(
                LogicalScreenPosition::new(layer as f32, 10.0),
                LogicalScreenVector::new(8.0, 8.0),
                Color::WHITE,
            )?;
            visual.set_layer(Layer::new(layer));
            sources.push(ScreenRectangleSource::new(entity, &visual).into());
        }
    }
    Ok(Fixture {
        application,
        generation,
        limits: RenderLimits::default().with_max_screen_images(2),
        sources,
        registry,
    })
}

fn first_image(f: &Fixture) -> LogicResult<(LogicEntity, ScreenImageVisual)> {
    let ScreenSource::Image(source) = f.sources[1] else {
        panic!("image")
    };
    let resolved = source.resolve(f.generation)?;
    let mut visual =
        ScreenImageVisual::new(resolved.image(), resolved.position(), resolved.size())?;
    visual.set_layer(resolved.layer());
    Ok((resolved.source(), visual))
}

fn assert_matches_fresh(buffer: &ScreenExtractionBuffer, fixture: &Fixture) -> LogicResult {
    let mut fresh = ScreenExtractionBuffer::new(fixture.limits)?;
    fresh.extract(fixture.generation, fixture.limits, fixture.sources.clone())?;
    assert_eq!(buffer.resolved, fresh.resolved);
    assert_eq!(buffer.primitives, fresh.primitives);
    assert_eq!(buffer.images, fresh.images);
    assert_eq!(buffer.draws, fresh.draws);
    assert_eq!(buffer.scene.statistics(), fresh.scene.statistics());
    // Compare Engine's actual command representation, not only logical counts.
    // clear() deliberately retains the last clip even for an empty scene.
    if buffer.scene.command_count() > 0 {
        assert_eq!(format!("{:?}", buffer.scene), format!("{:?}", fresh.scene));
    }
    assert_eq!(buffer.runs.len(), fresh.runs.len());
    for (cached, fresh) in buffer.runs.iter().zip(&fresh.runs) {
        assert_eq!((cached.start, cached.end), (fresh.start, fresh.end));
        assert_eq!(cached.records, fresh.records);
        assert_eq!(cached.scene.statistics(), fresh.scene.statistics());
        if cached.scene.command_count() > 0 {
            assert_eq!(format!("{:?}", cached.scene), format!("{:?}", fresh.scene));
        }
    }
    Ok(())
}

#[test]
fn idle_sources_skip_collection_sorting_and_scene_builds() -> LogicResult {
    let f = fixture()?;
    let mut buffer = ScreenExtractionBuffer::new(f.limits)?;
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(!buffer.updates.reused_snapshot);
    assert_eq!(buffer.updates.rebuilt_runs, 3);
    let keys = buffer.source_keys.as_ptr();
    let primitives = buffer.resolved.as_ptr();
    for _ in 0..10 {
        buffer.extract(f.generation, f.limits, f.sources.iter().cloned())?;
        assert!(buffer.updates.reused_snapshot);
        assert!(buffer.updates.reused_scene);
        assert_eq!(buffer.updates.compared_sources, 5);
        assert_eq!(buffer.updates.reused_runs, 3);
        assert_eq!(buffer.updates.rebuilt_runs, 0);
        assert_eq!(keys, buffer.source_keys.as_ptr());
        assert_eq!(primitives, buffer.resolved.as_ptr());
        assert!(buffer.source_scratch.is_empty());
        assert_matches_fresh(&buffer, &f)?;
    }
    Ok(())
}

#[test]
fn image_motion_reuses_geometry_but_changed_partition_does_not() -> LogicResult {
    let mut f = fixture()?;
    let mut buffer = ScreenExtractionBuffer::new(f.limits)?;
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    let (entity, mut visual) = first_image(&f)?;
    visual.set_position(LogicalScreenPosition::new(11.5, 8.25))?;
    f.sources[1] = ScreenSource::Image(ScreenImageSource::new(entity, &visual, &f.registry));
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(!buffer.updates.reused_snapshot);
    assert!(buffer.updates.reused_scene);
    assert_eq!(buffer.updates.reused_runs, 3);
    assert_eq!(buffer.updates.rebuilt_runs, 0);
    assert_matches_fresh(&buffer, &f)?;
    visual.set_layer(Layer::new(60));
    f.sources[1] = ScreenSource::Image(ScreenImageSource::new(entity, &visual, &f.registry));
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(buffer.updates.reused_scene);
    assert_eq!(buffer.updates.rebuilt_runs, 2);
    assert_matches_fresh(&buffer, &f)?;
    Ok(())
}

#[test]
fn one_geometry_edit_rebuilds_aggregate_and_only_its_mixed_run() -> LogicResult {
    let mut f = fixture()?;
    let mut buffer = ScreenExtractionBuffer::new(f.limits)?;
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    let ScreenSource::Rectangle(source) = &mut f.sources[2] else {
        panic!("rectangle")
    };
    source.visual.set_color(Color::BLACK)?;
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(!buffer.updates.reused_scene);
    assert_eq!(buffer.updates.rebuilt_runs, 1);
    assert_eq!(buffer.updates.reused_runs, 2);
    assert_matches_fresh(&buffer, &f)?;
    Ok(())
}

#[test]
fn exact_prefix_replay_handles_append_remove_permutation_and_empty() -> LogicResult {
    let mut f = fixture()?;
    let mut buffer = ScreenExtractionBuffer::new(f.limits)?;
    let original = f.sources.clone();
    for variant in [
        original.clone(),
        original[..4].to_vec(),
        original.clone(),
        original[1..].to_vec(),
        original.iter().rev().cloned().collect(),
        vec![],
        vec![],
        original,
    ] {
        f.sources = variant;
        buffer.extract(f.generation, f.limits, f.sources.clone())?;
        assert_matches_fresh(&buffer, &f)?;
    }
    Ok(())
}

#[test]
fn scope_admission_and_registry_failures_cannot_reuse_an_old_proof() -> LogicResult {
    let mut f = fixture()?;
    let mut buffer = ScreenExtractionBuffer::new(f.limits)?;
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(
        buffer
            .extract(
                f.generation,
                f.limits.with_max_screen_images(0),
                f.sources.clone()
            )
            .is_err()
    );
    assert!(buffer.cache_scope.is_none());
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert!(!buffer.updates.reused_snapshot);
    let other = WorldGeneration::new(f.application, 2);
    assert!(buffer.extract(other, f.limits, f.sources.clone()).is_err());
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    let wrong_registry =
        ImageAssetRegistry::new(ApplicationId::issue()?, ImageAssetLimits::default());
    let (entity, visual) = first_image(&f)?;
    f.sources[1] = ScreenSource::Image(ScreenImageSource::new(entity, &visual, &wrong_registry));
    assert!(matches!(
        buffer.extract(f.generation, f.limits, f.sources.clone()),
        Err(ExtractionError::UnregisteredImageAsset { .. })
    ));
    assert!(buffer.cache_scope.is_none());
    f.sources[1] = ScreenSource::Image(ScreenImageSource::new(entity, &visual, &f.registry));
    buffer.extract(f.generation, f.limits, f.sources.clone())?;
    assert_matches_fresh(&buffer, &f)?;
    Ok(())
}

#[test]
fn explicit_rebuild_route_never_reuses_proofs() -> LogicResult {
    let f = fixture()?;
    let limits = f.limits.with_screen_scene_reuse(false);
    let mut buffer = ScreenExtractionBuffer::new(limits)?;
    for _ in 0..5 {
        buffer.extract(f.generation, limits, f.sources.clone())?;
        assert!(!buffer.updates.reused_snapshot);
        assert!(!buffer.updates.reused_scene);
        assert_eq!(buffer.updates.rebuilt_runs, 3);
        assert_eq!(buffer.updates.reused_runs, 0);
        assert_matches_fresh(&buffer, &f)?;
    }
    Ok(())
}

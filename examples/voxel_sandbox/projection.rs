//! Dirty chunk projection: canonical blocks never contain managed/GPU identities.

use super::{
    app::Session,
    model::CHUNK_COUNT,
    scene::{Local, Phase},
};
use sim_logic::prelude::*;

#[derive(Component)]
pub struct ChunkPart {
    pub chunk: usize,
}

/// A stamp becomes visible only with its chunk's atomic structural batch.
/// Empty chunks have stamps too, so a missing projection is never mistaken for air.
#[derive(Component)]
pub struct ChunkStamp {
    pub chunk: usize,
    pub revision: u64,
    pub epoch: u64,
}

pub fn project(
    mut session: AppResMut<Session>,
    mut local: ResMut<Local>,
    parts: Query<(LogicEntityRef, &ChunkPart)>,
    stamps: Query<(LogicEntityRef, &ChunkStamp)>,
    mut commands: Commands,
) -> LogicResult {
    if local.departing {
        return Ok(());
    }
    if local.phase == Phase::Loading {
        if session
            .pending_load
            .as_ref()
            .is_some_and(|pending| pending.source != local.generation)
        {
            let pending = session
                .pending_load
                .take()
                .ok_or("pending load disappeared")?;
            session.game = pending.game;
            session.epoch = session.epoch.wrapping_add(1).max(1);
            session.notice = "Save loaded. Both regions restored.".into();
        }
        session.game.travel(local.region);
    }
    let region = session.game.region(local.region);
    let mut committed = [0; CHUNK_COUNT];
    for (_, stamp) in &stamps {
        if stamp.epoch == session.epoch {
            committed[stamp.chunk] = stamp.revision;
        }
    }
    local.chunk_revisions = committed;
    // Prepare every changed value before enqueueing structural changes.
    let mut replacements = Vec::new();
    for (chunk, committed_revision) in committed.into_iter().enumerate() {
        let revision = region.chunk_revision(chunk).ok_or("invalid chunk")?;
        if committed_revision == revision {
            continue;
        }
        let mut visuals = Vec::new();
        for part in region.chunk_meshes(chunk)? {
            visuals.push(MeshVisual3d::new(
                MeshAsset3d::new(part.mesh)?,
                sim_engine::Transform3d::IDENTITY,
                part.block.color(part.shade),
            )?);
        }
        replacements.push((chunk, revision, visuals));
    }
    let complete = replacements.is_empty();
    for (chunk, revision, visuals) in &replacements {
        for (entity, part) in &parts {
            if part.chunk == *chunk {
                commands.despawn(entity.handle())?;
            }
        }
        for (entity, stamp) in &stamps {
            if stamp.chunk == *chunk {
                commands.despawn(entity.handle())?;
            }
        }
        for visual in visuals {
            commands.spawn((ChunkPart { chunk: *chunk }, visual.clone()))?;
        }
        commands.spawn(ChunkStamp {
            chunk: *chunk,
            revision: *revision,
            epoch: session.epoch,
        })?;
    }
    for _ in replacements {
        local.rebuilt_chunks = local.rebuilt_chunks.saturating_add(1);
    }
    local.projected_epoch = session.epoch;
    if local.phase == Phase::Loading {
        local.phase = Phase::Queued;
    } else if local.phase == Phase::Queued && complete {
        local.phase = Phase::Ready;
    }
    Ok(())
}

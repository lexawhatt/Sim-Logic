//! Dirty chunk projection: canonical blocks never contain managed/GPU identities.

use super::{
    app::Session,
    materials::{self, Palette, Settings},
    model::Block,
    scene::{Local, Phase},
};
use sim_logic::prelude::*;

/// Stable rendering part identity within one World. A new source mesh is a
/// revision of this part, not a new entity, while this material/shade survives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Component)]
pub struct ChunkPart {
    pub chunk: usize,
    pub block: Block,
    pub shade: u8,
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
    palette: AppRes<Palette>,
    settings: Res<Settings>,
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
            session.notify("Save loaded. Both regions restored.");
        }
        session.game.travel(local.region);
    }
    session.game.stream_active()?;
    let region = session.game.region(local.region);
    local.chunk_revisions.clear();
    for (_, stamp) in &stamps {
        if stamp.epoch == session.epoch && region.chunk_revision(stamp.chunk).is_some() {
            local.chunk_revisions.push((stamp.chunk, stamp.revision));
        }
    }
    local
        .chunk_revisions
        .sort_unstable_by_key(|(chunk, _)| *chunk);
    // Only two chunks may generate meshes in one frame. The desired order starts
    // near the player; cache residency does not affect canonical collision.
    let mut replacements = Vec::new();
    let mut pending_meshes = 0;
    for &chunk in region.desired_chunks() {
        let Some(revision) = region.chunk_revision(chunk) else {
            continue;
        };
        let committed = local
            .chunk_revisions
            .binary_search_by_key(&chunk, |(id, _)| *id)
            .ok()
            .map(|index| local.chunk_revisions[index].1);
        if committed == Some(revision) {
            continue;
        }
        pending_meshes += 1;
        if replacements.len() >= 2 {
            continue;
        }
        let mut visuals = Vec::new();
        for part in region.chunk_meshes(chunk)? {
            visuals.push((
                ChunkPart {
                    chunk,
                    block: part.block,
                    shade: part.shade,
                },
                prepare_visual(part.mesh, part.block, part.shade, &palette, &settings)?,
            ));
        }
        replacements.push((chunk, revision, visuals));
    }
    let complete = pending_meshes == 0 && region.pending_chunks() == 0;
    // Retire no-longer-resident presentation only. Edits stay in the region's
    // sparse store and recreate the same cells after revisiting this coordinate.
    for (entity, part) in &parts {
        if region.chunk_revision(part.chunk).is_none() {
            commands.despawn(entity.handle())?;
        }
    }
    for (entity, stamp) in &stamps {
        if region.chunk_revision(stamp.chunk).is_none() {
            commands.despawn(entity.handle())?;
        }
    }
    for (chunk, revision, visuals) in &replacements {
        for (entity, part) in &parts {
            if part.chunk == *chunk {
                if let Some((_, visual)) = visuals.iter().find(|(key, _)| key == part) {
                    // This insert shares the same atomic barrier as the stamp.
                    // An Engine dynamic revision can retain the scene object ID
                    // only when the application retains its managed source ID.
                    commands.insert(entity.handle(), visual.clone())?;
                } else {
                    commands.despawn(entity.handle())?;
                }
            }
        }
        for (entity, stamp) in &stamps {
            if stamp.chunk == *chunk {
                commands.despawn(entity.handle())?;
            }
        }
        for (key, visual) in visuals {
            if !parts.iter().any(|(_, part)| part == key) {
                commands.spawn((*key, visual.clone()))?;
            }
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

/// Presentation-only construction point; terrain identity and Commands
/// publication do not depend on which Engine material this example selects.
fn prepare_visual(
    mesh: sim_engine::Mesh3d,
    block: Block,
    shade: u8,
    palette: &Palette,
    settings: &Settings,
) -> LogicResult<MeshVisual3d> {
    let mut visual = MeshVisual3d::with_surface(
        MeshAsset3d::new(mesh)?,
        sim_engine::Transform3d::IDENTITY,
        materials::surface(block, shade, settings)?,
    )?;
    visual.set_texture(Some(palette.texture(block, settings.mipmaps)?))?;
    Ok(visual)
}

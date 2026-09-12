use std::mem::size_of;

use sim_engine::{Color, Transform3d};

use crate::identity::{LogicEntity, WorldGeneration};

use super::{
    MeshVisual3d, ThreeDExtractionError, ThreeDLimitResource, ThreeDRenderLimits,
    extraction::check_limit,
};

/// One validated immutable mesh presentation and its managed source identity.
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMesh3d {
    source: LogicEntity,
    visual: MeshVisual3d,
}

impl ResolvedMesh3d {
    /// Returns the managed entity producing this mesh.
    pub const fn source(&self) -> LogicEntity {
        self.source
    }
    /// Returns the complete immutable-asset presentation value.
    pub const fn visual(&self) -> &MeshVisual3d {
        &self.visual
    }
    /// Returns the sampled transform without interpolation.
    pub const fn transform(&self) -> Transform3d {
        self.visual.transform()
    }
    /// Returns the normalized opaque surface color.
    pub const fn color(&self) -> Color {
        self.visual.color()
    }
}

pub(crate) struct MeshSource {
    entity: LogicEntity,
    visual: MeshVisual3d,
}

impl MeshSource {
    pub(crate) fn new(entity: LogicEntity, visual: &MeshVisual3d) -> Self {
        Self {
            entity,
            visual: visual.clone(),
        }
    }
}

pub(super) fn extract_meshes(
    resolved: &mut Vec<ResolvedMesh3d>,
    generation: WorldGeneration,
    limits: ThreeDRenderLimits,
    mut triangles: usize,
    sources: impl IntoIterator<Item = MeshSource>,
) -> Result<(), ThreeDExtractionError> {
    let mut source_bytes = 0_usize;
    for source in sources {
        if !source.visual.visible() {
            continue;
        }
        if source.entity.world_generation() != generation {
            return Err(ThreeDExtractionError::StaleEntity {
                source: source.entity,
            });
        }
        check_limit(
            ThreeDLimitResource::Meshes,
            resolved.len().saturating_add(1),
            limits.max_meshes(),
        )?;
        triangles = checked_sum(
            triangles,
            source.visual.asset().mesh().triangle_count(),
            ThreeDLimitResource::Triangles,
            limits.max_triangles(),
        )?;
        source_bytes = checked_sum(
            source_bytes,
            source.visual.asset().source_bytes(),
            ThreeDLimitResource::MeshSourceBytes,
            limits.max_mesh_source_bytes(),
        )?;
        if resolved.len() == resolved.capacity() {
            resolved
                .try_reserve_exact(1)
                .map_err(|_| ThreeDExtractionError::AllocationFailed {
                    requested_bytes: size_of::<ResolvedMesh3d>(),
                })?;
        }
        resolved.push(ResolvedMesh3d {
            source: source.entity,
            visual: source.visual,
        });
    }
    resolved.sort_unstable_by_key(|record| record.source.stable_bits());
    Ok(())
}

fn checked_sum(
    current: usize,
    additional: usize,
    resource: ThreeDLimitResource,
    limit: usize,
) -> Result<usize, ThreeDExtractionError> {
    let sum = current
        .checked_add(additional)
        .ok_or(ThreeDExtractionError::LimitExceeded {
            resource,
            requested: u64::MAX,
            limit: limit as u64,
        })?;
    check_limit(resource, sum, limit)?;
    Ok(sum)
}

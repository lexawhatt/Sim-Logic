use std::{error::Error, fmt, mem::size_of};

use sim_engine::{Color, Transform3d};

use crate::identity::{LogicEntity, WorldGeneration};

use super::{CuboidVisual3d, CuboidVisualError, MeshSource, ResolvedMesh3d, View3d, View3dError};

/// Frozen opt-in limits for geometry, textures and the desktop 3D passes.
///
/// Defaults are all zero. Every visible cuboid consumes twelve triangles and
/// at most twenty-four edge segments when both wireframe modes are selected.
/// Target pixels include one color and one depth attachment; mesh upload and
/// prepass work are separate from the final FrameBudget. The final composition
/// additionally consumes one target pass and its color texture byte allowance.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ThreeDRenderLimits {
    max_cuboids: usize,
    max_triangles: usize,
    max_target_pixels: u64,
    max_meshes: usize,
    max_mesh_source_bytes: usize,
    max_texture_source_bytes: usize,
    max_texture_gpu_bytes: usize,
}

impl ThreeDRenderLimits {
    /// Sets exact inclusive object, surface-triangle, and physical-pixel caps.
    pub const fn new(max_cuboids: usize, max_triangles: usize, max_target_pixels: u64) -> Self {
        Self {
            max_cuboids,
            max_triangles,
            max_target_pixels,
            max_meshes: 0,
            max_mesh_source_bytes: 0,
            max_texture_source_bytes: 0,
            max_texture_gpu_bytes: 0,
        }
    }

    /// Returns the maximum visible cuboid count and retained scene slot count.
    pub const fn max_cuboids(self) -> usize {
        self.max_cuboids
    }

    /// Returns the maximum submitted surface triangle count.
    pub const fn max_triangles(self) -> usize {
        self.max_triangles
    }

    /// Returns the maximum physical width times height of the depth target.
    pub const fn max_target_pixels(self) -> u64 {
        self.max_target_pixels
    }

    /// Enables custom meshes with inclusive instance and CPU-source caps.
    ///
    /// Source capacities are conservatively charged once per visible instance,
    /// even when instances share an asset. Triangle limits include both cuboids
    /// and custom meshes. Hidden/disabled meshes consume none of these limits.
    /// Engine's additional finite GPU/upload/scene-memory limits still apply.
    pub const fn with_mesh_limits(mut self, max_meshes: usize, max_source_bytes: usize) -> Self {
        self.max_meshes = max_meshes;
        self.max_mesh_source_bytes = max_source_bytes;
        self
    }

    /// Returns the maximum visible custom-mesh instance count.
    pub const fn max_meshes(self) -> usize {
        self.max_meshes
    }

    /// Returns the summed per-instance topology-capacity allowance.
    pub const fn max_mesh_source_bytes(self) -> usize {
        self.max_mesh_source_bytes
    }

    /// Enables textures with inclusive base-source and complete-mip texel caps.
    /// Both are conservatively charged per visible instance, including shared
    /// images. Engine additionally bounds its CPU mip storage, GPU resources and
    /// transient updates; these caps do not claim to measure driver allocations.
    pub const fn with_texture_limits(
        mut self,
        max_source_bytes: usize,
        max_gpu_bytes: usize,
    ) -> Self {
        self.max_texture_source_bytes = max_source_bytes;
        self.max_texture_gpu_bytes = max_gpu_bytes;
        self
    }

    /// Returns the per-visible-instance summed base-pixel capacity allowance.
    pub const fn max_texture_source_bytes(self) -> usize {
        self.max_texture_source_bytes
    }

    /// Returns the per-visible-instance summed nominal full-mip texel allowance.
    pub const fn max_texture_gpu_bytes(self) -> usize {
        self.max_texture_gpu_bytes
    }
}

/// Identifies which independent 3D work limit was exceeded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreeDLimitResource {
    /// Visible extracted cuboids and retained desktop scene slots.
    Cuboids,
    /// Visible custom-mesh instances.
    Meshes,
    /// CPU topology capacities charged once per visible custom-mesh instance.
    MeshSourceBytes,
    /// Base texture pixel capacities charged per visible textured mesh instance.
    TextureSourceBytes,
    /// Nominal complete-chain texture bytes charged per visible instance.
    TextureGpuBytes,
    /// Filled surface triangles from cuboids and custom meshes.
    Triangles,
    /// Physical pixels in the color/depth target pair.
    TargetPixels,
}

/// One validated current-value cuboid, retaining its managed source identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedCuboid3d {
    source: LogicEntity,
    visual: CuboidVisual3d,
}

impl ResolvedCuboid3d {
    /// Returns the managed entity that produced this cuboid.
    pub const fn source(self) -> LogicEntity {
        self.source
    }

    /// Returns the complete validated presentation value.
    pub const fn visual(self) -> CuboidVisual3d {
        self.visual
    }

    /// Returns the sampled world transform, with no interpolation.
    pub const fn transform(self) -> Transform3d {
        self.visual.transform()
    }

    /// Returns the normalized opaque surface color.
    pub const fn color(self) -> Color {
        self.visual.color()
    }
}

/// Borrowed complete enabled 3D view from a published CPU frame.
///
/// A view may be enabled with no objects, in which case its background still
/// renders. Missing or disabled views have no snapshot and require no prepass.
#[derive(Debug, Clone, Copy)]
pub struct ThreeDSnapshot<'a> {
    view: View3d,
    cuboids: &'a [ResolvedCuboid3d],
    meshes: &'a [ResolvedMesh3d],
}

impl<'a> ThreeDSnapshot<'a> {
    /// Returns the enabled camera, environment and normalized RGBA clear descriptor.
    pub const fn view(self) -> View3d {
        self.view
    }

    /// Returns visible cuboids in stable managed-identity extraction order.
    pub const fn cuboids(self) -> &'a [ResolvedCuboid3d] {
        self.cuboids
    }

    /// Returns surface meshes in stable managed-identity extraction order.
    pub const fn meshes(self) -> &'a [ResolvedMesh3d] {
        self.meshes
    }

    /// Returns original surface triangles before Engine's view-dependent clipping.
    pub fn source_triangle_count(self) -> usize {
        self.meshes
            .iter()
            .fold(self.cuboids.len().saturating_mul(12), |sum, mesh| {
                sum.saturating_add(mesh.visual().asset().mesh().triangle_count())
            })
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct CuboidSource {
    entity: LogicEntity,
    visual: CuboidVisual3d,
}

impl CuboidSource {
    pub(crate) const fn new(entity: LogicEntity, visual: &CuboidVisual3d) -> Self {
        Self {
            entity,
            visual: *visual,
        }
    }
}

pub(crate) struct ThreeDExtractionBuffer {
    view: Option<View3d>,
    resolved: Vec<ResolvedCuboid3d>,
    pub(super) meshes: Vec<ResolvedMesh3d>,
}

impl ThreeDExtractionBuffer {
    pub(crate) const fn new() -> Self {
        Self {
            view: None,
            resolved: Vec::new(),
            meshes: Vec::new(),
        }
    }

    pub(crate) fn clear(&mut self) {
        self.view = None;
        self.resolved.clear();
        self.meshes.clear();
    }

    pub(crate) fn extract(
        &mut self,
        generation: WorldGeneration,
        limits: ThreeDRenderLimits,
        view: Option<View3d>,
        sources: impl IntoIterator<Item = CuboidSource>,
    ) -> Result<(), ThreeDExtractionError> {
        self.clear();
        let Some(view) = view.filter(|view| view.enabled()) else {
            return Ok(());
        };
        view.validate().map_err(ThreeDExtractionError::View)?;
        for source in sources {
            if !source.visual.visible() {
                continue;
            }
            if source.entity.world_generation() != generation {
                return Err(ThreeDExtractionError::StaleEntity {
                    source: source.entity,
                });
            }
            let count = self.resolved.len().saturating_add(1);
            check_limit(ThreeDLimitResource::Cuboids, count, limits.max_cuboids)?;
            let triangles = count
                .checked_mul(12)
                .ok_or(ThreeDExtractionError::LimitExceeded {
                    resource: ThreeDLimitResource::Triangles,
                    requested: u64::MAX,
                    limit: limits.max_triangles as u64,
                })?;
            check_limit(
                ThreeDLimitResource::Triangles,
                triangles,
                limits.max_triangles,
            )?;
            source
                .visual
                .validate()
                .map_err(|error| ThreeDExtractionError::Cuboid {
                    source: source.entity,
                    error,
                })?;
            if self.resolved.len() == self.resolved.capacity() {
                self.resolved.try_reserve_exact(1).map_err(|_| {
                    ThreeDExtractionError::AllocationFailed {
                        requested_bytes: size_of::<ResolvedCuboid3d>(),
                    }
                })?;
            }
            self.resolved.push(ResolvedCuboid3d {
                source: source.entity,
                visual: source.visual,
            });
        }
        self.resolved
            .sort_unstable_by_key(|record| record.source.stable_bits());
        self.view = Some(view);
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> Option<ThreeDSnapshot<'_>> {
        self.view.map(|view| ThreeDSnapshot {
            view,
            cuboids: &self.resolved,
            meshes: &self.meshes,
        })
    }

    pub(crate) fn resolved(&self) -> &[ResolvedCuboid3d] {
        &self.resolved
    }

    pub(crate) fn extract_meshes(
        &mut self,
        generation: WorldGeneration,
        limits: ThreeDRenderLimits,
        sources: impl IntoIterator<Item = MeshSource>,
    ) -> Result<(), ThreeDExtractionError> {
        if self.view.is_none() {
            return Ok(());
        }
        if let Err(error) = super::mesh_extraction::extract_meshes(
            &mut self.meshes,
            generation,
            limits,
            self.resolved.len().saturating_mul(12),
            sources,
        ) {
            self.view = None;
            return Err(error);
        }
        Ok(())
    }
}

pub(super) fn check_limit(
    resource: ThreeDLimitResource,
    requested: usize,
    limit: usize,
) -> Result<(), ThreeDExtractionError> {
    if requested > limit {
        return Err(ThreeDExtractionError::LimitExceeded {
            resource,
            requested: requested as u64,
            limit: limit as u64,
        });
    }
    Ok(())
}

/// A complete 3D CPU snapshot could not be produced within its frozen contract.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ThreeDExtractionError {
    /// A visible cuboid belonged to a different World or application.
    StaleEntity {
        /// Rejected managed source identity.
        source: LogicEntity,
    },
    /// A configured independent 3D work cap was exceeded.
    LimitExceeded {
        /// Resource that would exceed its cap.
        resource: ThreeDLimitResource,
        /// Work requested by this snapshot.
        requested: u64,
        /// Frozen application cap.
        limit: u64,
    },
    /// The camera or opaque clear descriptor was invalid.
    View(View3dError),
    /// A visible cuboid could not be represented by portable CPU geometry.
    Cuboid {
        /// Managed source whose geometry failed validation.
        source: LogicEntity,
        /// Concrete value failure.
        error: CuboidVisualError,
    },
    /// Bounded CPU metadata storage could not be reserved.
    AllocationFailed {
        /// Minimum additional requested allocation bytes.
        requested_bytes: usize,
    },
}

impl fmt::Display for ThreeDExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleEntity { source } => {
                write!(formatter, "3D source {source:?} is from a stale World")
            }
            Self::LimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                formatter,
                "3D {resource:?} requested {requested}, exceeding {limit}"
            ),
            Self::View(error) => write!(formatter, "3D view extraction failed: {error}"),
            Self::Cuboid { source, error } => {
                write!(formatter, "3D cuboid {source:?} extraction failed: {error}")
            }
            Self::AllocationFailed { requested_bytes } => write!(
                formatter,
                "3D extraction could not reserve {requested_bytes} additional bytes"
            ),
        }
    }
}

impl Error for ThreeDExtractionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::View(error) => Some(error),
            Self::Cuboid { error, .. } => Some(error),
            _ => None,
        }
    }
}

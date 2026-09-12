//! Opt-in, bounded opaque cuboids and host-built surface meshes.
//!
//! These presentation values require no window or GPU. They are sampled
//! directly during extraction, without fixed-step interpolation. The desktop
//! bridge retains immutable topology and composes a real depth-buffered 3D target
//! above the 2D world and below every screen overlay. Sim;Engine 0.3 clips filled
//! triangles against the view frustum within explicit topology budgets. The
//! desktop bridge attributes object-specific failures to the source entity.

mod extraction;
mod mesh;
mod mesh_extraction;
mod view;
mod visual;

pub(crate) use extraction::{CuboidSource, ThreeDExtractionBuffer};
pub use extraction::{
    ResolvedCuboid3d, ThreeDExtractionError, ThreeDLimitResource, ThreeDRenderLimits,
    ThreeDSnapshot,
};
pub use mesh::{MeshAsset3d, MeshVisual3d, MeshVisualError};
pub(crate) use mesh_extraction::MeshSource;
pub use mesh_extraction::ResolvedMesh3d;
pub use view::{View3d, View3dError};
pub use visual::{CuboidVisual3d, CuboidVisualError};

// One shared topology for CPU geometry validation and the retained GPU mesh.
pub(crate) const CORNERS: [[f32; 3]; 8] = [
    [-0.5, -0.5, -0.5],
    [0.5, -0.5, -0.5],
    [0.5, 0.5, -0.5],
    [-0.5, 0.5, -0.5],
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

pub(crate) const TRIANGLES: [u32; 36] = [
    0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2,
    6, 1, 6, 5,
];

#[cfg(test)]
mod tests;

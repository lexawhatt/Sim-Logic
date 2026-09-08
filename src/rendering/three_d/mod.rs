//! Opt-in, bounded opaque cuboids with real depth-buffered desktop rendering.
//!
//! These presentation values require no window or GPU. They are sampled
//! directly during extraction, without fixed-step interpolation. The desktop
//! bridge uses one immutable unit-cube mesh and composes an opaque 3D target
//! above the 2D world and below every screen overlay. Sim;Engine 0.2 requires
//! filled triangles to remain wholly inside or wholly outside its frustum;
//! applications should keep visible solid cuboids comfortably inside the view.

mod extraction;
mod view;
mod visual;

pub(crate) use extraction::{CuboidSource, ThreeDExtractionBuffer};
pub use extraction::{
    ResolvedCuboid3d, ThreeDExtractionError, ThreeDLimitResource, ThreeDRenderLimits,
    ThreeDSnapshot,
};
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

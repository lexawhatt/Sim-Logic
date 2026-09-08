use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;
#[cfg(feature = "desktop")]
use sim_engine::MeshStyle3d;
use sim_engine::{
    Color, Mesh3dStyleError, Pseudo3dError, Rotation3d, SurfaceStyle3d, Transform3d, Vec3,
    WireframeStyle3d,
};

use super::{CORNERS, TRIANGLES};

/// One opaque, independently transformed solid cuboid in a managed World.
///
/// Size is the full extent along local axes, before rotation. There is no
/// physics, lighting, interpolation, mesh import, or screen-layer sorting.
/// Hardware depth resolves overlap; stable managed identity resolves submission
/// order. FrameUpdate may change this presentation component. Hidden cuboids
/// and disabled entities consume no extracted 3D object or triangle budget.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct CuboidVisual3d {
    transform: Transform3d,
    color: Color,
    wireframe: Option<WireframeStyle3d>,
    visible: bool,
}

impl CuboidVisual3d {
    /// Creates an axis-aligned cuboid with finite, representable solid geometry.
    ///
    /// Every size component must be positive; color must be normalized and
    /// fully opaque. Invalid values allocate no renderer resources.
    pub fn new(center: Vec3, size: Vec3, color: Color) -> Result<Self, CuboidVisualError> {
        let visual = Self {
            transform: Transform3d::new(center, Rotation3d::IDENTITY, size)?,
            color,
            wireframe: None,
            visible: true,
        };
        visual.validate()?;
        Ok(visual)
    }

    /// Returns the unit cube's world transform.
    pub const fn transform(self) -> Transform3d {
        self.transform
    }

    /// Returns the normalized opaque surface color.
    pub const fn color(self) -> Color {
        self.color
    }

    /// Returns the optional retained outer-edge style.
    pub const fn wireframe(self) -> Option<WireframeStyle3d> {
        self.wireframe
    }

    /// Reports whether this cuboid participates in enabled 3D extraction.
    pub const fn visible(self) -> bool {
        self.visible
    }

    /// Replaces translation, rotation, and positive scale atomically.
    ///
    /// Overflow and floating-point collapse of any filled triangle leave the
    /// previous transform unchanged. Frustum portability is checked by Engine
    /// at submission, not by this viewport-independent setter.
    pub fn set_transform(&mut self, transform: Transform3d) -> Result<(), CuboidVisualError> {
        let proposed = Self { transform, ..*self };
        proposed.validate()?;
        *self = proposed;
        Ok(())
    }

    /// Replaces the normalized opaque surface color, or changes nothing.
    pub fn set_color(&mut self, color: Color) -> Result<(), CuboidVisualError> {
        SurfaceStyle3d::opaque(color)?;
        self.color = color;
        Ok(())
    }

    /// Selects an already validated wireframe style for the twelve outer edges.
    ///
    /// Wireframe adds bounded edge draws, not additional surface triangles.
    pub fn set_wireframe(&mut self, wireframe: Option<WireframeStyle3d>) {
        self.wireframe = wireframe;
    }

    /// Changes extraction visibility without changing geometry or style.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Returns all eight world-space corners for bounds and camera-fit checks.
    pub fn corners(self) -> Result<[Vec3; 8], CuboidVisualError> {
        let mut corners = [Vec3::ZERO; 8];
        for (corner, [x, y, z]) in corners.iter_mut().zip(CORNERS) {
            *corner = self.transform.transform_point(Vec3::new(x, y, z)?)?;
        }
        Ok(corners)
    }

    pub(crate) fn validate(self) -> Result<(), CuboidVisualError> {
        SurfaceStyle3d::opaque(self.color)?;
        let corners = self.corners()?;
        for triangle in TRIANGLES.chunks_exact(3) {
            let a = coordinates(corners[triangle[0] as usize]);
            let b = coordinates(corners[triangle[1] as usize]);
            let c = coordinates(corners[triangle[2] as usize]);
            let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let cross = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            if cross == [0.0; 3] {
                return Err(CuboidVisualError::CollapsedGeometry);
            }
        }
        Ok(())
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn style(self) -> Result<MeshStyle3d, CuboidVisualError> {
        let style = MeshStyle3d::surface(SurfaceStyle3d::opaque(self.color)?);
        Ok(match self.wireframe {
            Some(wireframe) => style.with_wireframe(wireframe),
            None => style,
        })
    }
}

fn coordinates(point: Vec3) -> [f64; 3] {
    [
        f64::from(point.x()),
        f64::from(point.y()),
        f64::from(point.z()),
    ]
}

/// A cuboid value could not represent supported solid opaque geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CuboidVisualError {
    /// Invalid or overflowing 3D arithmetic.
    Geometry(Pseudo3dError),
    /// Surface color was not normalized and opaque.
    Style(Mesh3dStyleError),
    /// Transformed floating-point corners collapsed a filled triangle.
    CollapsedGeometry,
}

impl From<Pseudo3dError> for CuboidVisualError {
    fn from(error: Pseudo3dError) -> Self {
        Self::Geometry(error)
    }
}

impl From<Mesh3dStyleError> for CuboidVisualError {
    fn from(error: Mesh3dStyleError) -> Self {
        Self::Style(error)
    }
}

impl fmt::Display for CuboidVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => write!(formatter, "invalid cuboid geometry: {error}"),
            Self::Style(error) => write!(formatter, "invalid cuboid style: {error}"),
            Self::CollapsedGeometry => formatter.write_str("cuboid geometry collapses in f32"),
        }
    }
}

impl Error for CuboidVisualError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::Style(error) => Some(error),
            Self::CollapsedGeometry => None,
        }
    }
}

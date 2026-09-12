//! Immutable host-built topology, without GPU ownership or a second renderer.

use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;
#[cfg(feature = "desktop")]
use sim_engine::MeshStyle3d;
use sim_engine::{
    Color, Mesh3d, Mesh3dStyleError, Pseudo3dError, SurfaceAlphaMode3d, SurfaceLighting3d,
    SurfaceStyle3d, Transform3d,
};

use super::TextureVisual3d;

/// Shared immutable surface topology built by the application.
///
/// Cloning shares Engine's CPU allocation in constant time. Construct a new
/// asset after editing geometry; unchanged clones reuse one retained desktop
/// mesh. Asset identity is allocation identity, not content equality. The host
/// owns mesh generation, including chunk boundaries and hidden-face removal.
#[derive(Debug, Clone)]
pub struct MeshAsset3d {
    mesh: Mesh3d,
}

impl MeshAsset3d {
    /// Accepts validated Engine topology without allocating GPU resources.
    ///
    /// UVs, colors and normals retain Engine's validated attribute contracts.
    /// Edge-only meshes are not supported. Display edges may be present but are
    /// not drawn. Caller-owned
    /// assets are not application-budgeted until visible extraction uses them.
    pub fn new(mesh: Mesh3d) -> Result<Self, MeshVisualError> {
        if mesh.triangle_count() == 0 {
            return Err(MeshVisualError::UnsupportedTopology);
        }
        Ok(Self { mesh })
    }

    /// Returns immutable Engine topology in caller-defined model-space units.
    pub fn mesh(&self) -> &Mesh3d {
        &self.mesh
    }

    /// Returns retained CPU topology capacities, using Engine's accounting.
    pub fn source_bytes(&self) -> usize {
        self.mesh.recovery_memory_bytes()
    }

    /// Returns whether both assets share the same immutable source allocation.
    pub fn shares_storage(&self, other: &Self) -> bool {
        self.key() == other.key()
    }

    pub(crate) fn key(&self) -> usize {
        // Engine meshes have a nonempty immutable vertex allocation. Keep an
        // owning mesh alongside every cached key so an address cannot be reused
        // while it still names a live entry. No pointer is dereferenced here.
        self.mesh.vertices().as_ptr() as usize
    }
}

impl PartialEq for MeshAsset3d {
    fn eq(&self, other: &Self) -> bool {
        self.shares_storage(other)
    }
}

impl Eq for MeshAsset3d {}

/// One transformed instance of an immutable host-built surface mesh.
///
/// The constructor and geometry setters validate transformed vertices and
/// triangles before changing state. Extraction then clones only shared handles;
/// it does not rebuild topology or interpolate transforms. Engine remains
/// responsible for view-dependent clipping and GPU validation.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct MeshVisual3d {
    asset: MeshAsset3d,
    transform: Transform3d,
    surface: SurfaceStyle3d,
    texture: Option<TextureVisual3d>,
    visible: bool,
}

impl MeshVisual3d {
    /// Creates an opaque mesh instance after CPU geometry validation.
    pub fn new(
        asset: MeshAsset3d,
        transform: Transform3d,
        color: Color,
    ) -> Result<Self, MeshVisualError> {
        Self::with_surface(asset, transform, SurfaceStyle3d::opaque(color)?)
    }

    /// Creates an Opaque, Mask or Blend mesh with explicit Engine surface policy.
    /// Lambert requires host-supplied normals. View-dependent arithmetic remains
    /// Engine's responsibility; this constructor does not duplicate GPU proofs.
    pub fn with_surface(
        asset: MeshAsset3d,
        transform: Transform3d,
        surface: SurfaceStyle3d,
    ) -> Result<Self, MeshVisualError> {
        validate_transform(&asset, transform)?;
        validate_attributes(&asset, surface, None)?;
        Ok(Self {
            asset,
            transform,
            surface,
            texture: None,
            visible: true,
        })
    }

    /// Returns the shared immutable source topology.
    pub const fn asset(&self) -> &MeshAsset3d {
        &self.asset
    }

    /// Returns the current model-to-world transform.
    pub const fn transform(&self) -> Transform3d {
        self.transform
    }

    /// Returns normalized straight-linear surface tint, including surface alpha.
    pub const fn color(&self) -> Color {
        self.surface.color()
    }

    /// Returns alpha, sidedness, lighting and fog policy plus surface tint.
    pub const fn surface(&self) -> SurfaceStyle3d {
        self.surface
    }

    /// Returns optional immutable texture pixels and independent sampling state.
    pub const fn texture(&self) -> Option<&TextureVisual3d> {
        self.texture.as_ref()
    }

    /// Reports whether an enabled entity participates in 3D extraction.
    pub const fn visible(&self) -> bool {
        self.visible
    }

    /// Replaces topology atomically; invalid transformed geometry changes nothing.
    pub fn set_asset(&mut self, asset: MeshAsset3d) -> Result<(), MeshVisualError> {
        validate_transform(&asset, self.transform)?;
        validate_attributes(&asset, self.surface, self.texture.as_ref())?;
        self.asset = asset;
        Ok(())
    }

    /// Replaces the model-to-world transform, or preserves the previous value.
    pub fn set_transform(&mut self, transform: Transform3d) -> Result<(), MeshVisualError> {
        validate_transform(&self.asset, transform)?;
        self.transform = transform;
        Ok(())
    }

    /// Replaces normalized surface tint while preserving alpha/lighting/fog policy.
    /// Opaque surfaces still reject alpha other than one. Failure changes nothing.
    pub fn set_color(&mut self, color: Color) -> Result<(), MeshVisualError> {
        let surface = match self.surface.alpha_mode() {
            SurfaceAlphaMode3d::Opaque => SurfaceStyle3d::opaque(color)?,
            SurfaceAlphaMode3d::Mask => SurfaceStyle3d::mask(
                color,
                self.surface
                    .mask_cutoff()
                    .ok_or(MeshVisualError::UnsupportedSurface)?,
            )?,
            SurfaceAlphaMode3d::Blend => SurfaceStyle3d::blend(color)?,
            _ => return Err(MeshVisualError::UnsupportedSurface),
        }
        .with_sidedness(self.surface.sidedness())
        .with_lighting(self.surface.lighting())
        .with_fog(self.surface.fog_enabled());
        self.set_surface(surface)
    }

    /// Replaces the complete surface policy, preserving geometry and texture.
    /// Enabling Lambert without model normals is rejected atomically.
    pub fn set_surface(&mut self, surface: SurfaceStyle3d) -> Result<(), MeshVisualError> {
        validate_attributes(&self.asset, surface, self.texture.as_ref())?;
        self.surface = surface;
        Ok(())
    }

    /// Attaches or removes a texture. Attaching requires one UV per mesh vertex.
    /// Pixel snapshots are shared; no upload or image copying occurs here.
    pub fn set_texture(&mut self, texture: Option<TextureVisual3d>) -> Result<(), MeshVisualError> {
        validate_attributes(&self.asset, self.surface, texture.as_ref())?;
        self.texture = texture;
        Ok(())
    }

    /// Changes visibility without discarding the host's CPU asset.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn style(&self) -> Result<MeshStyle3d, MeshVisualError> {
        Ok(MeshStyle3d::surface(self.surface))
    }
}

fn validate_attributes(
    asset: &MeshAsset3d,
    surface: SurfaceStyle3d,
    texture: Option<&TextureVisual3d>,
) -> Result<(), MeshVisualError> {
    if surface.lighting() == SurfaceLighting3d::Lambert && asset.mesh.normals().is_empty() {
        return Err(MeshVisualError::MissingNormals);
    }
    if texture.is_some() && asset.mesh.texture_coordinates().is_empty() {
        return Err(MeshVisualError::MissingTextureCoordinates);
    }
    Ok(())
}

fn validate_transform(asset: &MeshAsset3d, transform: Transform3d) -> Result<(), MeshVisualError> {
    // Validate even unused vertices: Engine uploads the entire source array.
    for vertex in asset.mesh.vertices() {
        transform.transform_point(*vertex)?;
    }
    for indices in asset.mesh.triangle_indices().chunks_exact(3) {
        let mut points = [[0.0_f64; 3]; 3];
        for (point, index) in points.iter_mut().zip(indices) {
            let vertex = transform.transform_point(asset.mesh.vertices()[*index as usize])?;
            *point = [
                f64::from(vertex.x()),
                f64::from(vertex.y()),
                f64::from(vertex.z()),
            ];
        }
        let [a, b, c] = points;
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        if [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ] == [0.0; 3]
        {
            return Err(MeshVisualError::CollapsedGeometry);
        }
    }
    Ok(())
}

/// A managed mesh could not represent its requested presentation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MeshVisualError {
    /// Topology has no filled surface triangles.
    UnsupportedTopology,
    /// Invalid or overflowing model-to-world arithmetic.
    Geometry(Pseudo3dError),
    /// Surface color or alpha settings were invalid for their selected policy.
    Style(Mesh3dStyleError),
    /// A transformed filled triangle collapsed in floating-point coordinates.
    CollapsedGeometry,
    /// Lambert shading requires host-supplied model normals.
    MissingNormals,
    /// A texture requires host-supplied UVs on the mesh.
    MissingTextureCoordinates,
    /// An Engine surface mode is not understood by this Logic integration.
    UnsupportedSurface,
}

impl From<Pseudo3dError> for MeshVisualError {
    fn from(error: Pseudo3dError) -> Self {
        Self::Geometry(error)
    }
}

impl From<Mesh3dStyleError> for MeshVisualError {
    fn from(error: Mesh3dStyleError) -> Self {
        Self::Style(error)
    }
}

impl fmt::Display for MeshVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedTopology => {
                formatter.write_str("managed meshes require filled surface topology")
            }
            Self::Geometry(error) => write!(formatter, "invalid mesh transform: {error}"),
            Self::Style(error) => write!(formatter, "invalid mesh style: {error}"),
            Self::CollapsedGeometry => {
                formatter.write_str("transformed mesh triangle collapses in f32")
            }
            Self::MissingNormals => {
                formatter.write_str("Lambert mesh surface requires model normals")
            }
            Self::MissingTextureCoordinates => {
                formatter.write_str("textured mesh surface requires UV coordinates")
            }
            Self::UnsupportedSurface => formatter.write_str("unsupported Engine surface policy"),
        }
    }
}

impl Error for MeshVisualError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::Style(error) => Some(error),
            _ => None,
        }
    }
}

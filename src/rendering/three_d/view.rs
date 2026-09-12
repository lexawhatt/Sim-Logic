use std::{error::Error, fmt};

use bevy_ecs::prelude::Resource;
use sim_engine::{
    Camera3d, Color, Fog3d, Lighting3d, LogicalViewport, Mesh3dStyleError, Projection3d,
    Pseudo3dError, SurfaceStyle3d, UnitError, Vec3, WorldLength,
};

/// Filled-surface projection policy, available without a GPU dependency.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ThreeDSurfacePolicy {
    /// Require Engine's conservative cross-backend clipping/orientation proofs.
    #[default]
    StrictPortable,
    /// Use hardware clipping for filled surfaces, retaining finite-arithmetic checks.
    /// Mathematical display edges retain their independent strict validation.
    Native,
}

/// Optional World resource enabling a 3D view beneath screen overlays.
///
/// The positive-y up axis is fixed; the perspective aspect follows the current
/// logical viewport. Missing or disabled views publish no 3D content, even if
/// the World retains visible cuboids. Switching views does not change gameplay
/// or playback state. FrameUpdate may update this presentation resource.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct View3d {
    position: Vec3,
    target: Vec3,
    vertical_fov_radians: f32,
    near: WorldLength,
    far: WorldLength,
    background: Color,
    orthographic_span: Option<WorldLength>,
    surface_policy: ThreeDSurfacePolicy,
    lighting: Lighting3d,
    fog: Option<Fog3d>,
    enabled: bool,
}

impl View3d {
    /// Creates an enabled view with 60-degree vertical FOV and a 0.1..1000 range.
    ///
    /// Position and target must form a valid camera with positive-y up.
    pub fn new(position: Vec3, target: Vec3) -> Result<Self, View3dError> {
        let view = Self {
            position,
            target,
            vertical_fov_radians: std::f32::consts::FRAC_PI_3,
            near: WorldLength::new(0.1)?,
            far: WorldLength::new(1000.0)?,
            background: Color::BLACK,
            orthographic_span: None,
            surface_policy: ThreeDSurfacePolicy::StrictPortable,
            lighting: Lighting3d::default(),
            fog: None,
            enabled: true,
        };
        view.validate()?;
        Ok(view)
    }

    /// Returns the world-space eye position.
    pub const fn position(self) -> Vec3 {
        self.position
    }

    /// Returns the world-space look-at point.
    pub const fn target(self) -> Vec3 {
        self.target
    }

    /// Returns the last configured perspective field of view in radians.
    /// Orthographic views retain this value but do not use it.
    pub const fn vertical_fov_radians(self) -> f32 {
        self.vertical_fov_radians
    }

    /// Returns the positive near clipping distance.
    pub const fn near(self) -> WorldLength {
        self.near
    }

    /// Returns the far clipping distance, greater than the near distance.
    pub const fn far(self) -> WorldLength {
        self.far
    }

    /// Returns the normalized straight-linear RGBA clear color.
    /// Engine performs the target's premultiplied-alpha conversion.
    pub const fn background(self) -> Color {
        self.background
    }

    /// Reports whether the World participates in 3D extraction and rendering.
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    /// Returns the orthographic vertical world span, or None for perspective.
    pub const fn orthographic_span(self) -> Option<WorldLength> {
        self.orthographic_span
    }

    /// Returns the filled-surface projection policy; strict by default.
    pub const fn surface_policy(self) -> ThreeDSurfacePolicy {
        self.surface_policy
    }

    /// Selects filled-surface validation without changing camera or world state.
    pub fn set_surface_policy(&mut self, policy: ThreeDSurfacePolicy) {
        self.surface_policy = policy;
    }

    /// Returns the ambient and optional directional illumination descriptor.
    pub const fn lighting(self) -> Lighting3d {
        self.lighting
    }

    /// Replaces validated lighting; only Lambert surfaces use it.
    pub fn set_lighting(&mut self, lighting: Lighting3d) {
        self.lighting = lighting;
    }

    /// Returns optional distance fog, disabled by default.
    pub const fn fog(self) -> Option<Fog3d> {
        self.fog
    }

    /// Replaces validated fog; each surface must independently opt in.
    /// Fog never changes logical visibility, alpha or triangle budgets.
    pub fn set_fog(&mut self, fog: Option<Fog3d>) {
        self.fog = fog;
    }

    /// Replaces the eye and look-at point atomically after basis validation.
    pub fn set_pose(&mut self, position: Vec3, target: Vec3) -> Result<(), View3dError> {
        let proposed = Self {
            position,
            target,
            ..*self
        };
        proposed.validate()?;
        *self = proposed;
        Ok(())
    }

    /// Replaces perspective parameters atomically, preserving automatic aspect.
    pub fn set_perspective(
        &mut self,
        vertical_fov_radians: f32,
        near: WorldLength,
        far: WorldLength,
    ) -> Result<(), View3dError> {
        let proposed = Self {
            vertical_fov_radians,
            near,
            far,
            orthographic_span: None,
            ..*self
        };
        proposed.validate()?;
        *self = proposed;
        Ok(())
    }

    /// Selects orthographic projection with a vertical world span and automatic aspect.
    /// Invalid camera arithmetic leaves the entire previous view unchanged.
    pub fn set_orthographic(
        &mut self,
        vertical_span: WorldLength,
        near: WorldLength,
        far: WorldLength,
    ) -> Result<(), View3dError> {
        let proposed = Self {
            orthographic_span: Some(vertical_span),
            near,
            far,
            ..*self
        };
        proposed.validate()?;
        *self = proposed;
        Ok(())
    }

    /// Replaces the normalized straight-linear RGBA clear color, or changes nothing.
    pub fn set_background(&mut self, background: Color) -> Result<(), View3dError> {
        SurfaceStyle3d::blend(background)?;
        self.background = background;
        Ok(())
    }

    /// Enables or suppresses the complete 3D snapshot without removing cuboids.
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Builds a CPU camera for the current logical viewport without allocation.
    ///
    /// An extreme aspect ratio can be unrepresentable and returns a typed error.
    pub fn camera(self, viewport: LogicalViewport) -> Result<Camera3d, View3dError> {
        self.camera_with_aspect(viewport.width() / viewport.height())
    }

    pub(crate) fn validate(self) -> Result<(), View3dError> {
        SurfaceStyle3d::blend(self.background)?;
        self.camera_with_aspect(1.0)?;
        Ok(())
    }

    fn camera_with_aspect(self, aspect: f32) -> Result<Camera3d, View3dError> {
        let projection = match self.orthographic_span {
            Some(span) => Projection3d::orthographic(span, aspect, self.near, self.far)?,
            None => {
                Projection3d::perspective(self.vertical_fov_radians, aspect, self.near, self.far)?
            }
        };
        Ok(Camera3d::look_at(
            self.position,
            self.target,
            Vec3::Y,
            projection,
        )?)
    }
}

/// Camera or clear-color validation failed without changing the previous view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View3dError {
    /// Invalid camera basis, projection, or overflowing camera arithmetic.
    Geometry(Pseudo3dError),
    /// Invalid typed projection distance.
    Units(UnitError),
    /// The target clear color was not normalized straight-linear RGBA.
    Background(Mesh3dStyleError),
}

impl From<Pseudo3dError> for View3dError {
    fn from(error: Pseudo3dError) -> Self {
        Self::Geometry(error)
    }
}

impl From<UnitError> for View3dError {
    fn from(error: UnitError) -> Self {
        Self::Units(error)
    }
}

impl From<Mesh3dStyleError> for View3dError {
    fn from(error: Mesh3dStyleError) -> Self {
        Self::Background(error)
    }
}

impl fmt::Display for View3dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => write!(formatter, "invalid 3D view: {error}"),
            Self::Units(error) => write!(formatter, "invalid 3D view units: {error}"),
            Self::Background(error) => write!(formatter, "invalid 3D background: {error}"),
        }
    }
}

impl Error for View3dError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::Units(error) => Some(error),
            Self::Background(error) => Some(error),
        }
    }
}

//! Event-ordered desktop geometry, independent from live window queries.

use std::{error::Error, fmt};

use sim_engine::{
    LogicalScreenPosition, LogicalViewport, LogicalViewportError, RendererPresentMode,
    WgpuRendererOptions,
};
use winit::{
    dpi::{PhysicalPosition, PhysicalSize},
    event::MouseButton as PlatformMouseButton,
};

use crate::input::{InputEvent, MouseButton, PointerSample};

/// Invalid desktop geometry encountered before collecting a pointer sample.
///
/// The adapter reports this through [`super::DesktopRunError::Pointer`] and
/// stops before the invalid geometry reaches a logical frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DesktopPointerError {
    /// The display scale cannot support Sim;Engine's logical pixel conversion.
    InvalidScaleFactor {
        /// Rejected physical pixels per logical pixel.
        scale_factor: f64,
    },
    /// A physical cursor position was non-finite or its logical coordinates
    /// could not be represented as finite `f32` values.
    InvalidPosition {
        /// Rejected horizontal coordinate in physical pixels.
        x: f64,
        /// Rejected vertical coordinate in physical pixels.
        y: f64,
        /// Event-time physical pixels per logical pixel.
        scale_factor: f64,
    },
    /// A nonzero physical extent could not form positive finite logical dimensions.
    LogicalViewport(LogicalViewportError),
}

impl fmt::Display for DesktopPointerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScaleFactor { scale_factor } => {
                write!(formatter, "invalid pointer display scale {scale_factor}")
            }
            Self::InvalidPosition { x, y, scale_factor } => write!(
                formatter,
                "physical pointer ({x}, {y}) cannot form finite logical coordinates at scale {scale_factor}"
            ),
            Self::LogicalViewport(error) => write!(formatter, "pointer viewport failed: {error}"),
        }
    }
}

impl Error for DesktopPointerError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::LogicalViewport(error) => Some(error),
            Self::InvalidScaleFactor { .. } | Self::InvalidPosition { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct DesktopPointerGeometry {
    physical_size: PhysicalSize<u32>,
    scale_factor: f64,
    physical_cursor: Option<PhysicalPosition<f64>>,
}

impl DesktopPointerGeometry {
    pub(super) const fn empty() -> Self {
        Self {
            physical_size: PhysicalSize::new(0, 0),
            scale_factor: 1.0,
            physical_cursor: None,
        }
    }

    pub(super) fn new(
        physical_size: PhysicalSize<u32>,
        scale_factor: f64,
    ) -> Result<Self, DesktopPointerError> {
        validate_scale_factor(scale_factor)?;
        let geometry = Self {
            physical_size,
            scale_factor,
            physical_cursor: None,
        };
        geometry.logical_viewport()?;
        Ok(geometry)
    }

    pub(super) const fn physical_size(self) -> PhysicalSize<u32> {
        self.physical_size
    }

    pub(super) const fn scale_factor(self) -> f64 {
        self.scale_factor
    }

    pub(super) fn cursor_moved(
        &mut self,
        position: PhysicalPosition<f64>,
    ) -> Result<InputEvent, DesktopPointerError> {
        let logical_position = self.logical_position(position)?;
        let Some(viewport) = self.logical_viewport()? else {
            return Ok(self.cursor_left());
        };
        let sample = PointerSample::new(logical_position, viewport)
            .map_err(|_| self.position_error(position))?;
        self.physical_cursor = Some(position);
        Ok(InputEvent::pointer_moved(sample))
    }

    pub(super) fn cursor_left(&mut self) -> InputEvent {
        self.physical_cursor = None;
        InputEvent::PointerLeft
    }

    pub(super) fn resized(
        &mut self,
        physical_size: PhysicalSize<u32>,
    ) -> Result<Option<InputEvent>, DesktopPointerError> {
        self.replace_geometry(Self {
            physical_size,
            ..*self
        })
    }

    pub(super) fn scale_factor_changed(
        &mut self,
        scale_factor: f64,
    ) -> Result<Option<InputEvent>, DesktopPointerError> {
        validate_scale_factor(scale_factor)?;
        self.replace_geometry(Self {
            scale_factor,
            ..*self
        })
    }

    fn replace_geometry(
        &mut self,
        mut next: Self,
    ) -> Result<Option<InputEvent>, DesktopPointerError> {
        let event = if next.logical_viewport()?.is_none() {
            Some(next.cursor_left())
        } else if let Some(position) = next.physical_cursor {
            Some(next.cursor_moved(position)?)
        } else {
            None
        };
        // A failed conversion leaves both geometry and the known cursor intact.
        *self = next;
        Ok(event)
    }

    fn logical_viewport(self) -> Result<Option<LogicalViewport>, DesktopPointerError> {
        if super::extent_is_occluded(self.physical_size.width, self.physical_size.height) {
            return Ok(None);
        }
        // Match the renderer's f32 viewport arithmetic while retaining the
        // original physical extent and scale for later delivered events.
        let scale_factor = self.scale_factor as f32;
        LogicalViewport::new(
            self.physical_size.width as f32 / scale_factor,
            self.physical_size.height as f32 / scale_factor,
        )
        .map(Some)
        .map_err(DesktopPointerError::LogicalViewport)
    }

    fn logical_position(
        self,
        position: PhysicalPosition<f64>,
    ) -> Result<LogicalScreenPosition, DesktopPointerError> {
        let logical = LogicalScreenPosition::new(
            (position.x / self.scale_factor) as f32,
            (position.y / self.scale_factor) as f32,
        );
        if !position.x.is_finite() || !position.y.is_finite() || !logical.is_finite() {
            return Err(self.position_error(position));
        }
        Ok(logical)
    }

    const fn position_error(self, position: PhysicalPosition<f64>) -> DesktopPointerError {
        DesktopPointerError::InvalidPosition {
            x: position.x,
            y: position.y,
            scale_factor: self.scale_factor,
        }
    }
}

fn validate_scale_factor(scale_factor: f64) -> Result<(), DesktopPointerError> {
    // Reuse Engine's supported conversion range without creating a renderer.
    WgpuRendererOptions::new(RendererPresentMode::Vsync, scale_factor)
        .map(|_| ())
        .map_err(|_| DesktopPointerError::InvalidScaleFactor { scale_factor })
}

pub(super) const fn map_mouse_button(button: PlatformMouseButton) -> Option<MouseButton> {
    match button {
        PlatformMouseButton::Left => Some(MouseButton::Left),
        PlatformMouseButton::Right => Some(MouseButton::Right),
        PlatformMouseButton::Middle => Some(MouseButton::Middle),
        PlatformMouseButton::Back
        | PlatformMouseButton::Forward
        | PlatformMouseButton::Other(_) => None,
    }
}

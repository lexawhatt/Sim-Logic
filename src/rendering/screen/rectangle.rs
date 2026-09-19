use super::{ScreenClip, ScreenRectangleVisual, ScreenVisualError, validate_color};
use sim_engine::{LogicalPixels, SceneError, ScreenScene, ShapeStyle, Stroke};

impl ScreenRectangleVisual {
    /// Creates a rounded filled rectangle. Radius is in logical pixels and must
    /// be finite and nonnegative. Drawing clamps it to half the shorter side.
    pub fn rounded(
        position: sim_engine::LogicalScreenPosition,
        size: sim_engine::LogicalScreenVector,
        color: sim_engine::Color,
        radius: f32,
    ) -> Result<Self, ScreenVisualError> {
        let mut visual = Self::new(position, size, color)?;
        visual.set_corner_radius(radius)?;
        Ok(visual)
    }

    /// Returns the requested radius, before clamping to the current rectangle.
    pub const fn corner_radius(&self) -> f32 {
        self.corner_radius
    }

    /// Sets a finite nonnegative logical radius atomically. Zero means square corners.
    pub fn set_corner_radius(&mut self, radius: f32) -> Result<(), ScreenVisualError> {
        if !radius.is_finite() || radius < 0.0 {
            return Err(ScreenVisualError::InvalidRadius { value: radius });
        }
        self.corner_radius = radius;
        Ok(())
    }

    /// Returns the optional centered decorative outline in logical pixels.
    pub const fn stroke(&self) -> Option<Stroke> {
        self.stroke
    }

    /// Sets/removes a centered outline. Invalid width/color leaves the visual unchanged.
    /// Pointer hits test the rounded fill, not the outline extending beyond it.
    pub fn set_stroke(&mut self, stroke: Option<Stroke>) -> Result<(), ScreenVisualError> {
        if let Some(stroke) = stroke {
            validate_stroke(stroke)?;
        }
        self.stroke = stroke;
        Ok(())
    }

    /// Returns the explicit screen clip, shared by drawing and pointer geometry.
    pub const fn clip(&self) -> ScreenClip {
        self.clip
    }

    /// Replaces the clip without moving content or changing draw order.
    pub fn set_clip(&mut self, clip: ScreenClip) {
        self.clip = clip;
    }

    pub(crate) fn append(&self, scene: &mut ScreenScene) -> Result<(), SceneError> {
        let clip = match self.clip {
            ScreenClip::Empty => return Ok(()),
            ScreenClip::Unclipped => None,
            ScreenClip::Rectangle(clip) => Some(clip),
        };
        scene.set_screen_clip(clip)?;
        let style = match self.stroke {
            Some(stroke) => ShapeStyle::fill_stroke(self.color, stroke.width(), stroke.color()),
            None => ShapeStyle::filled(self.color),
        };
        if self.corner_radius == 0.0 {
            scene.try_square_rect_on_layer(self.layer, self.position, self.size, style)
        } else {
            let radius = LogicalPixels::new(self.corner_radius)
                .map_err(|_| SceneError::InvalidDimension(sim_engine::ScenePrimitive::Rect))?;
            scene.try_rect_on_layer(self.layer, self.position, self.size, radius, style)
        }
    }
}

pub(super) fn validate_stroke(stroke: Stroke) -> Result<(), ScreenVisualError> {
    if !stroke.width().is_finite() || stroke.width() <= 0.0 {
        return Err(ScreenVisualError::InvalidStrokeWidth {
            value: stroke.width(),
        });
    }
    validate_color(stroke.color())
}

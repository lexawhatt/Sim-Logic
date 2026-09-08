//! Standard visual components for the first Sim;Logic slice.

use bevy_ecs::prelude::{Component, Resource};
use sim_engine::{
    Camera2d, Color, DrawCommand, Interpolate, Layer, StrokeCap2d, StrokeStyle2d, Vec2,
};
use std::{error::Error, fmt};

/// Interpolated world-space translation of a managed visual entity.
///
/// Fixed-update code changes the current translation through
/// [`Transform2d::set_translation`] or [`Transform2d::translate_by`]. The
/// runtime snapshots the previous value at a fixed-tick boundary. Render
/// extraction reads both values but never writes the interpolated result back.
/// FrameUpdate must treat this component as read-only: Sim;Logic rejects
/// mutable component access when the FrameUpdate System is prepared.
/// Standard world-space visual and collider components add a default transform
/// at the world origin when their spawn bundle does not provide one.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct Transform2d {
    previous_translation: Vec2,
    translation: Vec2,
}

impl Transform2d {
    /// Creates a transform snapped to one finite world-space translation.
    pub fn new(translation: Vec2) -> Result<Self, VisualValueError> {
        validate_translation(translation)?;
        Ok(Self {
            previous_translation: translation,
            translation,
        })
    }

    /// Creates a transform snapped to the finite world-space coordinates `(x, y)`.
    ///
    /// This is exactly equivalent to `Transform2d::new(Vec2::new(x, y))`.
    /// Either non-finite coordinate returns
    /// [`VisualValueError::InvalidTranslation`].
    #[inline]
    pub fn from_xy(x: f32, y: f32) -> Result<Self, VisualValueError> {
        Self::new(Vec2::new(x, y))
    }

    /// Returns the current canonical world-space translation.
    pub const fn translation(&self) -> Vec2 {
        self.translation
    }

    /// Returns the previous committed fixed-tick translation.
    pub const fn previous_translation(&self) -> Vec2 {
        self.previous_translation
    }

    /// Replaces the current translation after finite-value validation.
    ///
    /// The previous translation is retained for fixed-step interpolation. Use
    /// [`Transform2d::teleport`] when the change must not be interpolated.
    /// FrameUpdate Systems cannot request mutable access to this component in
    /// the first-slice runner.
    pub fn set_translation(&mut self, translation: Vec2) -> Result<(), VisualValueError> {
        validate_translation(translation)?;
        self.translation = translation;
        Ok(())
    }

    /// Applies a finite world-space offset atomically.
    ///
    /// An invalid offset or arithmetic overflow leaves the transform unchanged.
    /// FrameUpdate Systems cannot request mutable access to this component in
    /// the first-slice runner.
    pub fn translate_by(&mut self, delta: Vec2) -> Result<(), VisualValueError> {
        validate_translation(delta)?;
        let translation = self.translation + delta;
        validate_translation(translation)?;
        self.translation = translation;
        Ok(())
    }

    /// Returns a copy with a finite world-space offset applied atomically.
    ///
    /// The source and the returned previous interpolation endpoint remain
    /// unchanged. Validation and arithmetic are exactly those of
    /// [`Transform2d::translate_by`]. This is useful for checking a proposed
    /// pose before deciding whether to update the live component.
    pub fn translated_by(&self, delta: Vec2) -> Result<Self, VisualValueError> {
        let mut translated = *self;
        translated.translate_by(delta)?;
        Ok(translated)
    }

    /// Replaces both interpolation endpoints with one finite translation.
    /// FrameUpdate Systems cannot request mutable access to this component in
    /// the first-slice runner.
    pub fn teleport(&mut self, translation: Vec2) -> Result<(), VisualValueError> {
        validate_translation(translation)?;
        self.previous_translation = translation;
        self.translation = translation;
        Ok(())
    }

    /// Captures the current translation as the previous fixed-tick endpoint.
    pub(crate) fn begin_fixed_tick(&mut self) {
        self.previous_translation = self.translation;
    }

    /// Removes interpolation history without changing canonical translation.
    pub(crate) fn snap_interpolation(&mut self) {
        self.previous_translation = self.translation;
    }

    pub(crate) fn restore_previous_translation(&mut self, previous: Vec2) {
        self.previous_translation = previous;
    }
}

impl Default for Transform2d {
    fn default() -> Self {
        Self {
            previous_translation: Vec2::ZERO,
            translation: Vec2::ZERO,
        }
    }
}

/// Filled world-space circle rendered by the standard Sim;Engine bridge.
///
/// Radius uses caller-defined world units. Layer controls primary draw order;
/// lower layers are drawn first. Within one layer, lower draw-order depth is
/// drawn first, followed by stable entity identity as the final tie-breaker.
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. The canonical translation of a
/// transform supplied in the same bundle takes precedence; normal spawn
/// snapping may reset its previous interpolation endpoint to that translation.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct CircleVisual {
    radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl CircleVisual {
    /// Creates a filled circle on the default layer and draw-order depth zero.
    pub fn new(radius: f32, color: Color) -> Result<Self, VisualValueError> {
        validate_radius(radius)?;
        validate_color(color)?;
        Ok(Self {
            radius,
            color,
            layer: Layer::DEFAULT,
            draw_order_depth: 0.0,
        })
    }

    /// Returns the radius in caller-defined world units.
    pub const fn radius(&self) -> f32 {
        self.radius
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(&self) -> Color {
        self.color
    }

    /// Returns the Sim;Engine layer used for primary ordering.
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Returns the within-layer draw-order depth.
    ///
    /// This value affects ordering only. It is not Sim;Engine pseudo-depth and
    /// does not change camera projection.
    pub const fn draw_order_depth(&self) -> f32 {
        self.draw_order_depth
    }

    /// Replaces the world-space radius after validation.
    pub fn set_radius(&mut self, radius: f32) -> Result<(), VisualValueError> {
        validate_radius(radius)?;
        self.radius = radius;
        Ok(())
    }

    /// Replaces the fill color after normalized-color validation.
    pub fn set_color(&mut self, color: Color) -> Result<(), VisualValueError> {
        validate_color(color)?;
        self.color = color;
        Ok(())
    }

    /// Replaces the primary draw layer.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Replaces within-layer draw depth after finite-value validation.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), VisualValueError> {
        validate_draw_order_depth(depth)?;
        self.draw_order_depth = depth;
        Ok(())
    }
}

/// Filled axis-aligned world-space rectangle rendered by the standard bridge.
///
/// [`Transform2d::translation`] is the rectangle center and `size` is its full
/// width and height in caller-defined world units. Layer and draw-order depth
/// follow the same ordering rules as [`CircleVisual`]. If both visuals belong
/// to one entity with the same layer and depth, the rectangle is drawn first.
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. The canonical translation of a
/// transform supplied in the same bundle takes precedence; normal spawn
/// snapping may reset its previous interpolation endpoint to that translation.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct RectangleVisual {
    size: Vec2,
    corner_radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl RectangleVisual {
    /// Creates a filled square-cornered rectangle on the default layer at depth zero.
    pub fn new(size: Vec2, color: Color) -> Result<Self, VisualValueError> {
        validate_size(size)?;
        validate_color(color)?;
        Ok(Self {
            size,
            corner_radius: 0.0,
            color,
            layer: Layer::DEFAULT,
            draw_order_depth: 0.0,
        })
    }

    /// Creates a filled rounded rectangle on the default layer at depth zero.
    ///
    /// The requested corner radius must be finite and nonnegative. Values
    /// larger than half the smaller side are retained here and clamped only
    /// when Sim;Engine presents the rectangle. This is exactly equivalent to
    /// [`RectangleVisual::new`] followed by
    /// [`RectangleVisual::set_corner_radius`].
    pub fn rounded(size: Vec2, color: Color, corner_radius: f32) -> Result<Self, VisualValueError> {
        let mut visual = Self::new(size, color)?;
        visual.set_corner_radius(corner_radius)?;
        Ok(visual)
    }

    /// Returns the full width and height in caller-defined world units.
    pub const fn size(&self) -> Vec2 {
        self.size
    }

    /// Returns the requested corner radius in world units.
    ///
    /// A radius larger than half the smaller side is retained here and clamped
    /// only while Sim;Engine presents the rectangle.
    pub const fn corner_radius(&self) -> f32 {
        self.corner_radius
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(&self) -> Color {
        self.color
    }

    /// Returns the Sim;Engine layer used for primary ordering.
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Returns the within-layer draw-order depth.
    ///
    /// This value affects ordering only. It is not Sim;Engine pseudo-depth and
    /// does not change camera projection.
    pub const fn draw_order_depth(&self) -> f32 {
        self.draw_order_depth
    }

    /// Replaces the full width and height after validation.
    pub fn set_size(&mut self, size: Vec2) -> Result<(), VisualValueError> {
        validate_size(size)?;
        self.size = size;
        Ok(())
    }

    /// Replaces the requested corner radius after validation.
    pub fn set_corner_radius(&mut self, radius: f32) -> Result<(), VisualValueError> {
        validate_corner_radius(radius)?;
        self.corner_radius = radius;
        Ok(())
    }

    /// Replaces the fill color after normalized-color validation.
    pub fn set_color(&mut self, color: Color) -> Result<(), VisualValueError> {
        validate_color(color)?;
        self.color = color;
        Ok(())
    }

    /// Replaces the primary draw layer.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Replaces within-layer draw depth after finite-value validation.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), VisualValueError> {
        validate_draw_order_depth(depth)?;
        self.draw_order_depth = depth;
        Ok(())
    }
}

/// A world-axis line segment anchored at an entity's [`Transform2d`].
///
/// The interpolated transform translation is the segment start. `vector` is
/// added in caller-defined world units to obtain its end and is not rotated or
/// scaled. Only the transform translation is interpolated; vector and style
/// changes are sampled at extraction like other visual geometry.
///
/// Stroke width is measured in logical screen pixels and remains readable as
/// the camera zooms. Rendering uses butt caps, so the segment body stops at
/// its mathematical endpoints. A zero vector is valid and emits no resolved
/// record or Scene command.
///
/// Lower layers and then lower draw-order depths are drawn first, followed by
/// stable entity identity. On an otherwise exact tie, rectangles are drawn
/// before circles and lines are drawn last, which keeps a line visible over a
/// body on the same entity. Spawning without a [`Transform2d`] supplies an
/// origin transform. An explicit transform in the same spawn bundle takes
/// precedence; normal spawn snapping may reset its previous interpolation
/// endpoint to its canonical translation.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct LineVisual {
    vector: Vec2,
    stroke_width_logical_pixels: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl LineVisual {
    /// Creates an anchored line on the default layer at draw-order depth zero.
    ///
    /// Validation order is vector, logical-pixel width, then color. Zero is a
    /// valid vector and represents an omitted line.
    pub fn new(
        vector: Vec2,
        stroke_width_logical_pixels: f32,
        color: Color,
    ) -> Result<Self, VisualValueError> {
        validate_line_vector(vector)?;
        validate_line_width(stroke_width_logical_pixels)?;
        validate_color(color)?;
        Ok(Self {
            vector,
            stroke_width_logical_pixels,
            color,
            layer: Layer::DEFAULT,
            draw_order_depth: 0.0,
        })
    }

    /// Returns the world-axis offset from the transform anchor to the endpoint.
    pub const fn vector(&self) -> Vec2 {
        self.vector
    }

    /// Returns stroke width in logical screen pixels.
    pub const fn stroke_width_logical_pixels(&self) -> f32 {
        self.stroke_width_logical_pixels
    }

    /// Returns the normalized straight-linear RGBA stroke color.
    pub const fn color(&self) -> Color {
        self.color
    }

    /// Returns the Sim;Engine layer used for primary ordering.
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Returns the within-layer draw-order value.
    ///
    /// This affects ordering only and is not Sim;Engine projection depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.draw_order_depth
    }

    /// Replaces the world-axis vector after finite-value validation.
    ///
    /// Zero is accepted and omits the line at extraction. A rejected value
    /// leaves the previous vector unchanged.
    pub fn set_vector(&mut self, vector: Vec2) -> Result<(), VisualValueError> {
        validate_line_vector(vector)?;
        self.vector = vector;
        Ok(())
    }

    /// Replaces the logical-pixel stroke width after Sim;Engine validation.
    ///
    /// A rejected value leaves the previous width unchanged.
    pub fn set_stroke_width_logical_pixels(&mut self, width: f32) -> Result<(), VisualValueError> {
        validate_line_width(width)?;
        self.stroke_width_logical_pixels = width;
        Ok(())
    }

    /// Replaces the stroke color after normalized-color validation.
    pub fn set_color(&mut self, color: Color) -> Result<(), VisualValueError> {
        validate_color(color)?;
        self.color = color;
        Ok(())
    }

    /// Replaces the primary draw layer.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Replaces within-layer draw order after finite-value validation.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), VisualValueError> {
        validate_draw_order_depth(depth)?;
        self.draw_order_depth = depth;
        Ok(())
    }
}

/// The only active world camera, with a fixed-step-interpolated center.
///
/// FixedUpdate code changes the canonical center through
/// [`ActiveCamera2d::set_center`] or [`ActiveCamera2d::pan_by`]. The runtime
/// retains the previous center and extraction presents an interpolated copy of
/// the wrapped Sim;Engine camera. Zoom, rotation, and projection remain the
/// static values supplied at construction in this slice.
///
/// FrameUpdate Systems cannot request mutable access to this component.
/// Extraction also fails when a World contains zero or more than one enabled
/// `ActiveCamera2d`. This component intentionally denotes the fixed-owned
/// active camera and does not add a [`Transform2d`]; its wrapped camera center
/// is the sole position source. A future frame-driven camera must use a
/// distinct camera source instead of changing this component's ownership at
/// runtime.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct ActiveCamera2d {
    previous_camera: Camera2d,
    camera: Camera2d,
}

impl ActiveCamera2d {
    /// Marks a validated Sim;Engine camera as active and snaps its history.
    pub const fn new(camera: Camera2d) -> Self {
        Self {
            previous_camera: camera,
            camera,
        }
    }

    /// Creates an active camera centered at the world origin.
    ///
    /// `zoom` is logical screen pixels per world unit and must be finite and
    /// positive. Validation is delegated to [`Camera2d::new`]. Use
    /// [`ActiveCamera2d::new`] when the initial center is not the origin.
    pub fn centered(zoom: f32) -> Result<Self, sim_engine::Camera2dError> {
        Camera2d::new(Vec2::ZERO, zoom).map(Self::new)
    }

    /// Returns the current canonical Sim;Engine camera value.
    ///
    /// During a partial fixed interval this can differ from the presentation
    /// camera returned by [`crate::ExtractedFrame::camera`].
    pub const fn camera(&self) -> Camera2d {
        self.camera
    }

    /// Returns the current canonical world-space center.
    pub fn center(&self) -> Vec2 {
        self.camera.center()
    }

    /// Returns the previous center endpoint retained for interpolation.
    pub fn previous_center(&self) -> Vec2 {
        self.previous_camera.center()
    }

    /// Replaces the canonical center after Sim;Engine validation.
    ///
    /// The previous center is retained for fixed-step interpolation. Use
    /// [`ActiveCamera2d::teleport_center`] when the change must be immediate.
    pub fn set_center(&mut self, center: Vec2) -> Result<(), sim_engine::Camera2dError> {
        self.camera.set_center(center)
    }

    /// Applies a validated world-space offset to the canonical center.
    pub fn pan_by(&mut self, delta: Vec2) -> Result<(), sim_engine::Camera2dError> {
        self.camera.pan_by(delta)
    }

    /// Replaces both center interpolation endpoints with one validated value.
    pub fn teleport_center(&mut self, center: Vec2) -> Result<(), sim_engine::Camera2dError> {
        self.camera.set_center(center)?;
        self.previous_camera.set_center(center)?;
        Ok(())
    }

    pub(crate) fn begin_fixed_tick(&mut self) {
        self.previous_camera = self.camera;
    }

    pub(crate) fn snap_interpolation(&mut self) {
        self.previous_camera = self.camera;
    }

    pub(crate) fn restore_previous_center(&mut self, previous: Vec2) {
        let result = self.previous_camera.set_center(previous);
        debug_assert!(result.is_ok(), "valid camera backup was rejected");
    }

    pub(crate) fn interpolated_camera(&self, alpha: f32) -> Camera2d {
        let center = self
            .previous_camera
            .center()
            .interpolate(self.camera.center(), alpha);
        let mut camera = self.camera;
        // Both endpoints and alpha are validated by their public/runtime
        // contracts. Sim;Engine's interpolation also preserves a finite
        // endpoint defensively, so this setter cannot reject here.
        let result = camera.set_center(center);
        debug_assert!(
            result.is_ok(),
            "validated camera interpolation became invalid"
        );
        camera
    }
}

/// Normalized clear color owned by one logical World.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct WorldBackground(Color);

impl WorldBackground {
    /// Creates a World background after normalized-color validation.
    pub fn new(color: Color) -> Result<Self, VisualValueError> {
        validate_color(color)?;
        Ok(Self(color))
    }

    /// Returns the normalized straight-linear RGBA clear color.
    pub const fn color(self) -> Color {
        self.0
    }
}

impl Default for WorldBackground {
    fn default() -> Self {
        Self(Color::BLACK)
    }
}

/// Invalid value rejected by a standard visual component operation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum VisualValueError {
    /// A world-space translation or offset contained a non-finite component.
    InvalidTranslation {
        /// Rejected translation or offset.
        value: Vec2,
    },
    /// A circle radius was non-finite or not strictly positive.
    InvalidRadius {
        /// Rejected radius in world units.
        value: f32,
    },
    /// A rectangle size had a non-finite or non-positive component.
    InvalidSize {
        /// Rejected full width and height in world units.
        value: Vec2,
    },
    /// A line vector contained a non-finite component.
    InvalidLineVector {
        /// Rejected world-axis vector.
        value: Vec2,
    },
    /// A logical-pixel line width was rejected by Sim;Engine.
    InvalidLineWidth {
        /// Rejected untyped logical-pixel width.
        value: f32,
    },
    /// A rectangle corner radius was non-finite or negative.
    InvalidCornerRadius {
        /// Rejected corner radius in world units.
        value: f32,
    },
    /// A color was non-finite or outside the normalized `0.0..=1.0` range.
    InvalidColor {
        /// Rejected straight-linear RGBA color.
        value: Color,
    },
    /// A draw-order depth was not finite.
    InvalidDrawOrderDepth {
        /// Rejected ordering value.
        value: f32,
    },
}

impl fmt::Display for VisualValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTranslation { value } => {
                write!(formatter, "world translation must be finite, got {value:?}")
            }
            Self::InvalidRadius { value } => {
                write!(
                    formatter,
                    "circle radius must be positive and finite, got {value}"
                )
            }
            Self::InvalidSize { value } => write!(
                formatter,
                "rectangle size components must be positive and finite, got {value:?}"
            ),
            Self::InvalidLineVector { value } => {
                write!(formatter, "line vector must be finite, got {value:?}")
            }
            Self::InvalidLineWidth { value } => write!(
                formatter,
                "line stroke width in logical pixels is invalid, got {value}"
            ),
            Self::InvalidCornerRadius { value } => write!(
                formatter,
                "rectangle corner radius must be nonnegative and finite, got {value}"
            ),
            Self::InvalidColor { value } => {
                write!(
                    formatter,
                    "visual color must be normalized linear RGBA, got {value:?}"
                )
            }
            Self::InvalidDrawOrderDepth { value } => {
                write!(formatter, "draw-order depth must be finite, got {value}")
            }
        }
    }
}

impl Error for VisualValueError {}

pub(crate) fn validate_translation(value: Vec2) -> Result<(), VisualValueError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(VisualValueError::InvalidTranslation { value })
}

pub(crate) fn validate_radius(value: f32) -> Result<(), VisualValueError> {
    (value.is_finite() && value > 0.0)
        .then_some(())
        .ok_or(VisualValueError::InvalidRadius { value })
}

pub(crate) fn validate_size(value: Vec2) -> Result<(), VisualValueError> {
    (value.is_finite() && value.x() > 0.0 && value.y() > 0.0)
        .then_some(())
        .ok_or(VisualValueError::InvalidSize { value })
}

pub(crate) fn validate_line_vector(value: Vec2) -> Result<(), VisualValueError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(VisualValueError::InvalidLineVector { value })
}

pub(crate) fn validate_line_width(value: f32) -> Result<(), VisualValueError> {
    let style = line_stroke_style(value, Color::WHITE);
    DrawCommand::styled_line(Vec2::ZERO, Vec2::new(1.0, 0.0), style)
        .map(|_| ())
        .map_err(|_| VisualValueError::InvalidLineWidth { value })
}

pub(crate) const fn line_stroke_style(width: f32, color: Color) -> StrokeStyle2d {
    StrokeStyle2d::new(width, color).with_cap(StrokeCap2d::Butt)
}

pub(crate) fn validate_corner_radius(value: f32) -> Result<(), VisualValueError> {
    (value.is_finite() && value >= 0.0)
        .then_some(())
        .ok_or(VisualValueError::InvalidCornerRadius { value })
}

pub(crate) fn validate_color(value: Color) -> Result<(), VisualValueError> {
    value
        .is_normalized()
        .then_some(())
        .ok_or(VisualValueError::InvalidColor { value })
}

pub(crate) fn validate_draw_order_depth(value: f32) -> Result<(), VisualValueError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(VisualValueError::InvalidDrawOrderDepth { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_from_xy_exactly_delegates_to_vector_construction() {
        let valid = [
            (-0.0, 0.0),
            (f32::from_bits(1), -f32::from_bits(1)),
            (12.5, -98.25),
            (f32::MAX, -f32::MAX),
        ];
        for (x, y) in valid {
            let direct = Transform2d::new(Vec2::new(x, y)).unwrap();
            let scalar = Transform2d::from_xy(x, y).unwrap();
            assert_eq!(scalar, direct);
            assert_eq!(scalar.translation().x().to_bits(), x.to_bits());
            assert_eq!(scalar.translation().y().to_bits(), y.to_bits());
            assert_eq!(scalar.previous_translation().x().to_bits(), x.to_bits());
            assert_eq!(scalar.previous_translation().y().to_bits(), y.to_bits());
        }

        let nan = f32::from_bits(0x7fc0_1234);
        for (x, y) in [
            (nan, -0.0),
            (-0.0, nan),
            (f32::INFINITY, 1.0),
            (1.0, f32::INFINITY),
            (f32::NEG_INFINITY, -1.0),
            (-1.0, f32::NEG_INFINITY),
            (nan, f32::NEG_INFINITY),
        ] {
            let direct = Transform2d::new(Vec2::new(x, y)).unwrap_err();
            let scalar = Transform2d::from_xy(x, y).unwrap_err();
            match (direct, scalar) {
                (
                    VisualValueError::InvalidTranslation { value: direct },
                    VisualValueError::InvalidTranslation { value: scalar },
                ) => {
                    assert_eq!(scalar.x().to_bits(), direct.x().to_bits());
                    assert_eq!(scalar.y().to_bits(), direct.y().to_bits());
                }
                errors => panic!("from_xy changed the validation error: {errors:?}"),
            }
        }
    }

    #[test]
    fn centered_camera_exactly_delegates_to_sim_engine_camera() {
        for zoom in [0.0001, 1.0, 20.0, f32::MAX] {
            let direct = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, zoom).unwrap());
            let centered = ActiveCamera2d::centered(zoom).unwrap();

            assert_eq!(centered, direct);
            assert_eq!(centered.center(), Vec2::ZERO);
            assert_eq!(centered.previous_center(), Vec2::ZERO);
        }

        for zoom in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let direct = Camera2d::new(Vec2::ZERO, zoom).unwrap_err();
            let centered = ActiveCamera2d::centered(zoom).unwrap_err();

            match (direct, centered) {
                (
                    sim_engine::Camera2dError::InvalidZoom { zoom: direct },
                    sim_engine::Camera2dError::InvalidZoom { zoom: centered },
                ) => assert_eq!(centered.to_bits(), direct.to_bits()),
                errors => panic!("centered camera changed the validation error: {errors:?}"),
            }
        }
    }

    #[test]
    fn rejected_transform_updates_are_atomic() -> Result<(), VisualValueError> {
        let initial = Vec2::splat(f32::MAX);
        let mut transform = Transform2d::new(initial)?;

        let error = transform.translate_by(Vec2::new(f32::MAX, f32::MAX));

        assert!(matches!(
            error,
            Err(VisualValueError::InvalidTranslation { .. })
        ));
        assert_eq!(transform.translation(), initial);
        assert_eq!(transform.previous_translation(), initial);
        Ok(())
    }

    #[test]
    fn translated_transform_exactly_matches_mutating_a_copy() -> Result<(), VisualValueError> {
        let mut with_history = Transform2d::new(Vec2::new(-0.0, f32::from_bits(1)))?;
        with_history.set_translation(Vec2::new(4.0, -8.0))?;
        let valid = [
            (with_history, Vec2::new(0.5, 2.0)),
            (Transform2d::default(), Vec2::new(-0.0, f32::from_bits(1))),
            (Transform2d::default(), Vec2::new(f32::MAX, -f32::MAX)),
        ];

        for (source, delta) in valid {
            let original = source;
            let mut direct = source;
            direct.translate_by(delta)?;
            let translated = source.translated_by(delta)?;

            assert_transform_bits_eq(translated, direct);
            assert_transform_bits_eq(source, original);
            assert_vec2_bits_eq(
                translated.previous_translation(),
                original.previous_translation(),
            );
        }

        let nan = f32::from_bits(0x7fc0_1234);
        let invalid = [
            (with_history, Vec2::new(nan, -0.0)),
            (with_history, Vec2::new(-0.0, f32::INFINITY)),
            (
                Transform2d::new(Vec2::new(f32::MAX, -f32::MAX))?,
                Vec2::new(f32::MAX, -f32::MAX),
            ),
        ];

        for (source, delta) in invalid {
            let original = source;
            let mut direct = source;
            let direct_error = direct
                .translate_by(delta)
                .expect_err("test offset should be rejected");
            let translated_error = source
                .translated_by(delta)
                .expect_err("test offset should be rejected");

            assert_translation_error_bits_eq(translated_error, direct_error);
            assert_transform_bits_eq(source, original);
            assert_transform_bits_eq(direct, original);
        }
        Ok(())
    }

    fn assert_transform_bits_eq(actual: Transform2d, expected: Transform2d) {
        assert_vec2_bits_eq(actual.translation(), expected.translation());
        assert_vec2_bits_eq(
            actual.previous_translation(),
            expected.previous_translation(),
        );
    }

    fn assert_vec2_bits_eq(actual: Vec2, expected: Vec2) {
        assert_eq!(actual.x().to_bits(), expected.x().to_bits());
        assert_eq!(actual.y().to_bits(), expected.y().to_bits());
    }

    fn assert_translation_error_bits_eq(actual: VisualValueError, expected: VisualValueError) {
        match (actual, expected) {
            (
                VisualValueError::InvalidTranslation { value: actual },
                VisualValueError::InvalidTranslation { value: expected },
            ) => assert_vec2_bits_eq(actual, expected),
            errors => panic!("translated_by changed the validation error: {errors:?}"),
        }
    }

    #[test]
    fn fixed_tick_and_teleport_have_distinct_interpolation_semantics()
    -> Result<(), VisualValueError> {
        let mut transform = Transform2d::default();
        transform.set_translation(Vec2::new(1.0, 0.0))?;
        transform.begin_fixed_tick();
        transform.set_translation(Vec2::new(2.0, 0.0))?;

        assert_eq!(transform.previous_translation(), Vec2::new(1.0, 0.0));
        assert_eq!(transform.translation(), Vec2::new(2.0, 0.0));

        transform.teleport(Vec2::new(8.0, 4.0))?;
        assert_eq!(transform.previous_translation(), Vec2::new(8.0, 4.0));
        assert_eq!(transform.translation(), Vec2::new(8.0, 4.0));
        Ok(())
    }

    #[test]
    fn camera_center_updates_and_teleports_have_distinct_interpolation_semantics()
    -> Result<(), sim_engine::Camera2dError> {
        let mut camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
        camera.set_center(Vec2::new(1.0, 0.0))?;
        camera.begin_fixed_tick();
        camera.pan_by(Vec2::new(3.0, 2.0))?;

        assert_eq!(camera.previous_center(), Vec2::new(1.0, 0.0));
        assert_eq!(camera.center(), Vec2::new(4.0, 2.0));
        assert_eq!(
            camera.interpolated_camera(0.5).center(),
            Vec2::new(2.5, 1.0)
        );

        camera.teleport_center(Vec2::new(8.0, 4.0))?;
        assert_eq!(camera.previous_center(), Vec2::new(8.0, 4.0));
        assert_eq!(camera.center(), Vec2::new(8.0, 4.0));
        Ok(())
    }

    #[test]
    fn rejected_camera_updates_are_atomic() -> Result<(), sim_engine::Camera2dError> {
        let mut camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);

        assert!(camera.set_center(Vec2::splat(f32::NAN)).is_err());
        camera.teleport_center(Vec2::splat(f32::MAX))?;
        assert!(camera.pan_by(Vec2::splat(f32::MAX)).is_err());
        assert_eq!(camera.center(), Vec2::splat(f32::MAX));
        assert_eq!(camera.previous_center(), Vec2::splat(f32::MAX));
        Ok(())
    }

    #[test]
    fn whole_camera_replacement_interpolates_only_a_restored_center() -> Result<(), Box<dyn Error>>
    {
        let mut active = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
        let previous = active.previous_center();

        let mut replacement = Camera2d::new(Vec2::new(10.0, 4.0), 40.0)?;
        replacement.set_rotation(0.75)?;
        replacement.set_projection(sim_engine::Projection2d::new(0.25, 2.0)?);
        active = ActiveCamera2d::new(replacement);
        active.restore_previous_center(previous);

        let presentation = active.interpolated_camera(0.5);
        assert_eq!(presentation.center(), Vec2::new(5.0, 2.0));
        assert_eq!(presentation.zoom(), 40.0);
        assert_eq!(presentation.rotation(), 0.75);
        assert_eq!(presentation.projection(), replacement.projection());
        Ok(())
    }

    #[test]
    fn circle_rejects_invalid_required_values() {
        assert!(matches!(
            CircleVisual::new(0.0, Color::WHITE),
            Err(VisualValueError::InvalidRadius { .. })
        ));
        assert!(matches!(
            CircleVisual::new(1.0, Color::rgb(1.1, 0.0, 0.0)),
            Err(VisualValueError::InvalidColor { .. })
        ));
    }

    #[test]
    fn line_defaults_and_rejected_updates_are_atomic_and_bit_exact() -> Result<(), VisualValueError>
    {
        let vector = Vec2::new(4.0, -2.0);
        let color = Color::rgb8(255, 120, 72);
        let mut line = LineVisual::new(vector, 2.5, color)?;

        assert_eq!(line.vector(), vector);
        assert_eq!(line.stroke_width_logical_pixels(), 2.5);
        assert_eq!(line.color(), color);
        assert_eq!(line.layer(), Layer::DEFAULT);
        assert_eq!(line.draw_order_depth(), 0.0);

        for valid in [
            Vec2::ZERO,
            Vec2::new(-0.0, 0.0),
            Vec2::new(f32::from_bits(1), -f32::from_bits(1)),
            Vec2::new(f32::MAX, -f32::MAX),
        ] {
            line.set_vector(valid)?;
            assert_eq!(line.vector().x().to_bits(), valid.x().to_bits());
            assert_eq!(line.vector().y().to_bits(), valid.y().to_bits());
        }

        let retained_vector = line.vector();
        for invalid in [
            Vec2::new(f32::NAN, 0.0),
            Vec2::new(0.0, f32::INFINITY),
            Vec2::new(f32::NEG_INFINITY, f32::NAN),
        ] {
            assert!(matches!(
                line.set_vector(invalid),
                Err(VisualValueError::InvalidLineVector { value })
                    if value.x().to_bits() == invalid.x().to_bits()
                        && value.y().to_bits() == invalid.y().to_bits()
            ));
            assert_eq!(line.vector(), retained_vector);
        }

        let retained_width = line.stroke_width_logical_pixels();
        for invalid in [
            0.0,
            -0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
        ] {
            assert!(matches!(
                line.set_stroke_width_logical_pixels(invalid),
                Err(VisualValueError::InvalidLineWidth { value })
                    if value.to_bits() == invalid.to_bits()
            ));
            assert_eq!(line.stroke_width_logical_pixels(), retained_width);
        }

        assert!(matches!(
            LineVisual::new(Vec2::new(f32::NAN, 0.0), 0.0, Color::rgb(2.0, 0.0, 0.0),),
            Err(VisualValueError::InvalidLineVector { .. })
        ));
        assert!(matches!(
            LineVisual::new(vector, 0.0, Color::rgb(2.0, 0.0, 0.0)),
            Err(VisualValueError::InvalidLineWidth { .. })
        ));
        assert!(matches!(
            LineVisual::new(vector, 2.5, Color::rgb(2.0, 0.0, 0.0)),
            Err(VisualValueError::InvalidColor { .. })
        ));

        assert!(matches!(
            line.set_color(Color::rgb(-0.1, 0.0, 0.0)),
            Err(VisualValueError::InvalidColor { .. })
        ));
        assert_eq!(line.color(), color);
        line.set_layer(Layer::new(-7));
        assert_eq!(line.layer(), Layer::new(-7));
        line.set_draw_order_depth(3.0)?;
        assert!(matches!(
            line.set_draw_order_depth(f32::NAN),
            Err(VisualValueError::InvalidDrawOrderDepth { .. })
        ));
        assert_eq!(line.draw_order_depth(), 3.0);
        line.set_layer(Layer::new(4));
        assert_eq!(line.layer(), Layer::new(4));
        Ok(())
    }

    #[test]
    fn rectangle_defaults_and_rejected_updates_are_atomic() -> Result<(), VisualValueError> {
        let size = Vec2::new(4.0, 2.0);
        let color = Color::rgb8(68, 144, 255);
        let mut rectangle = RectangleVisual::new(size, color)?;

        assert_eq!(rectangle.size(), size);
        assert_eq!(rectangle.corner_radius(), 0.0);
        assert_eq!(rectangle.color(), color);
        assert_eq!(rectangle.layer(), Layer::DEFAULT);
        assert_eq!(rectangle.draw_order_depth(), 0.0);

        for invalid in [
            Vec2::ZERO,
            Vec2::new(-1.0, 1.0),
            Vec2::new(1.0, f32::NAN),
            Vec2::new(1.0, f32::NEG_INFINITY),
            Vec2::new(f32::INFINITY, 1.0),
        ] {
            assert!(matches!(
                rectangle.set_size(invalid),
                Err(VisualValueError::InvalidSize { .. })
            ));
            assert_eq!(rectangle.size(), size);
        }

        for invalid in [-1.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(
                rectangle.set_corner_radius(invalid),
                Err(VisualValueError::InvalidCornerRadius { .. })
            ));
            assert_eq!(rectangle.corner_radius(), 0.0);
        }

        rectangle.set_corner_radius(20.0)?;
        assert_eq!(rectangle.corner_radius(), 20.0);

        assert!(matches!(
            RectangleVisual::new(Vec2::ZERO, color),
            Err(VisualValueError::InvalidSize { .. })
        ));
        assert!(matches!(
            RectangleVisual::new(size, Color::rgb(1.1, 0.0, 0.0)),
            Err(VisualValueError::InvalidColor { .. })
        ));

        assert!(matches!(
            rectangle.set_color(Color::rgb(-0.1, 0.0, 0.0)),
            Err(VisualValueError::InvalidColor { .. })
        ));
        assert_eq!(rectangle.color(), color);

        rectangle.set_draw_order_depth(3.0)?;
        assert!(matches!(
            rectangle.set_draw_order_depth(f32::NAN),
            Err(VisualValueError::InvalidDrawOrderDepth { .. })
        ));
        assert_eq!(rectangle.draw_order_depth(), 3.0);
        Ok(())
    }

    #[test]
    fn rounded_rectangle_exactly_delegates_to_constructor_and_setter() {
        let size = Vec2::new(4.0, 2.0);
        let color = Color::rgb8(68, 144, 255);
        for corner_radius in [0.0, -0.0, f32::from_bits(1), 0.25, 20.0, f32::MAX] {
            let mut direct = RectangleVisual::new(size, color).unwrap();
            direct.set_corner_radius(corner_radius).unwrap();
            let rounded = RectangleVisual::rounded(size, color, corner_radius).unwrap();

            assert_eq!(rounded, direct);
            assert_eq!(rounded.corner_radius().to_bits(), corner_radius.to_bits());
        }

        assert!(matches!(
            RectangleVisual::rounded(
                Vec2::new(f32::NAN, 1.0),
                Color::rgb(2.0, 0.0, 0.0),
                f32::NAN,
            ),
            Err(VisualValueError::InvalidSize { .. })
        ));
        assert!(matches!(
            RectangleVisual::rounded(size, Color::rgb(2.0, 0.0, 0.0), f32::NAN),
            Err(VisualValueError::InvalidColor { .. })
        ));
        for corner_radius in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(matches!(
                RectangleVisual::rounded(size, color, corner_radius),
                Err(VisualValueError::InvalidCornerRadius { value })
                    if value.to_bits() == corner_radius.to_bits()
            ));
        }
    }
}

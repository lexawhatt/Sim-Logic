//! Managed presentation geometry in logical screen pixels.

use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;
use sim_engine::{Color, Layer, LogicalScreenPosition, LogicalScreenVector};

/// A filled, square-cornered rectangle positioned in logical screen pixels.
///
/// The origin is the content area's top-left, with x increasing rightward and
/// y downward. Finite negative and offscreen positions are allowed. World
/// camera movement, zoom, rotation, and fixed interpolation do not affect this
/// visual. It requires no world-space Transform and may coexist with a world
/// visual on the same entity without controlling that visual.
///
/// FrameUpdate may mutate this presentation-only component directly. Extraction
/// samples its current values, excludes disabled entities, and draws screen
/// rectangles after all world visuals. Within screen content, lower layer and
/// draw-order depth draw first; stable source identity breaks remaining ties.
/// Visibility does not capture input or provide hit testing. No window or GPU
/// is required to construct, update, or extract the component.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct ScreenRectangleVisual {
    position: LogicalScreenPosition,
    size: LogicalScreenVector,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl ScreenRectangleVisual {
    /// Creates a rectangle on the default layer with draw-order depth zero.
    ///
    /// Position must be finite, size must be finite and strictly positive on
    /// both axes, and color must be normalized straight-linear RGBA. Adding
    /// size to position in f32 must produce finite, strictly greater far bounds.
    pub fn new(
        position: LogicalScreenPosition,
        size: LogicalScreenVector,
        color: Color,
    ) -> Result<Self, ScreenVisualError> {
        let visual = Self {
            position,
            size,
            color,
            layer: Layer::DEFAULT,
            draw_order_depth: 0.0,
        };
        visual.validate()?;
        Ok(visual)
    }

    /// Returns the rectangle's top-left position in logical screen pixels.
    pub const fn position(&self) -> LogicalScreenPosition {
        self.position
    }

    /// Returns the full positive width and height in logical screen pixels.
    pub const fn size(&self) -> LogicalScreenVector {
        self.size
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(&self) -> Color {
        self.color
    }

    /// Returns the primary ordering layer within screen content.
    pub const fn layer(&self) -> Layer {
        self.layer
    }

    /// Returns the finite ordering depth within the screen layer.
    ///
    /// This value controls draw order only; it is not camera projection depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.draw_order_depth
    }

    /// Replaces position and size together after validating their bounds.
    ///
    /// Failure leaves every component value unchanged. Finite offscreen
    /// geometry is accepted without clamping.
    pub fn set_geometry(
        &mut self,
        position: LogicalScreenPosition,
        size: LogicalScreenVector,
    ) -> Result<(), ScreenVisualError> {
        validate_geometry(position, size)?;
        self.position = position;
        self.size = size;
        Ok(())
    }

    /// Replaces the logical top-left position while retaining size.
    ///
    /// Invalid or unrepresentable bounds leave the component unchanged.
    pub fn set_position(
        &mut self,
        position: LogicalScreenPosition,
    ) -> Result<(), ScreenVisualError> {
        self.set_geometry(position, self.size)
    }

    /// Replaces logical width and height while retaining position.
    ///
    /// Non-positive size or invalid bounds leave the component unchanged.
    pub fn set_size(&mut self, size: LogicalScreenVector) -> Result<(), ScreenVisualError> {
        self.set_geometry(self.position, size)
    }

    /// Replaces normalized straight-linear RGBA color atomically.
    ///
    /// A non-finite channel or channel outside `0.0..=1.0` leaves it unchanged.
    pub fn set_color(&mut self, color: Color) -> Result<(), ScreenVisualError> {
        validate_color(color)?;
        self.color = color;
        Ok(())
    }

    /// Replaces the ordering layer within screen content.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }

    /// Replaces within-layer ordering depth after finite-value validation.
    ///
    /// Negative values are allowed. Failure leaves the component unchanged.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), ScreenVisualError> {
        validate_draw_order_depth(depth)?;
        self.draw_order_depth = depth;
        Ok(())
    }

    pub(crate) fn validate(&self) -> Result<(), ScreenVisualError> {
        validate_geometry(self.position, self.size)?;
        validate_color(self.color)?;
        validate_draw_order_depth(self.draw_order_depth)
    }
}

/// Invalid value rejected by a screen visual's constructor or atomic setter.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ScreenVisualError {
    /// A logical screen position contained a non-finite coordinate.
    InvalidPosition {
        /// Rejected logical top-left position.
        value: LogicalScreenPosition,
    },
    /// A logical width or height was non-finite or not strictly positive.
    InvalidSize {
        /// Rejected logical-pixel size.
        value: LogicalScreenVector,
    },
    /// Adding size overflowed or failed to advance either f32 coordinate.
    InvalidBounds {
        /// Logical top-left position of the rejected geometry.
        position: LogicalScreenPosition,
        /// Logical-pixel size of the rejected geometry.
        size: LogicalScreenVector,
    },
    /// A color channel was non-finite or outside `0.0..=1.0`.
    InvalidColor {
        /// Rejected straight-linear RGBA color.
        value: Color,
    },
    /// Draw-order depth was non-finite.
    InvalidDrawOrderDepth {
        /// Rejected ordering value.
        value: f32,
    },
}

impl fmt::Display for ScreenVisualError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPosition { value } => {
                write!(
                    formatter,
                    "logical screen position must be finite, got {value:?}"
                )
            }
            Self::InvalidSize { value } => write!(
                formatter,
                "logical screen size must be positive and finite, got {value:?}"
            ),
            Self::InvalidBounds { position, size } => write!(
                formatter,
                "logical screen bounds must be finite and strictly increasing, got {position:?} plus {size:?}"
            ),
            Self::InvalidColor { value } => write!(
                formatter,
                "screen color must be normalized linear RGBA, got {value:?}"
            ),
            Self::InvalidDrawOrderDepth { value } => {
                write!(
                    formatter,
                    "screen draw-order depth must be finite, got {value}"
                )
            }
        }
    }
}

impl Error for ScreenVisualError {}

fn validate_geometry(
    position: LogicalScreenPosition,
    size: LogicalScreenVector,
) -> Result<(), ScreenVisualError> {
    if !position.is_finite() {
        return Err(ScreenVisualError::InvalidPosition { value: position });
    }
    let extent = size.to_vec2();
    if !size.is_finite() || extent.x() <= 0.0 || extent.y() <= 0.0 {
        return Err(ScreenVisualError::InvalidSize { value: size });
    }
    let minimum = position.to_vec2();
    let maximum = minimum + extent;
    if !maximum.is_finite() || maximum.x() <= minimum.x() || maximum.y() <= minimum.y() {
        return Err(ScreenVisualError::InvalidBounds { position, size });
    }
    Ok(())
}

fn validate_color(value: Color) -> Result<(), ScreenVisualError> {
    value
        .is_normalized()
        .then_some(())
        .ok_or(ScreenVisualError::InvalidColor { value })
}

fn validate_draw_order_depth(value: f32) -> Result<(), ScreenVisualError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(ScreenVisualError::InvalidDrawOrderDepth { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_bits(visual: ScreenRectangleVisual) -> [u32; 9] {
        let position = visual.position().to_vec2();
        let size = visual.size().to_vec2();
        [
            position.x(),
            position.y(),
            size.x(),
            size.y(),
            visual.color().red(),
            visual.color().green(),
            visual.color().blue(),
            visual.color().alpha(),
            visual.draw_order_depth(),
        ]
        .map(f32::to_bits)
    }

    #[test]
    fn finite_offscreen_and_small_representable_geometry_is_preserved()
    -> Result<(), ScreenVisualError> {
        let position = LogicalScreenPosition::new(-40.0, 9_000.0);
        let size = LogicalScreenVector::new(20.0, 10.0);
        let mut visual = ScreenRectangleVisual::new(position, size, Color::WHITE)?;
        assert_eq!(visual.position(), position);
        assert_eq!(visual.size(), size);
        assert_eq!(visual.layer(), Layer::DEFAULT);
        assert_eq!(visual.draw_order_depth(), 0.0);

        let subnormal = f32::from_bits(1);
        visual.set_geometry(
            LogicalScreenPosition::new(-0.0, 0.0),
            LogicalScreenVector::new(subnormal, subnormal),
        )?;
        assert_eq!(
            visual.position().to_vec2().x().to_bits(),
            (-0.0_f32).to_bits()
        );
        assert_eq!(visual.size().to_vec2().x().to_bits(), 1);
        visual.set_layer(Layer::new(-3));
        visual.set_draw_order_depth(-2.0)?;
        visual.set_color(Color::TRANSPARENT)?;
        assert_eq!(visual.layer(), Layer::new(-3));
        assert_eq!(visual.draw_order_depth(), -2.0);
        assert_eq!(visual.color(), Color::TRANSPARENT);
        Ok(())
    }

    #[test]
    fn constructors_reject_nonfinite_nonpositive_and_unrepresentable_geometry() {
        let origin = LogicalScreenPosition::default();
        let unit = LogicalScreenVector::new(1.0, 1.0);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for position in [
                LogicalScreenPosition::new(value, 0.0),
                LogicalScreenPosition::new(0.0, value),
            ] {
                assert!(matches!(
                    ScreenRectangleVisual::new(position, unit, Color::WHITE),
                    Err(ScreenVisualError::InvalidPosition { .. })
                ));
            }
        }
        for value in [0.0, -0.0, -1.0, f32::NAN, f32::INFINITY] {
            for size in [
                LogicalScreenVector::new(value, 1.0),
                LogicalScreenVector::new(1.0, value),
            ] {
                assert!(matches!(
                    ScreenRectangleVisual::new(origin, size, Color::WHITE),
                    Err(ScreenVisualError::InvalidSize { .. })
                ));
            }
        }
        for (position, size) in [
            (
                LogicalScreenPosition::new(f32::MAX, 0.0),
                LogicalScreenVector::new(f32::MAX, 1.0),
            ),
            (
                LogicalScreenPosition::new(0.0, f32::MAX),
                LogicalScreenVector::new(1.0, f32::MAX),
            ),
            (LogicalScreenPosition::new(16_777_216.0, 0.0), unit),
            (LogicalScreenPosition::new(0.0, 16_777_216.0), unit),
        ] {
            assert!(matches!(
                ScreenRectangleVisual::new(position, size, Color::WHITE),
                Err(ScreenVisualError::InvalidBounds { .. })
            ));
        }
        assert!(matches!(
            ScreenRectangleVisual::new(origin, unit, Color::rgb(2.0, 0.0, 0.0)),
            Err(ScreenVisualError::InvalidColor { .. })
        ));
    }

    #[test]
    fn failed_setters_preserve_every_prior_bit() -> Result<(), ScreenVisualError> {
        let mut visual = ScreenRectangleVisual::new(
            LogicalScreenPosition::new(-0.0, -0.0),
            LogicalScreenVector::new(16.0, 8.0),
            Color::rgba(-0.0, 0.25, 0.5, 1.0),
        )?;
        visual.set_draw_order_depth(-0.0)?;
        visual.set_layer(Layer::new(7));
        let before = state_bits(visual);
        assert!(
            visual
                .set_geometry(
                    LogicalScreenPosition::new(3.0, 4.0),
                    LogicalScreenVector::new(0.0, 1.0)
                )
                .is_err()
        );
        assert_eq!(state_bits(visual), before);
        assert!(
            visual
                .set_position(LogicalScreenPosition::new(f32::MAX, 0.0))
                .is_err()
        );
        assert_eq!(state_bits(visual), before);
        assert!(
            visual
                .set_size(LogicalScreenVector::new(1.0, f32::NAN))
                .is_err()
        );
        assert_eq!(state_bits(visual), before);
        for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
            assert!(visual.set_color(Color::WHITE.with_alpha(value)).is_err());
            assert_eq!(state_bits(visual), before);
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(visual.set_draw_order_depth(value).is_err());
            assert_eq!(state_bits(visual), before);
        }
        assert_eq!(visual.layer(), Layer::new(7));
        Ok(())
    }

    #[test]
    fn geometry_setter_validates_the_requested_pair_together() -> Result<(), ScreenVisualError> {
        let mut visual = ScreenRectangleVisual::new(
            LogicalScreenPosition::default(),
            LogicalScreenVector::new(1.0, 1.0),
            Color::WHITE,
        )?;
        let position = LogicalScreenPosition::new(16_777_216.0, 16_777_216.0);
        let size = LogicalScreenVector::new(2.0, 2.0);
        assert!(visual.set_position(position).is_err());
        visual.set_geometry(position, size)?;
        assert_eq!(visual.position(), position);
        assert_eq!(visual.size(), size);
        assert!(visual.set_size(LogicalScreenVector::new(1.0, 1.0)).is_err());
        assert_eq!(visual.size(), size);
        Ok(())
    }
}

//! Presentation-only vectors, independent of fixed-step world transforms.

use super::{
    ScreenClip, ScreenVisualError, rectangle::validate_stroke, validate_color,
    validate_draw_order_depth,
};
use bevy_ecs::prelude::Component;
use sim_engine::{
    Color, Layer, LogicalPixels, LogicalScreenPosition, SceneError, ScreenScene, ShapeStyle, Stroke,
};

/// A round-ended line in top-left/downward logical screen pixels.
///
/// FrameUpdate can move it while simulation is paused. No Transform2d or camera
/// is required. Draw order shares layer/depth/source ordering with the screen UI.
/// Values are CPU-only; Engine performs its authoritative portability check at extraction.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct ScreenLineVisual {
    start: LogicalScreenPosition,
    end: LogicalScreenPosition,
    stroke: Stroke,
    layer: Layer,
    depth: f32,
    clip: ScreenClip,
}

impl ScreenLineVisual {
    /// Returns the primary mixed-screen ordering layer.
    pub const fn layer(&self) -> Layer {
        self.layer
    }
    /// Sets the primary mixed-screen ordering layer.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }
    /// Returns finite within-layer ordering depth, not camera depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.depth
    }
    /// Sets within-layer depth atomically; nonfinite values are rejected.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), ScreenVisualError> {
        validate_draw_order_depth(depth)?;
        self.depth = depth;
        Ok(())
    }
    /// Returns the explicit fixed-screen clip.
    pub const fn clip(&self) -> ScreenClip {
        self.clip
    }
    /// Sets the clip without changing geometry or ordering.
    pub fn set_clip(&mut self, clip: ScreenClip) {
        self.clip = clip;
    }
    /// Creates a nondegenerate line with a positive logical width and normalized color.
    pub fn new(
        start: LogicalScreenPosition,
        end: LogicalScreenPosition,
        width: f32,
        color: Color,
    ) -> Result<Self, ScreenVisualError> {
        validate_endpoints(start, end)?;
        let stroke = Stroke::new(width, color);
        validate_stroke(stroke)?;
        Ok(Self {
            start,
            end,
            stroke,
            layer: Layer::DEFAULT,
            depth: 0.0,
            clip: ScreenClip::Unclipped,
        })
    }
    /// Returns the first endpoint in logical pixels.
    pub const fn start(&self) -> LogicalScreenPosition {
        self.start
    }
    /// Returns the second endpoint in logical pixels.
    pub const fn end(&self) -> LogicalScreenPosition {
        self.end
    }
    /// Returns logical width and normalized line color.
    pub const fn stroke(&self) -> Stroke {
        self.stroke
    }
    /// Sets both endpoints atomically; nonfinite or coincident endpoints are rejected.
    pub fn set_endpoints(
        &mut self,
        start: LogicalScreenPosition,
        end: LogicalScreenPosition,
    ) -> Result<(), ScreenVisualError> {
        validate_endpoints(start, end)?;
        self.start = start;
        self.end = end;
        Ok(())
    }
    /// Sets logical width and color atomically.
    pub fn set_stroke(&mut self, stroke: Stroke) -> Result<(), ScreenVisualError> {
        validate_stroke(stroke)?;
        self.stroke = stroke;
        Ok(())
    }
    /// Tests the clipped round-ended stroke, independently of entity/input routing.
    pub fn contains_pointer(&self, sample: crate::input::PointerSample) -> bool {
        if !self.clip.contains_pointer(sample) {
            return false;
        }
        let a = self.start.to_vec2();
        let b = self.end.to_vec2();
        let p = sample.position().to_vec2();
        let dx = f64::from(b.x()) - f64::from(a.x());
        let dy = f64::from(b.y()) - f64::from(a.y());
        let px = f64::from(p.x()) - f64::from(a.x());
        let py = f64::from(p.y()) - f64::from(a.y());
        let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        (px - t * dx).hypot(py - t * dy) <= f64::from(self.stroke.width()) * 0.5
    }
    pub(crate) fn append(&self, scene: &mut ScreenScene) -> Result<(), SceneError> {
        if !apply_clip(scene, self.clip)? {
            return Ok(());
        }
        let width = LogicalPixels::new(self.stroke.width())
            .map_err(|_| SceneError::InvalidStroke(sim_engine::ScenePrimitive::Line))?;
        scene.try_line_on_layer(self.layer, self.start, self.end, width, self.stroke.color())
    }
}

fn validate_endpoints(
    start: LogicalScreenPosition,
    end: LogicalScreenPosition,
) -> Result<(), ScreenVisualError> {
    for value in [start, end] {
        if !value.is_finite() {
            return Err(ScreenVisualError::InvalidPosition { value });
        }
    }
    if start == end || !(end.to_vec2() - start.to_vec2()).is_finite() {
        return Err(ScreenVisualError::InvalidLine);
    }
    Ok(())
}

/// A filled circle with optional centered outline in logical screen pixels.
/// FrameUpdate may animate center/radius/color even when fixed simulation is paused.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
pub struct ScreenCircleVisual {
    center: LogicalScreenPosition,
    radius: f32,
    color: Color,
    stroke: Option<Stroke>,
    layer: Layer,
    depth: f32,
    clip: ScreenClip,
}

impl ScreenCircleVisual {
    /// Returns the primary mixed-screen ordering layer.
    pub const fn layer(&self) -> Layer {
        self.layer
    }
    /// Sets the primary mixed-screen ordering layer.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }
    /// Returns finite within-layer ordering depth, not camera depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.depth
    }
    /// Sets within-layer depth atomically; nonfinite values are rejected.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), ScreenVisualError> {
        validate_draw_order_depth(depth)?;
        self.depth = depth;
        Ok(())
    }
    /// Returns the explicit fixed-screen clip.
    pub const fn clip(&self) -> ScreenClip {
        self.clip
    }
    /// Sets the clip without changing geometry or ordering.
    pub fn set_clip(&mut self, clip: ScreenClip) {
        self.clip = clip;
    }
    /// Creates a circle with finite center, positive finite radius, and normalized fill.
    pub fn new(
        center: LogicalScreenPosition,
        radius: f32,
        color: Color,
    ) -> Result<Self, ScreenVisualError> {
        validate_circle(center, radius)?;
        validate_color(color)?;
        Ok(Self {
            center,
            radius,
            color,
            stroke: None,
            layer: Layer::DEFAULT,
            depth: 0.0,
            clip: ScreenClip::Unclipped,
        })
    }
    /// Returns the logical screen center.
    pub const fn center(&self) -> LogicalScreenPosition {
        self.center
    }
    /// Returns the positive logical radius.
    pub const fn radius(&self) -> f32 {
        self.radius
    }
    /// Returns normalized straight-linear fill color.
    pub const fn color(&self) -> Color {
        self.color
    }
    /// Atomically changes center and radius, preserving old values on failure.
    pub fn set_geometry(
        &mut self,
        center: LogicalScreenPosition,
        radius: f32,
    ) -> Result<(), ScreenVisualError> {
        validate_circle(center, radius)?;
        self.center = center;
        self.radius = radius;
        Ok(())
    }
    /// Changes only the logical screen center, atomically.
    pub fn set_center(&mut self, center: LogicalScreenPosition) -> Result<(), ScreenVisualError> {
        self.set_geometry(center, self.radius)
    }
    /// Changes only the logical radius, atomically.
    pub fn set_radius(&mut self, radius: f32) -> Result<(), ScreenVisualError> {
        self.set_geometry(self.center, radius)
    }
    /// Sets normalized fill color; rejection leaves the previous color intact.
    pub fn set_color(&mut self, color: Color) -> Result<(), ScreenVisualError> {
        validate_color(color)?;
        self.color = color;
        Ok(())
    }
    /// Returns the optional decorative outline.
    pub const fn stroke(&self) -> Option<Stroke> {
        self.stroke
    }
    /// Sets a positive-width normalized-color outline, or removes it with None.
    pub fn set_stroke(&mut self, stroke: Option<Stroke>) -> Result<(), ScreenVisualError> {
        if let Some(stroke) = stroke {
            validate_stroke(stroke)?;
        }
        self.stroke = stroke;
        Ok(())
    }
    /// Tests the clipped filled disk, not the decorative stroke or painter priority.
    pub fn contains_pointer(&self, sample: crate::input::PointerSample) -> bool {
        if !self.clip.contains_pointer(sample) {
            return false;
        }
        let p = sample.position().to_vec2();
        let c = self.center.to_vec2();
        (f64::from(p.x()) - f64::from(c.x())).hypot(f64::from(p.y()) - f64::from(c.y()))
            <= f64::from(self.radius)
    }
    pub(crate) fn append(&self, scene: &mut ScreenScene) -> Result<(), SceneError> {
        if !apply_clip(scene, self.clip)? {
            return Ok(());
        }
        let style = self.stroke.map_or_else(
            || ShapeStyle::filled(self.color),
            |stroke| ShapeStyle::fill_stroke(self.color, stroke.width(), stroke.color()),
        );
        let radius = LogicalPixels::new(self.radius)
            .map_err(|_| SceneError::InvalidDimension(sim_engine::ScenePrimitive::Circle))?;
        scene.try_circle_on_layer(self.layer, self.center, radius, style)
    }
}

fn validate_circle(center: LogicalScreenPosition, radius: f32) -> Result<(), ScreenVisualError> {
    if !center.is_finite() {
        return Err(ScreenVisualError::InvalidPosition { value: center });
    }
    if !radius.is_finite() || radius <= 0.0 {
        return Err(ScreenVisualError::InvalidRadius { value: radius });
    }
    Ok(())
}

fn apply_clip(scene: &mut ScreenScene, clip: ScreenClip) -> Result<bool, SceneError> {
    match clip {
        ScreenClip::Empty => Ok(false),
        ScreenClip::Unclipped => {
            scene.set_screen_clip(None)?;
            Ok(true)
        }
        ScreenClip::Rectangle(clip) => {
            scene.set_screen_clip(Some(clip))?;
            Ok(true)
        }
    }
}

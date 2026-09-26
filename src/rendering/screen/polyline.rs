//! Shared CPU paths; Engine alone validates and tessellates their strokes.

use super::{ScreenClip, ScreenVisualError, validate_draw_order_depth, vectors::apply_clip};
use bevy_ecs::prelude::Component;
use sim_engine::{Color, Layer, LogicalScreenPosition, SceneError, ScreenScene, StrokeStyle2d};
use std::{error::Error, fmt, sync::Arc};

/// One open line strip in top-left/downward logical screen pixels.
///
/// One component represents the whole path, with continuous joins/dashes and
/// only two endpoint caps. Two points provide a styled line. Engine owns stroke
/// validation, clipping and tessellation. Repeating the first point at the end
/// is rejected: Engine 0.4.2 has no closed styled-path contract.
///
/// Clones share immutable points and style; setters validate before replacing state.
/// World cameras and fixed interpolation do not transform these coordinates.
/// CPU creation requires no window or GPU.
#[derive(Debug, Clone, PartialEq, Component)]
pub struct ScreenPolylineVisual {
    points: Arc<Vec<LogicalScreenPosition>>,
    // Keep the large dash/marker value outside the common primitive enum.
    // Simple rectangles/lines must not pay for unused path-style storage.
    style: Arc<StrokeStyle2d>,
    max_points: usize,
    layer: Layer,
    depth: f32,
    clip: ScreenClip,
}

impl ScreenPolylineVisual {
    /// Default per-path point ceiling; aggregate extraction limits are separate.
    pub const DEFAULT_MAX_POINTS: usize = 65_536;

    /// Copies a bounded open path with explicit logical stroke style.
    /// Invalid geometry/style, world-unit widths and point-buffer reservation
    /// failures are errors. Small shared metadata uses Rust's ordinary allocator.
    pub fn new(
        points: &[LogicalScreenPosition],
        style: StrokeStyle2d,
    ) -> Result<Self, ScreenPolylineError> {
        Self::with_point_limit(points, style, Self::DEFAULT_MAX_POINTS)
    }
    /// Creates a path with an immutable point-count ceiling, at least two.
    /// This is not an application quota; set aggregate RenderLimits separately.
    pub fn with_point_limit(
        points: &[LogicalScreenPosition],
        style: StrokeStyle2d,
        max_points: usize,
    ) -> Result<Self, ScreenPolylineError> {
        validate(points, style, max_points)?;
        Ok(Self {
            points: copy_points(points)?,
            style: Arc::new(style),
            max_points,
            layer: Layer::DEFAULT,
            depth: 0.0,
            clip: ScreenClip::Unclipped,
        })
    }
    /// Borrows all immutable logical-screen points.
    pub fn points(&self) -> &[LogicalScreenPosition] {
        &self.points
    }
    /// Returns the fixed per-path point-count ceiling.
    pub const fn max_points(&self) -> usize {
        self.max_points
    }
    /// Returns logical width/color, caps, joins, dashes and markers.
    pub fn style(&self) -> StrokeStyle2d {
        *self.style
    }
    /// Returns point Vec capacity bytes, excluding shared-Arc metadata.
    /// Clones report shared storage; do not sum them as independent allocations.
    pub fn point_allocation_bytes(&self) -> usize {
        self.points
            .capacity()
            .saturating_mul(size_of::<LogicalScreenPosition>())
    }
    /// Atomically copies new points. Equal points skip validation and copying.
    pub fn set_points(
        &mut self,
        points: &[LogicalScreenPosition],
    ) -> Result<(), ScreenPolylineError> {
        if self.points() == points {
            return Ok(());
        }
        validate(points, *self.style, self.max_points)?;
        self.points = copy_points(points)?;
        Ok(())
    }
    /// Atomically changes style, preserving shared point storage.
    pub fn set_style(&mut self, style: StrokeStyle2d) -> Result<(), ScreenPolylineError> {
        if *self.style == style {
            return Ok(());
        }
        validate(self.points(), style, self.max_points)?;
        *Arc::make_mut(&mut self.style) = style;
        Ok(())
    }
    /// Returns the mixed-screen ordering layer.
    pub const fn layer(&self) -> Layer {
        self.layer
    }
    /// Sets the layer without copying points.
    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }
    /// Returns within-layer ordering depth, not camera depth.
    pub const fn draw_order_depth(&self) -> f32 {
        self.depth
    }
    /// Sets finite ordering depth; rejection leaves the old value intact.
    pub fn set_draw_order_depth(&mut self, depth: f32) -> Result<(), ScreenVisualError> {
        validate_draw_order_depth(depth)?;
        self.depth = depth;
        Ok(())
    }
    /// Returns the fixed logical-screen clip.
    pub const fn clip(&self) -> ScreenClip {
        self.clip
    }
    /// Sets clipping without copying or moving points.
    pub fn set_clip(&mut self, clip: ScreenClip) {
        self.clip = clip;
    }

    /// Picks the clipped centerline within a finite nonnegative logical radius.
    ///
    /// This is editor selection, not painted-pixel coverage: it includes dash
    /// gaps and endpoint disks, ignoring caps, joins, markers, alpha and stroke
    /// width. Supply the selection radius explicitly. Work is linear in the
    /// bounded point count, without allocations or GPU access.
    pub fn hit_test_centerline(
        &self,
        sample: crate::input::PointerSample,
        radius: f32,
    ) -> Result<bool, ScreenPolylineError> {
        if !radius.is_finite() || radius < 0.0 {
            return Err(ScreenPolylineError::InvalidPickRadius);
        }
        if !self.clip.contains_pointer(sample) {
            return Ok(false);
        }
        let p = sample.position().to_vec2();
        Ok(self.points.windows(2).any(|pair| {
            let a = pair[0].to_vec2();
            let b = pair[1].to_vec2();
            let dx = f64::from(b.x()) - f64::from(a.x());
            let dy = f64::from(b.y()) - f64::from(a.y());
            let px = f64::from(p.x()) - f64::from(a.x());
            let py = f64::from(p.y()) - f64::from(a.y());
            let t = ((px * dx + py * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
            (px - t * dx).hypot(py - t * dy) <= f64::from(radius)
        }))
    }
    pub(crate) fn append(&self, scene: &mut ScreenScene) -> Result<(), SceneError> {
        if !apply_clip(scene, self.clip)? {
            return Ok(());
        }
        scene.try_styled_polyline_on_layer(self.layer, self.points(), *self.style)
    }
}

fn validate(
    points: &[LogicalScreenPosition],
    style: StrokeStyle2d,
    limit: usize,
) -> Result<(), ScreenPolylineError> {
    if limit < 2 || points.len() > limit {
        return Err(ScreenPolylineError::PointLimit {
            limit,
            requested: points.len(),
        });
    }
    if points.len() >= 2 && points.first() == points.last() {
        return Err(ScreenPolylineError::ClosedPathUnsupported);
    }
    // Engine validates turns, dashes, marker constraints and logical width.
    // Its temporary conversion is fallible; no approximate validator in Logic.
    let mut probe = ScreenScene::new(Color::TRANSPARENT).map_err(ScreenPolylineError::Geometry)?;
    probe
        .try_styled_polyline(points, style)
        .map_err(ScreenPolylineError::Geometry)
}
fn copy_points(
    points: &[LogicalScreenPosition],
) -> Result<Arc<Vec<LogicalScreenPosition>>, ScreenPolylineError> {
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(points.len())
        .map_err(|_| ScreenPolylineError::Allocation {
            requested_bytes: points
                .len()
                .saturating_mul(size_of::<LogicalScreenPosition>()),
        })?;
    owned.extend_from_slice(points);
    Ok(Arc::new(owned))
}

/// A rejected path constructor/setter leaves existing visual state intact.
#[derive(Debug)]
pub enum ScreenPolylineError {
    /// Requested count exceeds the ceiling, or the ceiling is below two.
    PointLimit {
        /// Per-path point ceiling.
        limit: usize,
        /// Requested point count.
        requested: usize,
    },
    /// Engine 0.4.2 cannot join a closed seam without endpoint overlap.
    ClosedPathUnsupported,
    /// Exact Engine geometry/style rejection, including unsupported width units.
    Geometry(SceneError),
    /// The owned point buffer could not be reserved.
    Allocation {
        /// Minimum requested point-buffer bytes.
        requested_bytes: usize,
    },
    /// Selection radius must be finite and nonnegative.
    InvalidPickRadius,
}
impl fmt::Display for ScreenPolylineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PointLimit { limit, requested } => write!(
                f,
                "screen path requests {requested} points with limit {limit} (minimum limit: 2)"
            ),
            Self::ClosedPathUnsupported => {
                f.write_str("closed styled screen paths require an Engine closed-path API")
            }
            Self::Geometry(error) => write!(f, "screen path: {error}"),
            Self::Allocation { requested_bytes } => {
                write!(f, "screen path could not reserve {requested_bytes} bytes")
            }
            Self::InvalidPickRadius => {
                f.write_str("path picking radius must be finite and nonnegative")
            }
        }
    }
}
impl Error for ScreenPolylineError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            _ => None,
        }
    }
}

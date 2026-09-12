//! Atomic CPU extraction into bounded Sim;Engine visual state.

use crate::{
    assets::ImageAssetId,
    identity::{LogicEntity, WorldGeneration},
    render::RenderLimits,
    screen::{ImageVisualError, ScreenVisualError},
    three_d::{
        CuboidSource, MeshSource, ResolvedCuboid3d, ResolvedMesh3d, ThreeDExtractionBuffer,
        ThreeDExtractionError, ThreeDRenderLimits, ThreeDSnapshot, View3d,
    },
    visual::{
        ActiveCamera2d, CircleVisual, LineVisual, RectangleVisual, Transform2d, VisualValueError,
        line_stroke_style,
    },
};
use sim_engine::{
    Camera2d, Color, Layer, Rect, Scene, SceneBudgetResource, SceneError, ShapeStyle, Vec2,
};
use std::{cmp::Ordering, error::Error, fmt, mem::size_of};

#[path = "extraction/screen.rs"]
mod screen;
#[cfg(feature = "text")]
pub use screen::ResolvedScreenText;
use screen::ScreenExtractionBuffer;
#[cfg(feature = "text")]
pub(crate) use screen::ScreenTextSource;
pub use screen::{ResolvedScreenImage, ResolvedScreenRectangle, ScreenDraw};
pub(crate) use screen::{ScreenImageSource, ScreenRectangleSource, ScreenSource};

/// One fully resolved circle retained for headless parity and diagnostics.
///
/// Records preserve their relative order from the single deterministic mixed
/// Sim;Engine scene. The interpolated position is presentation state and is
/// never written back to the ECS World.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedCircle {
    source: LogicEntity,
    position: Vec2,
    radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

/// One fully resolved rectangle retained for headless parity and diagnostics.
///
/// Records preserve their relative order from the single deterministic mixed
/// Sim;Engine scene. Position is the interpolated world-space center; size and
/// style are sampled directly from the canonical component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedRectangle {
    source: LogicEntity,
    position: Vec2,
    size: Vec2,
    corner_radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

/// One fully resolved anchored line retained for headless parity and diagnostics.
///
/// `from` and `to` are presentation-space world points after Transform
/// interpolation. Vector and style are sampled from the current component and
/// never written back to the ECS World.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedLine {
    source: LogicEntity,
    from: Vec2,
    to: Vec2,
    stroke_width_logical_pixels: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl ResolvedLine {
    /// Returns the managed entity that produced this visual.
    pub const fn source(self) -> LogicEntity {
        self.source
    }

    /// Returns the interpolated anchor in world coordinates.
    pub const fn from(self) -> Vec2 {
        self.from
    }

    /// Returns the resolved endpoint in world coordinates.
    pub const fn to(self) -> Vec2 {
        self.to
    }

    /// Returns stroke width in logical screen pixels.
    pub const fn stroke_width_logical_pixels(self) -> f32 {
        self.stroke_width_logical_pixels
    }

    /// Returns the normalized straight-linear RGBA stroke color.
    pub const fn color(self) -> Color {
        self.color
    }

    /// Returns the primary Sim;Engine draw layer.
    pub const fn layer(self) -> Layer {
        self.layer
    }

    /// Returns the within-layer ordering value used by Sim;Logic.
    ///
    /// This is not Sim;Engine projection depth.
    pub const fn draw_order_depth(self) -> f32 {
        self.draw_order_depth
    }
}

impl ResolvedRectangle {
    /// Returns the managed entity that produced this visual.
    pub const fn source(self) -> LogicEntity {
        self.source
    }

    /// Returns the interpolated world-space center.
    pub const fn position(self) -> Vec2 {
        self.position
    }

    /// Returns the full width and height in caller-defined world units.
    pub const fn size(self) -> Vec2 {
        self.size
    }

    /// Returns the requested corner radius before renderer-side clamping.
    pub const fn corner_radius(self) -> f32 {
        self.corner_radius
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(self) -> Color {
        self.color
    }

    /// Returns the primary Sim;Engine draw layer.
    pub const fn layer(self) -> Layer {
        self.layer
    }

    /// Returns the within-layer ordering value used by Sim;Logic.
    ///
    /// This is not Sim;Engine projection depth.
    pub const fn draw_order_depth(self) -> f32 {
        self.draw_order_depth
    }
}

impl ResolvedCircle {
    /// Returns the managed entity that produced this visual.
    pub const fn source(self) -> LogicEntity {
        self.source
    }

    /// Returns the interpolated world-space center.
    pub const fn position(self) -> Vec2 {
        self.position
    }

    /// Returns the circle radius in caller-defined world units.
    pub const fn radius(self) -> f32 {
        self.radius
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(self) -> Color {
        self.color
    }

    /// Returns the primary Sim;Engine draw layer.
    pub const fn layer(self) -> Layer {
        self.layer
    }

    /// Returns the within-layer ordering value used by Sim;Logic.
    ///
    /// This is not Sim;Engine projection depth.
    pub const fn draw_order_depth(self) -> f32 {
        self.draw_order_depth
    }
}

/// Complete renderer-independent visual snapshot for one World generation.
///
/// A snapshot is published only after every required visual and its CPU/Scene
/// limits have been validated. The desktop bridge must compare
/// [`ExtractedFrame::world_generation`] with the active World immediately
/// before submission so a retired generation is never rendered.
pub struct ExtractedFrame {
    world_generation: WorldGeneration,
    background: Color,
    camera: Camera2d,
    storage: ExtractionBuffer,
}

struct ExtractionBuffer {
    resolved_circles: Vec<ResolvedCircle>,
    resolved_rectangles: Vec<ResolvedRectangle>,
    resolved_lines: Vec<ResolvedLine>,
    #[cfg_attr(
        not(feature = "desktop"),
        allow(dead_code, reason = "the headless build retains CPU parity data only")
    )]
    world_scene: Scene,
    screen: ScreenExtractionBuffer,
    three_d: ThreeDExtractionBuffer,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedMetadata {
    world_generation: WorldGeneration,
    background: Color,
    camera: Camera2d,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ExtractionParameters {
    world_generation: WorldGeneration,
    background: Color,
    alpha: f32,
    limits: RenderLimits,
}

/// Application-owned published/work storage for atomic CPU extraction.
pub(crate) struct ExtractionBuffers {
    published: Option<ExtractedFrame>,
    spare: Option<ExtractionBuffer>,
    staged: Option<ExtractedMetadata>,
}

impl ExtractedFrame {
    /// Returns the World generation from which this snapshot was extracted.
    pub const fn world_generation(&self) -> WorldGeneration {
        self.world_generation
    }

    /// Returns the normalized clear color for the composed frame.
    pub const fn background(&self) -> Color {
        self.background
    }

    /// Returns the validated, interpolated presentation camera for this frame.
    ///
    /// This may differ from the active component's canonical camera during a
    /// partial fixed interval.
    pub const fn camera(&self) -> Camera2d {
        self.camera
    }

    /// Returns value-comparable circles in their relative mixed-scene order.
    pub fn resolved_circles(&self) -> &[ResolvedCircle] {
        &self.storage.resolved_circles
    }

    /// Returns value-comparable rectangles in their relative mixed-scene order.
    pub fn resolved_rectangles(&self) -> &[ResolvedRectangle] {
        &self.storage.resolved_rectangles
    }

    /// Returns nonzero lines in their relative mixed-scene order.
    pub fn resolved_lines(&self) -> &[ResolvedLine] {
        &self.storage.resolved_lines
    }

    /// Returns screen rectangles in their deterministic screen-layer order.
    ///
    /// Positions and sizes are current logical pixels, not interpolated world
    /// coordinates. All these records are composed above the world scene.
    pub fn resolved_screen_rectangles(&self) -> &[ResolvedScreenRectangle] {
        self.storage.screen.resolved()
    }

    /// Returns current screen images in their relative mixed-screen order.
    ///
    /// Records contain immutable asset IDs, not GPU resources. Use
    /// `HeadlessRunner::image_asset` to inspect the registered source pixels.
    pub fn resolved_screen_images(&self) -> &[ResolvedScreenImage] {
        self.storage.screen.images()
    }

    /// Returns current prepared labels in their relative mixed-screen order.
    ///
    /// Strings, metrics, and fonts share their component's immutable CPU
    /// preparation. No shaping, GPU allocation, or fixed interpolation occurs.
    #[cfg(feature = "text")]
    pub fn resolved_screen_texts(&self) -> &[ResolvedScreenText] {
        self.storage.screen.texts()
    }

    /// Returns the enabled 3D view and cuboids, or None when 3D is suppressed.
    ///
    /// Desktop composition places this opaque depth target above the 2D world
    /// and below all screen draws. The snapshot contains no GPU resources.
    pub fn three_d(&self) -> Option<ThreeDSnapshot<'_>> {
        self.storage.three_d.snapshot()
    }

    /// Returns current visible cuboids in stable managed-identity order.
    ///
    /// Missing or disabled View3d resources produce an empty slice.
    pub fn resolved_cuboids(&self) -> &[ResolvedCuboid3d] {
        self.storage.three_d.resolved()
    }

    /// Returns current visible custom meshes in stable managed-identity order.
    /// Missing or disabled View3d resources produce an empty slice.
    pub fn resolved_meshes(&self) -> &[ResolvedMesh3d] {
        self.storage
            .three_d
            .snapshot()
            .map_or(&[], |snapshot| snapshot.meshes())
    }

    /// Returns the exact screen composition after the world scene.
    ///
    /// Contiguous rectangle runs, images, and optional text share layer, depth,
    /// and stable source order. Exact same-source ties put rectangles first,
    /// then images, then text. Empty screen content produces no items. This plan does not
    /// replace the desktop compositor's aggregate budget checks.
    pub fn screen_draws(&self) -> &[ScreenDraw] {
        self.storage.screen.draws()
    }

    /// Returns the sorted rectangle records for one run in `screen_draws`.
    pub fn screen_rectangle_run_records(&self, run: usize) -> Option<&[ResolvedScreenRectangle]> {
        self.storage.screen.run_records(run)
    }

    #[cfg(feature = "desktop")]
    pub(crate) fn screen_rectangle_run(&self, run: usize) -> Option<&sim_engine::ScreenScene> {
        self.storage.screen.run_scene(run)
    }

    /// Returns the camera-independent scene for the optional desktop bridge.
    #[cfg(feature = "desktop")]
    pub(crate) fn screen_scene(&self) -> &sim_engine::ScreenScene {
        self.storage.screen.scene()
    }

    /// Returns the bounded world scene used by the desktop bridge.
    #[cfg_attr(
        not(feature = "desktop"),
        allow(dead_code, reason = "used by the optional desktop bridge")
    )]
    pub(crate) const fn world_scene(&self) -> &Scene {
        &self.storage.world_scene
    }
}

impl ExtractionBuffer {
    fn new(limits: RenderLimits) -> Result<Self, ExtractionError> {
        let world_scene = Scene::with_budget(Color::BLACK, limits.world_scene_budget())
            .map_err(ExtractionError::Scene)?;
        Ok(Self {
            resolved_circles: Vec::new(),
            resolved_rectangles: Vec::new(),
            resolved_lines: Vec::new(),
            world_scene,
            screen: ScreenExtractionBuffer::new(limits)?,
            three_d: ThreeDExtractionBuffer::new(),
        })
    }
}

impl ExtractedMetadata {
    fn with_storage(self, storage: ExtractionBuffer) -> ExtractedFrame {
        ExtractedFrame {
            world_generation: self.world_generation,
            background: self.background,
            camera: self.camera,
            storage,
        }
    }
}

impl ExtractionParameters {
    pub(crate) const fn new(
        world_generation: WorldGeneration,
        background: Color,
        alpha: f32,
        limits: RenderLimits,
    ) -> Self {
        Self {
            world_generation,
            background,
            alpha,
            limits,
        }
    }
}

impl ExtractionBuffers {
    pub(crate) const fn new() -> Self {
        Self {
            published: None,
            spare: None,
            staged: None,
        }
    }

    /// Invalidates metadata from an abandoned candidate before more fallible work.
    pub(crate) fn discard_staged(&mut self) {
        self.staged = None;
    }

    pub(crate) fn stage(
        &mut self,
        parameters: ExtractionParameters,
        cameras: impl IntoIterator<Item = ActiveCamera2d>,
        circles: impl IntoIterator<Item = CircleSource>,
        rectangles: impl IntoIterator<Item = RectangleSource>,
        lines: impl IntoIterator<Item = LineSource>,
        screen_rectangles: impl IntoIterator<Item = ScreenSource>,
    ) -> Result<(), ExtractionError> {
        self.staged = None;
        let mut spare = match self.spare.take() {
            Some(spare) => spare,
            None => ExtractionBuffer::new(parameters.limits)?,
        };
        let result = extract_frame_into(
            &mut spare,
            parameters,
            cameras,
            circles,
            rectangles,
            lines,
            screen_rectangles,
        );
        self.spare = Some(spare);
        let metadata = result?;
        self.staged = Some(metadata);
        Ok(())
    }

    #[cfg(test)]
    fn stage_world_only(
        &mut self,
        parameters: ExtractionParameters,
        cameras: impl IntoIterator<Item = ActiveCamera2d>,
        circles: impl IntoIterator<Item = CircleSource>,
        rectangles: impl IntoIterator<Item = RectangleSource>,
        lines: impl IntoIterator<Item = LineSource>,
    ) -> Result<(), ExtractionError> {
        self.stage(parameters, cameras, circles, rectangles, lines, [])
    }

    pub(crate) fn stage_three_d(
        &mut self,
        generation: WorldGeneration,
        limits: ThreeDRenderLimits,
        view: Option<View3d>,
        sources: impl IntoIterator<Item = CuboidSource>,
        meshes: impl IntoIterator<Item = MeshSource>,
    ) -> Result<(), ExtractionError> {
        let Some(metadata) = self.staged.take() else {
            return Err(ExtractionError::MissingStagedFrame);
        };
        let Some(spare) = self.spare.as_mut() else {
            return Err(ExtractionError::MissingStagedFrame);
        };
        spare
            .three_d
            .extract(generation, limits, view, sources)
            .map_err(ExtractionError::ThreeD)?;
        spare
            .three_d
            .extract_meshes(generation, limits, meshes)
            .map_err(ExtractionError::ThreeD)?;
        self.staged = Some(metadata);
        Ok(())
    }

    /// Publishes the complete staged buffer and recycles the previous one.
    pub(crate) fn publish(&mut self) -> Option<WorldGeneration> {
        let metadata = self.staged.take()?;
        let Some(storage) = self.spare.take() else {
            self.staged = Some(metadata);
            return None;
        };
        let generation = metadata.world_generation;
        let previous = self.published.replace(metadata.with_storage(storage));
        self.spare = previous.map(|frame| frame.storage);
        Some(generation)
    }

    pub(crate) const fn published(&self) -> Option<&ExtractedFrame> {
        self.published.as_ref()
    }
}

/// Structured reason why a new CPU visual snapshot was not published.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExtractionError {
    /// A private staged snapshot was unavailable for completing 3D extraction.
    MissingStagedFrame,
    /// Independent bounded 3D snapshot preparation failed.
    ThreeD(ThreeDExtractionError),
    /// The active World did not contain a camera marked for extraction.
    MissingActiveCamera,
    /// The active World contained more than one marked camera.
    MultipleActiveCameras,
    /// Interpolation alpha was non-finite or outside the inclusive `0.0..=1.0` range.
    InvalidInterpolationAlpha {
        /// Rejected accumulator fraction.
        alpha: f32,
    },
    /// A visual entity did not belong to the generation being extracted.
    ForeignEntity {
        /// Entity that carried stale or foreign provenance.
        entity: LogicEntity,
        /// Generation accepted by this extraction.
        expected: WorldGeneration,
    },
    /// A component contained a value outside its public visual contract.
    InvalidVisual {
        /// Entity whose visual data was rejected.
        entity: LogicEntity,
        /// Exact invalid value category.
        error: VisualValueError,
    },
    /// Interpolation of two finite translations did not produce a finite result.
    InvalidInterpolatedTranslation {
        /// Entity whose interpolation failed.
        entity: LogicEntity,
    },
    /// A finite center and size produced collapsed or non-finite f32 bounds.
    InvalidResolvedRectangle {
        /// Entity whose derived bounds were not representable.
        entity: LogicEntity,
    },
    /// An interpolated anchor and finite vector did not form a representable segment.
    InvalidResolvedLine {
        /// Entity whose resolved segment was rejected.
        entity: LogicEntity,
    },
    /// The configured extracted-circle count was exceeded.
    CircleLimitExceeded {
        /// Configured maximum resolved circle count.
        limit: usize,
    },
    /// The configured extracted-rectangle count was exceeded.
    RectangleLimitExceeded {
        /// Configured maximum resolved rectangle count.
        limit: usize,
    },
    /// The configured enabled line-source count was exceeded.
    LineLimitExceeded {
        /// Configured maximum line-source count.
        limit: usize,
    },
    /// The configured enabled screen-rectangle count was exceeded.
    ScreenRectangleLimitExceeded {
        /// Maximum number of screen rectangles accepted in one extraction.
        limit: usize,
    },
    /// The opted-in enabled screen-image source count was exceeded.
    ScreenImageLimitExceeded {
        /// Configured image source limit (zero by default).
        limit: usize,
    },
    /// The opted-in enabled screen-text source count was exceeded.
    #[cfg(feature = "text")]
    ScreenTextLimitExceeded {
        /// Configured label count limit, zero by default.
        limit: usize,
    },
    /// Aggregate enabled label strings exceed the UTF-8 byte allowance.
    #[cfg(feature = "text")]
    ScreenTextBytesLimitExceeded {
        /// Configured inclusive UTF-8 byte limit.
        limit: usize,
        /// Requested aggregate bytes; usize::MAX also represents overflow.
        requested: usize,
    },
    /// Aggregate enabled labels exceed the shaped-glyph allowance.
    #[cfg(feature = "text")]
    ScreenTextGlyphLimitExceeded {
        /// Configured inclusive shaped-glyph count limit.
        limit: usize,
        /// Requested aggregate glyphs; usize::MAX also represents overflow.
        requested: usize,
    },
    /// A label uses a font not registered by this Application.
    #[cfg(feature = "text")]
    UnregisteredTextFont {
        /// Managed source of the foreign or unavailable font.
        entity: LogicEntity,
        /// Shared immutable font whose Application provenance was rejected.
        font: crate::text::TextFont,
    },
    /// A label violated its prepared-text, position, or tint contract.
    #[cfg(feature = "text")]
    InvalidScreenText {
        /// Managed source whose text visual was rejected.
        entity: LogicEntity,
        /// Exact component validation failure.
        error: crate::text::TextError,
    },
    /// An image handle was not registered by this Application.
    UnregisteredImageAsset {
        /// Managed source of the invalid reference.
        entity: LogicEntity,
        /// Rejected immutable image handle.
        image: ImageAssetId,
    },
    /// An image component violated its geometry, tint, or source-region contract.
    InvalidScreenImage {
        /// Managed source whose visual was rejected.
        entity: LogicEntity,
        /// Exact component validation failure.
        error: ImageVisualError,
    },
    /// A screen visual violated its validated logical-pixel contract.
    InvalidScreenVisual {
        /// Managed entity whose screen visual was rejected.
        entity: LogicEntity,
        /// Exact invalid value category.
        error: ScreenVisualError,
    },
    /// CPU storage for a bounded fixed-size resolved record could not be reserved.
    AllocationFailed {
        /// Minimum bytes requested for the next fixed-size record.
        requested_bytes: usize,
    },
    /// Sim;Engine rejected bounded scene construction.
    Scene(SceneError),
    /// Sim;Engine rejected bounded screen-scene construction.
    ScreenScene(SceneError),
}

impl fmt::Display for ExtractionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingStagedFrame => formatter.write_str("no staged CPU frame is available"),
            Self::ThreeD(error) => write!(formatter, "3D extraction failed: {error}"),
            Self::MissingActiveCamera => formatter.write_str("active World has no 2D camera"),
            Self::MultipleActiveCameras => {
                formatter.write_str("active World has more than one 2D camera")
            }
            Self::InvalidInterpolationAlpha { alpha } => write!(
                formatter,
                "interpolation alpha must be finite and within 0..=1, got {alpha}"
            ),
            Self::ForeignEntity { entity, expected } => write!(
                formatter,
                "visual entity {entity:?} does not belong to World generation {expected:?}"
            ),
            Self::InvalidVisual { entity, error } => {
                write!(formatter, "visual entity {entity:?} is invalid: {error}")
            }
            Self::InvalidInterpolatedTranslation { entity } => write!(
                formatter,
                "visual entity {entity:?} produced a non-finite interpolated translation"
            ),
            Self::InvalidResolvedRectangle { entity } => write!(
                formatter,
                "visual entity {entity:?} produced collapsed or non-finite rectangle bounds"
            ),
            Self::InvalidResolvedLine { entity } => write!(
                formatter,
                "visual entity {entity:?} produced a collapsed or non-finite line"
            ),
            Self::CircleLimitExceeded { limit } => {
                write!(
                    formatter,
                    "extracted circle count exceeds the limit of {limit}"
                )
            }
            Self::RectangleLimitExceeded { limit } => write!(
                formatter,
                "extracted rectangle count exceeds the limit of {limit}"
            ),
            Self::LineLimitExceeded { limit } => write!(
                formatter,
                "enabled line-source count exceeds the limit of {limit}"
            ),
            Self::ScreenRectangleLimitExceeded { limit } => write!(
                formatter,
                "extracted screen rectangle count exceeds the limit of {limit}"
            ),
            Self::ScreenImageLimitExceeded { limit } => write!(
                formatter,
                "extracted screen image count exceeds the limit of {limit}"
            ),
            #[cfg(feature = "text")]
            Self::ScreenTextLimitExceeded { limit } => write!(
                formatter,
                "extracted screen text count exceeds the limit of {limit}"
            ),
            #[cfg(feature = "text")]
            Self::ScreenTextBytesLimitExceeded { limit, requested } => write!(
                formatter,
                "screen text requests {requested} UTF-8 bytes, exceeding {limit}"
            ),
            #[cfg(feature = "text")]
            Self::ScreenTextGlyphLimitExceeded { limit, requested } => write!(
                formatter,
                "screen text requests {requested} shaped glyphs, exceeding {limit}"
            ),
            #[cfg(feature = "text")]
            Self::UnregisteredTextFont { entity, font } => write!(
                formatter,
                "screen text entity {entity:?} references unregistered font {font:?}"
            ),
            #[cfg(feature = "text")]
            Self::InvalidScreenText { entity, error } => write!(
                formatter,
                "screen text entity {entity:?} is invalid: {error}"
            ),
            Self::UnregisteredImageAsset { entity, image } => write!(
                formatter,
                "screen image entity {entity:?} references unregistered image {image:?}"
            ),
            Self::InvalidScreenImage { entity, error } => write!(
                formatter,
                "screen image entity {entity:?} is invalid: {error}"
            ),
            Self::InvalidScreenVisual { entity, error } => write!(
                formatter,
                "screen visual entity {entity:?} is invalid: {error}"
            ),
            Self::AllocationFailed { requested_bytes } => write!(
                formatter,
                "failed to reserve {requested_bytes} bytes for an extracted visual record"
            ),
            Self::Scene(error) => write!(formatter, "Sim;Engine scene extraction failed: {error}"),
            Self::ScreenScene(error) => {
                write!(formatter, "Sim;Engine screen extraction failed: {error}")
            }
        }
    }
}

impl Error for ExtractionError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ThreeD(error) => Some(error),
            Self::InvalidVisual { error, .. } => Some(error),
            Self::Scene(error) => Some(error),
            Self::ScreenScene(error) => Some(error),
            Self::InvalidScreenVisual { error, .. } => Some(error),
            Self::InvalidScreenImage { error, .. } => Some(error),
            #[cfg(feature = "text")]
            Self::InvalidScreenText { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// Copied ECS values consumed by the bounded extractor.
///
/// Keeping this adapter record private prevents raw Bevy identity from
/// becoming public render API while allowing the ECS query to feed extraction
/// without first allocating an unbounded intermediate collection.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CircleSource {
    entity: LogicEntity,
    previous_translation: Vec2,
    current_translation: Vec2,
    radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

/// Copied rectangle values consumed by the bounded extractor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RectangleSource {
    entity: LogicEntity,
    previous_translation: Vec2,
    current_translation: Vec2,
    size: Vec2,
    corner_radius: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

/// Copied anchored-line values consumed by the bounded extractor.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LineSource {
    entity: LogicEntity,
    previous_translation: Vec2,
    current_translation: Vec2,
    vector: Vec2,
    stroke_width_logical_pixels: f32,
    color: Color,
    layer: Layer,
    draw_order_depth: f32,
}

impl LineSource {
    pub(crate) fn new(entity: LogicEntity, transform: &Transform2d, visual: &LineVisual) -> Self {
        Self {
            entity,
            previous_translation: transform.previous_translation(),
            current_translation: transform.translation(),
            vector: visual.vector(),
            stroke_width_logical_pixels: visual.stroke_width_logical_pixels(),
            color: visual.color(),
            layer: visual.layer(),
            draw_order_depth: visual.draw_order_depth(),
        }
    }
}

impl RectangleSource {
    pub(crate) fn new(
        entity: LogicEntity,
        transform: &Transform2d,
        visual: &RectangleVisual,
    ) -> Self {
        Self {
            entity,
            previous_translation: transform.previous_translation(),
            current_translation: transform.translation(),
            size: visual.size(),
            corner_radius: visual.corner_radius(),
            color: visual.color(),
            layer: visual.layer(),
            draw_order_depth: visual.draw_order_depth(),
        }
    }
}

impl CircleSource {
    pub(crate) fn new(entity: LogicEntity, transform: &Transform2d, visual: &CircleVisual) -> Self {
        Self {
            entity,
            previous_translation: transform.previous_translation(),
            current_translation: transform.translation(),
            radius: visual.radius(),
            color: visual.color(),
            layer: visual.layer(),
            draw_order_depth: visual.draw_order_depth(),
        }
    }
}

/// Builds one complete bounded CPU visual snapshot for focused extractor tests.
#[cfg(test)]
pub(crate) fn extract_frame(
    parameters: ExtractionParameters,
    cameras: impl IntoIterator<Item = ActiveCamera2d>,
    circles: impl IntoIterator<Item = CircleSource>,
    rectangles: impl IntoIterator<Item = RectangleSource>,
    lines: impl IntoIterator<Item = LineSource>,
) -> Result<ExtractedFrame, ExtractionError> {
    let mut storage = ExtractionBuffer::new(parameters.limits)?;
    let metadata = extract_frame_into(
        &mut storage,
        parameters,
        cameras,
        circles,
        rectangles,
        lines,
        [],
    )?;
    Ok(metadata.with_storage(storage))
}

fn extract_frame_into(
    storage: &mut ExtractionBuffer,
    parameters: ExtractionParameters,
    cameras: impl IntoIterator<Item = ActiveCamera2d>,
    circles: impl IntoIterator<Item = CircleSource>,
    rectangles: impl IntoIterator<Item = RectangleSource>,
    lines: impl IntoIterator<Item = LineSource>,
    screen_rectangles: impl IntoIterator<Item = ScreenSource>,
) -> Result<ExtractedMetadata, ExtractionError> {
    let ExtractionParameters {
        world_generation,
        background,
        alpha,
        limits,
    } = parameters;
    debug_assert_eq!(
        storage.world_scene.budget(),
        Some(limits.world_scene_budget()),
        "an extraction buffer cannot cross a frozen render-limit boundary"
    );
    storage.resolved_circles.clear();
    storage.resolved_rectangles.clear();
    storage.resolved_lines.clear();
    storage.world_scene.clear();
    storage.screen.clear();
    storage.three_d.clear();

    if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
        return Err(ExtractionError::InvalidInterpolationAlpha { alpha });
    }

    let mut cameras = cameras.into_iter();
    let camera = cameras.next().ok_or(ExtractionError::MissingActiveCamera)?;
    if cameras.next().is_some() {
        return Err(ExtractionError::MultipleActiveCameras);
    }
    let camera = camera.interpolated_camera(alpha);

    for source in circles {
        if source.entity.world_generation() != world_generation {
            return Err(ExtractionError::ForeignEntity {
                entity: source.entity,
                expected: world_generation,
            });
        }
        if storage.resolved_circles.len() == limits.max_world_circles() {
            return Err(ExtractionError::CircleLimitExceeded {
                limit: limits.max_world_circles(),
            });
        }
        validate_aggregate_command_capacity(storage.resolved_circles.len(), limits)?;

        validate_source(source)?;
        let position = source
            .previous_translation
            .lerp(source.current_translation, alpha);
        if !position.is_finite() {
            return Err(ExtractionError::InvalidInterpolatedTranslation {
                entity: source.entity,
            });
        }

        storage
            .resolved_circles
            .try_reserve(1)
            .map_err(|_| ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ResolvedCircle>(),
            })?;
        storage.resolved_circles.push(ResolvedCircle {
            source: source.entity,
            position,
            radius: source.radius,
            color: source.color,
            layer: source.layer,
            draw_order_depth: source.draw_order_depth,
        });
    }

    for source in rectangles {
        if source.entity.world_generation() != world_generation {
            return Err(ExtractionError::ForeignEntity {
                entity: source.entity,
                expected: world_generation,
            });
        }
        if storage.resolved_rectangles.len() == limits.max_world_rectangles() {
            return Err(ExtractionError::RectangleLimitExceeded {
                limit: limits.max_world_rectangles(),
            });
        }
        validate_aggregate_command_capacity(
            storage
                .resolved_circles
                .len()
                .saturating_add(storage.resolved_rectangles.len()),
            limits,
        )?;

        validate_rectangle_source(source)?;
        let position = source
            .previous_translation
            .lerp(source.current_translation, alpha);
        if !position.is_finite() {
            return Err(ExtractionError::InvalidInterpolatedTranslation {
                entity: source.entity,
            });
        }
        let bounds = Rect::from_center_size(position, source.size);
        if !rectangle_bounds_are_representable(bounds) {
            return Err(ExtractionError::InvalidResolvedRectangle {
                entity: source.entity,
            });
        }

        storage.resolved_rectangles.try_reserve(1).map_err(|_| {
            ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ResolvedRectangle>(),
            }
        })?;
        storage.resolved_rectangles.push(ResolvedRectangle {
            source: source.entity,
            position,
            size: source.size,
            corner_radius: source.corner_radius,
            color: source.color,
            layer: source.layer,
            draw_order_depth: source.draw_order_depth,
        });
    }

    let mut line_sources_seen = 0usize;
    for source in lines {
        if source.entity.world_generation() != world_generation {
            return Err(ExtractionError::ForeignEntity {
                entity: source.entity,
                expected: world_generation,
            });
        }
        if line_sources_seen == limits.max_world_lines() {
            return Err(ExtractionError::LineLimitExceeded {
                limit: limits.max_world_lines(),
            });
        }
        line_sources_seen = line_sources_seen.saturating_add(1);

        validate_line_source(source)?;
        if source.vector.x() == 0.0 && source.vector.y() == 0.0 {
            continue;
        }
        validate_aggregate_command_capacity(
            storage
                .resolved_circles
                .len()
                .saturating_add(storage.resolved_rectangles.len())
                .saturating_add(storage.resolved_lines.len()),
            limits,
        )?;

        let from = source
            .previous_translation
            .lerp(source.current_translation, alpha);
        if !from.is_finite() {
            return Err(ExtractionError::InvalidInterpolatedTranslation {
                entity: source.entity,
            });
        }
        let to = from + source.vector;
        let delta = to - from;
        if !to.is_finite() || !delta.is_finite() || (delta.x() == 0.0 && delta.y() == 0.0) {
            return Err(ExtractionError::InvalidResolvedLine {
                entity: source.entity,
            });
        }

        storage
            .resolved_lines
            .try_reserve(1)
            .map_err(|_| ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ResolvedLine>(),
            })?;
        storage.resolved_lines.push(ResolvedLine {
            source: source.entity,
            from,
            to,
            stroke_width_logical_pixels: source.stroke_width_logical_pixels,
            color: source.color,
            layer: source.layer,
            draw_order_depth: source.draw_order_depth,
        });
    }

    storage.resolved_circles.sort_unstable_by(|left, right| {
        compare_visual_order(
            left.layer,
            left.draw_order_depth,
            left.source,
            right.layer,
            right.draw_order_depth,
            right.source,
        )
    });
    storage.resolved_rectangles.sort_unstable_by(|left, right| {
        compare_visual_order(
            left.layer,
            left.draw_order_depth,
            left.source,
            right.layer,
            right.draw_order_depth,
            right.source,
        )
    });
    storage.resolved_lines.sort_unstable_by(|left, right| {
        compare_visual_order(
            left.layer,
            left.draw_order_depth,
            left.source,
            right.layer,
            right.draw_order_depth,
            right.source,
        )
    });

    storage
        .world_scene
        .set_background(background)
        .map_err(ExtractionError::Scene)?;
    populate_scene(
        &mut storage.world_scene,
        &storage.resolved_circles,
        &storage.resolved_rectangles,
        &storage.resolved_lines,
    )?;
    storage
        .screen
        .extract(world_generation, limits, screen_rectangles)?;

    Ok(ExtractedMetadata {
        world_generation,
        background,
        camera,
    })
}

fn validate_aggregate_command_capacity(
    retained_visuals: usize,
    limits: RenderLimits,
) -> Result<(), ExtractionError> {
    let limit = limits.world_scene_budget().max_commands();
    if retained_visuals == limit {
        return Err(ExtractionError::Scene(SceneError::BudgetExceeded {
            resource: SceneBudgetResource::Commands,
            limit,
            requested: retained_visuals.saturating_add(1),
        }));
    }
    Ok(())
}

fn rectangle_bounds_are_representable(bounds: Rect) -> bool {
    bounds.min().is_finite()
        && bounds.max().is_finite()
        && bounds.width().is_finite()
        && bounds.height().is_finite()
        && bounds.width() > 0.0
        && bounds.height() > 0.0
}

fn compare_visual_order(
    left_layer: Layer,
    left_depth: f32,
    left_entity: LogicEntity,
    right_layer: Layer,
    right_depth: f32,
    right_entity: LogicEntity,
) -> Ordering {
    left_layer
        .cmp(&right_layer)
        .then_with(|| {
            if left_depth == right_depth {
                Ordering::Equal
            } else {
                left_depth.total_cmp(&right_depth)
            }
        })
        .then_with(|| left_entity.stable_bits().cmp(&right_entity.stable_bits()))
}

fn populate_scene(
    scene: &mut Scene,
    circles: &[ResolvedCircle],
    rectangles: &[ResolvedRectangle],
    lines: &[ResolvedLine],
) -> Result<(), ExtractionError> {
    if lines.is_empty() && rectangles.is_empty() {
        for circle in circles {
            push_circle(scene, circle)?;
        }
        return Ok(());
    }
    if lines.is_empty() && circles.is_empty() {
        for rectangle in rectangles {
            push_rectangle(scene, rectangle)?;
        }
        return Ok(());
    }
    if lines.is_empty() {
        let mut circle_index = 0;
        let mut rectangle_index = 0;
        while circle_index < circles.len() && rectangle_index < rectangles.len() {
            let circle = &circles[circle_index];
            let rectangle = &rectangles[rectangle_index];
            let order = compare_visual_order(
                rectangle.layer,
                rectangle.draw_order_depth,
                rectangle.source,
                circle.layer,
                circle.draw_order_depth,
                circle.source,
            );
            if order != Ordering::Greater {
                // Rectangle has the lower primitive rank for an otherwise exact tie.
                push_rectangle(scene, rectangle)?;
                rectangle_index += 1;
            } else {
                push_circle(scene, circle)?;
                circle_index += 1;
            }
        }
        for circle in &circles[circle_index..] {
            push_circle(scene, circle)?;
        }
        for rectangle in &rectangles[rectangle_index..] {
            push_rectangle(scene, rectangle)?;
        }
        return Ok(());
    }
    if circles.is_empty() && rectangles.is_empty() {
        for line in lines {
            push_line(scene, line)?;
        }
        return Ok(());
    }

    #[derive(Clone, Copy)]
    enum VisualKind {
        Rectangle,
        Circle,
        Line,
    }

    let mut circle_index = 0;
    let mut rectangle_index = 0;
    let mut line_index = 0;
    while circle_index < circles.len()
        || rectangle_index < rectangles.len()
        || line_index < lines.len()
    {
        let next = [
            rectangles.get(rectangle_index).map(|visual| {
                (
                    VisualKind::Rectangle,
                    visual.layer,
                    visual.draw_order_depth,
                    visual.source,
                    0_u8,
                )
            }),
            circles.get(circle_index).map(|visual| {
                (
                    VisualKind::Circle,
                    visual.layer,
                    visual.draw_order_depth,
                    visual.source,
                    1_u8,
                )
            }),
            lines.get(line_index).map(|visual| {
                (
                    VisualKind::Line,
                    visual.layer,
                    visual.draw_order_depth,
                    visual.source,
                    2_u8,
                )
            }),
        ]
        .into_iter()
        .flatten()
        .min_by(|left, right| {
            compare_visual_order(left.1, left.2, left.3, right.1, right.2, right.3)
                .then_with(|| left.4.cmp(&right.4))
        });
        let Some(next) = next else {
            break;
        };

        match next.0 {
            VisualKind::Rectangle => {
                push_rectangle(scene, &rectangles[rectangle_index])?;
                rectangle_index += 1;
            }
            VisualKind::Circle => {
                push_circle(scene, &circles[circle_index])?;
                circle_index += 1;
            }
            VisualKind::Line => {
                push_line(scene, &lines[line_index])?;
                line_index += 1;
            }
        }
    }
    Ok(())
}

fn push_circle(scene: &mut Scene, circle: &ResolvedCircle) -> Result<(), ExtractionError> {
    scene
        .try_circle_on_layer(
            circle.layer,
            circle.position,
            circle.radius,
            ShapeStyle::filled(circle.color),
        )
        .map_err(ExtractionError::Scene)
}

fn push_rectangle(scene: &mut Scene, rectangle: &ResolvedRectangle) -> Result<(), ExtractionError> {
    scene
        .try_rect_on_layer(
            rectangle.layer,
            Rect::from_center_size(rectangle.position, rectangle.size),
            rectangle.corner_radius,
            ShapeStyle::filled(rectangle.color),
        )
        .map_err(ExtractionError::Scene)
}

fn push_line(scene: &mut Scene, line: &ResolvedLine) -> Result<(), ExtractionError> {
    scene
        .try_styled_line_on_layer(
            line.layer,
            line.from,
            line.to,
            line_stroke_style(line.stroke_width_logical_pixels, line.color),
        )
        .map_err(ExtractionError::Scene)
}

fn validate_source(source: CircleSource) -> Result<(), ExtractionError> {
    crate::visual::validate_translation(source.previous_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_translation(source.current_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_radius(source.radius).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_color(source.color).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_draw_order_depth(source.draw_order_depth).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    Ok(())
}

fn validate_rectangle_source(source: RectangleSource) -> Result<(), ExtractionError> {
    crate::visual::validate_translation(source.previous_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_translation(source.current_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_size(source.size).map_err(|error| ExtractionError::InvalidVisual {
        entity: source.entity,
        error,
    })?;
    crate::visual::validate_corner_radius(source.corner_radius).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_color(source.color).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_draw_order_depth(source.draw_order_depth).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    Ok(())
}

fn validate_line_source(source: LineSource) -> Result<(), ExtractionError> {
    crate::visual::validate_translation(source.previous_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_translation(source.current_translation).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_line_vector(source.vector).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_line_width(source.stroke_width_logical_pixels).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_color(source.color).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    crate::visual::validate_draw_order_depth(source.draw_order_depth).map_err(|error| {
        ExtractionError::InvalidVisual {
            entity: source.entity,
            error,
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{identity::ApplicationId, render::DEFAULT_WORLD_SCENE_BUDGET};
    use bevy_ecs::world::World;
    use sim_engine::{DrawCommand, StrokeCap2d, StrokeWidthMode2d};

    fn generation(sequence: u64) -> WorldGeneration {
        WorldGeneration::new(ApplicationId::from_raw(1), sequence)
    }

    fn entity(world: &mut World, generation: WorldGeneration) -> LogicEntity {
        let raw = world.spawn_empty().id();
        LogicEntity::new(ApplicationId::from_raw(1), generation, raw)
    }

    fn camera() -> Result<ActiveCamera2d, sim_engine::Camera2dError> {
        Camera2d::new(Vec2::ZERO, 32.0).map(ActiveCamera2d::new)
    }

    fn source(
        entity: LogicEntity,
        previous: Vec2,
        current: Vec2,
        layer: i32,
        depth: f32,
    ) -> CircleSource {
        CircleSource {
            entity,
            previous_translation: previous,
            current_translation: current,
            radius: 2.0,
            color: Color::WHITE,
            layer: Layer::new(layer),
            draw_order_depth: depth,
        }
    }

    fn rectangle_source(
        entity: LogicEntity,
        previous: Vec2,
        current: Vec2,
        size: Vec2,
        layer: i32,
        depth: f32,
    ) -> RectangleSource {
        RectangleSource {
            entity,
            previous_translation: previous,
            current_translation: current,
            size,
            corner_radius: 0.25,
            color: Color::WHITE,
            layer: Layer::new(layer),
            draw_order_depth: depth,
        }
    }

    fn line_source(
        entity: LogicEntity,
        previous: Vec2,
        current: Vec2,
        vector: Vec2,
        layer: i32,
        depth: f32,
    ) -> LineSource {
        LineSource {
            entity,
            previous_translation: previous,
            current_translation: current,
            vector,
            stroke_width_logical_pixels: 2.5,
            color: Color::rgb8(255, 160, 40),
            layer: Layer::new(layer),
            draw_order_depth: depth,
        }
    }

    #[test]
    fn interpolation_covers_zero_middle_and_just_below_one() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);
        let previous = Vec2::new(0.0, 4.0);
        let current = Vec2::new(8.0, 12.0);
        let just_below_one = f32::from_bits(1.0_f32.to_bits() - 1);

        for (alpha, expected) in [
            (0.0, previous),
            (0.5, Vec2::new(4.0, 8.0)),
            (just_below_one, previous.lerp(current, just_below_one)),
        ] {
            let frame = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, alpha, RenderLimits::default()),
                [camera()?],
                [source(entity, previous, current, 0, 0.0)],
                [],
                [],
            )?;

            assert_eq!(frame.resolved_circles()[0].position(), expected);
            assert_eq!(frame.world_scene().command_count(), 1);
        }
        Ok(())
    }

    #[test]
    fn rectangle_interpolation_and_scene_bounds_match() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);
        let previous = Vec2::new(-4.0, 2.0);
        let current = Vec2::new(8.0, 6.0);
        let size = Vec2::new(6.0, 2.0);

        for (alpha, expected) in [(0.0, previous), (0.5, Vec2::new(2.0, 4.0)), (1.0, current)] {
            let frame = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, alpha, RenderLimits::default()),
                [camera()?],
                [],
                [rectangle_source(entity, previous, current, size, 0, 0.0)],
                [],
            )?;
            let [resolved] = frame.resolved_rectangles() else {
                return Err("expected one resolved rectangle".into());
            };
            assert_eq!(resolved.position(), expected);
            assert_eq!(resolved.size(), size);
            assert_eq!(resolved.corner_radius(), 0.25);

            let [command] = frame.world_scene().commands() else {
                return Err("expected one rectangle command".into());
            };
            let DrawCommand::Rect(rectangle) = command.command() else {
                return Err("resolved rectangle produced another command kind".into());
            };
            assert_eq!(rectangle.rect().min(), expected - size * 0.5);
            assert_eq!(rectangle.rect().max(), expected + size * 0.5);
            assert_eq!(rectangle.corner_radius(), 0.25);
        }
        Ok(())
    }

    #[test]
    fn line_interpolates_only_its_anchor_and_uses_an_exact_butt_stroke()
    -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);
        let previous = Vec2::new(-4.0, 2.0);
        let current = Vec2::new(8.0, 6.0);
        let vector = Vec2::new(3.0, -1.0);
        let color = Color::rgb8(255, 160, 40);

        let frame = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.5, RenderLimits::default()),
            [camera()?],
            [],
            [],
            [line_source(entity, previous, current, vector, 4, -2.0)],
        )?;
        let [resolved] = frame.resolved_lines() else {
            return Err("expected one resolved line".into());
        };
        assert_eq!(resolved.source(), entity);
        assert_eq!(resolved.from(), Vec2::new(2.0, 4.0));
        assert_eq!(resolved.to(), Vec2::new(5.0, 3.0));
        assert_eq!(resolved.stroke_width_logical_pixels(), 2.5);
        assert_eq!(resolved.color(), color);
        assert_eq!(resolved.layer(), Layer::new(4));
        assert_eq!(resolved.draw_order_depth(), -2.0);

        let [command] = frame.world_scene().commands() else {
            return Err("expected one line command".into());
        };
        assert_eq!(command.layer(), Layer::new(4));
        let DrawCommand::Line(line) = command.command() else {
            return Err("resolved line produced another command kind".into());
        };
        assert_eq!(line.from(), resolved.from());
        assert_eq!(line.to(), resolved.to());
        assert_eq!(line.stroke().width(), 2.5);
        assert_eq!(line.stroke().color(), color);
        assert_eq!(
            line.stroke_style().width_mode(),
            StrokeWidthMode2d::LogicalPixels
        );
        assert_eq!(line.stroke_style().cap(), StrokeCap2d::Butt);
        assert_eq!(line.stroke_style().start_marker(), None);
        assert_eq!(line.stroke_style().end_marker(), None);
        Ok(())
    }

    #[test]
    fn zero_lines_are_omitted_but_still_count_toward_the_source_limit() -> Result<(), Box<dyn Error>>
    {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let limits = RenderLimits::default().with_max_world_lines(1);

        let frame = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [],
            [],
            [line_source(
                first,
                Vec2::ZERO,
                Vec2::ZERO,
                Vec2::ZERO,
                0,
                0.0,
            )],
        )?;
        assert!(frame.resolved_lines().is_empty());
        assert_eq!(frame.world_scene().command_count(), 0);

        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [],
            [],
            [
                line_source(first, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
                line_source(second, Vec2::ZERO, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
            ],
        );
        assert!(matches!(
            result,
            Err(ExtractionError::LineLimitExceeded { limit: 1 })
        ));
        Ok(())
    }

    #[test]
    fn line_resolution_keeps_representable_subnormal_and_maximum_vectors()
    -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);

        for vector in [Vec2::new(f32::from_bits(1), 0.0), Vec2::new(f32::MAX, 0.0)] {
            let frame = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
                [camera()?],
                [],
                [],
                [line_source(entity, Vec2::ZERO, Vec2::ZERO, vector, 0, 0.0)],
            )?;
            let [line] = frame.resolved_lines() else {
                return Err("representable nonzero vector should resolve".into());
            };
            assert_eq!(line.from(), Vec2::ZERO);
            assert_eq!(line.to().x().to_bits(), vector.x().to_bits());
            assert_eq!(line.to().y().to_bits(), vector.y().to_bits());
        }
        Ok(())
    }

    #[test]
    fn camera_center_uses_the_same_interpolation_alpha() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let previous = Vec2::new(-4.0, 2.0);
        let current = Vec2::new(8.0, 6.0);
        let mut configured = Camera2d::new(previous, 24.0)?;
        configured.set_rotation(0.75)?;
        configured.set_projection(sim_engine::Projection2d::new(0.25, 2.0)?);
        let mut camera = ActiveCamera2d::new(configured);
        camera.set_center(current)?;

        for (alpha, expected) in [(0.0, previous), (0.5, Vec2::new(2.0, 4.0)), (1.0, current)] {
            let frame = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, alpha, RenderLimits::default()),
                [camera],
                [],
                [],
                [],
            )?;

            assert_eq!(frame.camera().center(), expected);
            assert_eq!(frame.camera().zoom(), 24.0);
            assert_eq!(frame.camera().rotation(), 0.75);
            assert_eq!(
                frame.camera().projection(),
                sim_engine::Projection2d::new(0.25, 2.0)?
            );
            assert_eq!(camera.center(), current);
            assert_eq!(camera.previous_center(), previous);
        }
        Ok(())
    }

    #[test]
    fn sorting_uses_layer_then_depth_then_stable_entity() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let third = entity(&mut world, generation);
        let frame = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
            [camera()?],
            [
                source(third, Vec2::new(3.0, 0.0), Vec2::new(3.0, 0.0), 1, 0.0),
                source(second, Vec2::new(2.0, 0.0), Vec2::new(2.0, 0.0), 0, 1.0),
                source(first, Vec2::new(1.0, 0.0), Vec2::new(1.0, 0.0), 0, 1.0),
            ],
            [],
            [
                line_source(
                    third,
                    Vec2::new(3.0, 0.0),
                    Vec2::new(3.0, 0.0),
                    Vec2::ONE,
                    1,
                    0.0,
                ),
                line_source(
                    second,
                    Vec2::new(2.0, 0.0),
                    Vec2::new(2.0, 0.0),
                    Vec2::ONE,
                    0,
                    1.0,
                ),
                line_source(
                    first,
                    Vec2::new(1.0, 0.0),
                    Vec2::new(1.0, 0.0),
                    Vec2::ONE,
                    0,
                    1.0,
                ),
            ],
        )?;

        let ordered_sources: Vec<_> = frame
            .resolved_circles()
            .iter()
            .map(|circle| circle.source())
            .collect();
        let mut same_layer = [first, second];
        same_layer.sort_by_key(|entity| entity.stable_bits());
        let expected_sources = vec![same_layer[0], same_layer[1], third];
        assert_eq!(ordered_sources, expected_sources);
        let ordered_lines: Vec<_> = frame
            .resolved_lines()
            .iter()
            .map(|line| line.source())
            .collect();
        assert_eq!(ordered_lines, expected_sources);

        let scene_centers: Vec<_> = frame
            .world_scene()
            .commands()
            .iter()
            .filter_map(|command| match command.command() {
                DrawCommand::Circle(circle) => Some(circle.center()),
                _ => None,
            })
            .collect();
        let expected_center = |entity: LogicEntity| {
            if entity == first {
                Vec2::new(1.0, 0.0)
            } else {
                Vec2::new(2.0, 0.0)
            }
        };
        assert_eq!(
            scene_centers,
            vec![
                expected_center(same_layer[0]),
                expected_center(same_layer[1]),
                Vec2::new(3.0, 0.0)
            ]
        );
        Ok(())
    }

    #[test]
    fn mixed_scene_uses_one_total_order_with_rectangle_circle_line_on_exact_tie()
    -> Result<(), Box<dyn Error>> {
        #[derive(Debug, PartialEq)]
        enum Observed {
            Circle(Vec2),
            Rectangle(Vec2),
            Line(Vec2, Vec2),
        }

        let generation = generation(1);
        let mut world = World::new();
        let tied = entity(&mut world, generation);
        let early = entity(&mut world, generation);
        let late = entity(&mut world, generation);
        let frame = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
            [camera()?],
            [
                source(late, Vec2::new(30.0, 0.0), Vec2::new(30.0, 0.0), 1, 0.0),
                source(tied, Vec2::new(20.0, 0.0), Vec2::new(20.0, 0.0), 0, 1.0),
            ],
            [
                rectangle_source(
                    tied,
                    Vec2::new(20.0, 0.0),
                    Vec2::new(20.0, 0.0),
                    Vec2::ONE,
                    0,
                    1.0,
                ),
                rectangle_source(
                    early,
                    Vec2::new(10.0, 0.0),
                    Vec2::new(10.0, 0.0),
                    Vec2::ONE,
                    0,
                    0.0,
                ),
            ],
            [line_source(
                tied,
                Vec2::new(20.0, 0.0),
                Vec2::new(20.0, 0.0),
                Vec2::new(2.0, 0.0),
                0,
                1.0,
            )],
        )?;

        let observed: Vec<_> = frame
            .world_scene()
            .commands()
            .iter()
            .map(|command| match command.command() {
                DrawCommand::Circle(circle) => Observed::Circle(circle.center()),
                DrawCommand::Rect(rectangle) => Observed::Rectangle(rectangle.rect().center()),
                DrawCommand::Line(line) => Observed::Line(line.from(), line.to()),
                _ => unreachable!("extractor emitted an unsupported command kind"),
            })
            .collect();
        assert_eq!(
            observed,
            [
                Observed::Rectangle(Vec2::new(10.0, 0.0)),
                Observed::Rectangle(Vec2::new(20.0, 0.0)),
                Observed::Circle(Vec2::new(20.0, 0.0)),
                Observed::Line(Vec2::new(20.0, 0.0), Vec2::new(22.0, 0.0)),
                Observed::Circle(Vec2::new(30.0, 0.0)),
            ]
        );

        for (circle_depth, rectangle_depth) in [(-0.0, 0.0), (0.0, -0.0)] {
            let frame = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
                [camera()?],
                [source(tied, Vec2::ZERO, Vec2::ZERO, 0, circle_depth)],
                [rectangle_source(
                    tied,
                    Vec2::ZERO,
                    Vec2::ZERO,
                    Vec2::ONE,
                    0,
                    rectangle_depth,
                )],
                [line_source(
                    tied,
                    Vec2::ZERO,
                    Vec2::ZERO,
                    Vec2::ONE,
                    0,
                    circle_depth,
                )],
            )?;
            let [first, second, third] = frame.world_scene().commands() else {
                return Err("signed-zero tie should produce three commands".into());
            };
            assert!(matches!(first.command(), DrawCommand::Rect(_)));
            assert!(matches!(second.command(), DrawCommand::Circle(_)));
            assert!(matches!(third.command(), DrawCommand::Line(_)));
        }
        Ok(())
    }

    #[test]
    fn circle_count_is_bounded_before_an_extra_record_is_retained() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let limits = RenderLimits::new(1, DEFAULT_WORLD_SCENE_BUDGET, Default::default());

        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [
                source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
                source(second, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
            ],
            [],
            [],
        );

        assert!(matches!(
            result,
            Err(ExtractionError::CircleLimitExceeded { limit: 1 })
        ));
        Ok(())
    }

    #[test]
    fn rectangle_and_aggregate_command_limits_are_distinct() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let rectangle_limit = RenderLimits::default().with_max_world_rectangles(1);
        let rectangles = [
            rectangle_source(first, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
            rectangle_source(second, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
        ];
        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, rectangle_limit),
            [camera()?],
            [],
            rectangles,
            [],
        );
        assert!(matches!(
            result,
            Err(ExtractionError::RectangleLimitExceeded { limit: 1 })
        ));

        let standard = DEFAULT_WORLD_SCENE_BUDGET;
        let one_command = sim_engine::SceneBudget::new(
            1,
            standard.max_points(),
            standard.max_tessellated_vertices(),
            standard.max_retained_bytes(),
            standard.max_allocation_bytes(),
            standard.max_upload_bytes(),
            standard.max_draw_batches(),
        );
        let aggregate_limits =
            RenderLimits::new(1, one_command, Default::default()).with_max_world_rectangles(1);
        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, aggregate_limits),
            [camera()?],
            [source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [rectangle_source(
                second,
                Vec2::ZERO,
                Vec2::ZERO,
                Vec2::ONE,
                0,
                0.0,
            )],
            [],
        );
        assert!(matches!(
            result,
            Err(ExtractionError::Scene(SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands,
                limit: 1,
                requested: 2,
            }))
        ));
        Ok(())
    }

    #[test]
    fn line_and_aggregate_command_limits_are_distinct() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let line_limit = RenderLimits::default().with_max_world_lines(1);
        let lines = [
            line_source(first, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
            line_source(second, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
        ];
        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, line_limit),
            [camera()?],
            [],
            [],
            lines,
        );
        assert!(matches!(
            result,
            Err(ExtractionError::LineLimitExceeded { limit: 1 })
        ));

        let standard = DEFAULT_WORLD_SCENE_BUDGET;
        let one_command = sim_engine::SceneBudget::new(
            1,
            standard.max_points(),
            standard.max_tessellated_vertices(),
            standard.max_retained_bytes(),
            standard.max_allocation_bytes(),
            standard.max_upload_bytes(),
            standard.max_draw_batches(),
        );
        let limits = RenderLimits::new(1, one_command, Default::default()).with_max_world_lines(1);
        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [line_source(
                second,
                Vec2::ZERO,
                Vec2::ZERO,
                Vec2::ONE,
                0,
                0.0,
            )],
        );
        assert!(matches!(
            result,
            Err(ExtractionError::Scene(SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands,
                limit: 1,
                requested: 2,
            }))
        ));
        Ok(())
    }

    #[test]
    fn collapsed_or_overflowed_rectangle_bounds_keep_entity_attribution()
    -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let collapsed = entity(&mut world, generation);
        let overflowed = entity(&mut world, generation);

        for source in [
            rectangle_source(
                collapsed,
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                Vec2::ONE,
                0,
                0.0,
            ),
            rectangle_source(
                overflowed,
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                0,
                0.0,
            ),
        ] {
            let result = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
                [camera()?],
                [],
                [source],
                [],
            );
            assert!(matches!(
                result,
                Err(ExtractionError::InvalidResolvedRectangle { entity })
                    if entity == source.entity
            ));
        }
        Ok(())
    }

    #[test]
    fn collapsed_or_overflowed_line_endpoints_keep_entity_attribution() -> Result<(), Box<dyn Error>>
    {
        let generation = generation(1);
        let mut world = World::new();
        let collapsed = entity(&mut world, generation);
        let overflowed = entity(&mut world, generation);

        for source in [
            line_source(
                collapsed,
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                Vec2::ONE,
                0,
                0.0,
            ),
            line_source(
                overflowed,
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                Vec2::splat(f32::MAX),
                0,
                0.0,
            ),
        ] {
            let result = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
                [camera()?],
                [],
                [],
                [source],
            );
            assert!(matches!(
                result,
                Err(ExtractionError::InvalidResolvedLine { entity })
                    if entity == source.entity
            ));
        }
        Ok(())
    }

    #[test]
    fn foreign_generation_is_rejected() -> Result<(), Box<dyn Error>> {
        let active = generation(1);
        let stale = generation(2);
        let mut world = World::new();
        let stale_entity = entity(&mut world, stale);

        let result = extract_frame(
            ExtractionParameters::new(active, Color::BLACK, 0.0, RenderLimits::default()),
            [camera()?],
            [source(stale_entity, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [],
        );

        assert!(matches!(
            result,
            Err(ExtractionError::ForeignEntity { entity, expected })
                if entity == stale_entity && expected == active
        ));
        Ok(())
    }

    #[test]
    fn defensive_visual_validation_precedes_scene_publication() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);
        let invalid = CircleSource {
            radius: f32::NAN,
            ..source(entity, Vec2::ZERO, Vec2::ZERO, 0, 0.0)
        };

        let result = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.5, RenderLimits::default()),
            [camera()?],
            [invalid],
            [],
            [],
        );

        assert!(matches!(
            result,
            Err(ExtractionError::InvalidVisual {
                error: VisualValueError::InvalidRadius { .. },
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn defensive_line_validation_preserves_entity_attribution() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let entity = entity(&mut world, generation);

        for invalid in [
            LineSource {
                vector: Vec2::new(f32::NAN, 0.0),
                ..line_source(entity, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0)
            },
            LineSource {
                stroke_width_logical_pixels: f32::MAX,
                ..line_source(entity, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0)
            },
        ] {
            let result = extract_frame(
                ExtractionParameters::new(generation, Color::BLACK, 0.0, RenderLimits::default()),
                [camera()?],
                [],
                [],
                [invalid],
            );
            assert!(matches!(
                result,
                Err(ExtractionError::InvalidVisual {
                    entity: rejected,
                    error: VisualValueError::InvalidLineVector { .. }
                        | VisualValueError::InvalidLineWidth { .. },
                }) if rejected == entity
            ));
        }
        Ok(())
    }

    #[test]
    fn camera_cardinality_is_checked() -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let limits = RenderLimits::default();
        let missing = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [],
            [],
            [],
            [],
        );
        assert!(matches!(missing, Err(ExtractionError::MissingActiveCamera)));

        let multiple = extract_frame(
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?, camera()?],
            [],
            [],
            [],
        );
        assert!(matches!(
            multiple,
            Err(ExtractionError::MultipleActiveCameras)
        ));

        Ok(())
    }

    #[test]
    fn reusable_storage_clears_logical_state_without_losing_fixed_record_capacity()
    -> Result<(), Box<dyn Error>> {
        let generation = generation(1);
        let mut world = World::new();
        let first = entity(&mut world, generation);
        let second = entity(&mut world, generation);
        let third = entity(&mut world, generation);
        let fourth = entity(&mut world, generation);
        let fifth = entity(&mut world, generation);
        let limits = RenderLimits::default();
        let mut storage = ExtractionBuffer::new(limits)?;

        let first_metadata = extract_frame_into(
            &mut storage,
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [
                source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
                source(second, Vec2::ONE, Vec2::ONE, 0, 1.0),
            ],
            [
                rectangle_source(third, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 2.0),
                rectangle_source(fourth, Vec2::ONE, Vec2::ONE, Vec2::ONE, 0, 3.0),
            ],
            [line_source(
                fifth,
                Vec2::new(2.0, 4.0),
                Vec2::new(2.0, 4.0),
                Vec2::new(3.0, -1.0),
                0,
                4.0,
            )],
            [],
        )?;
        assert_eq!(first_metadata.background, Color::BLACK);
        let circle_capacity = storage.resolved_circles.capacity();
        let rectangle_capacity = storage.resolved_rectangles.capacity();
        let line_capacity = storage.resolved_lines.capacity();
        let circle_address = storage.resolved_circles.as_ptr();
        let rectangle_address = storage.resolved_rectangles.as_ptr();
        let line_address = storage.resolved_lines.as_ptr();
        let scene_address = storage.world_scene.commands().as_ptr();
        let scene_allocation = storage.world_scene.allocation_bytes();

        let moved_camera = ActiveCamera2d::new(Camera2d::new(Vec2::new(7.0, -4.0), 20.0)?);
        let second_metadata = extract_frame_into(
            &mut storage,
            ExtractionParameters::new(generation, Color::WHITE, 0.0, limits),
            [moved_camera],
            [source(first, Vec2::ONE, Vec2::ONE, 0, 0.0)],
            [],
            [],
            [],
        )?;
        assert_eq!(second_metadata.background, Color::WHITE);
        assert_eq!(second_metadata.camera.center(), Vec2::new(7.0, -4.0));
        assert_eq!(storage.resolved_circles.len(), 1);
        assert!(storage.resolved_rectangles.is_empty());
        assert!(storage.resolved_lines.is_empty());
        assert_eq!(storage.world_scene.command_count(), 1);
        assert_eq!(storage.world_scene.background(), Color::WHITE);
        assert_eq!(storage.resolved_circles.capacity(), circle_capacity);
        assert_eq!(storage.resolved_rectangles.capacity(), rectangle_capacity);
        assert_eq!(storage.resolved_lines.capacity(), line_capacity);
        assert_eq!(storage.resolved_circles.as_ptr(), circle_address);
        assert_eq!(storage.resolved_rectangles.as_ptr(), rectangle_address);
        assert_eq!(storage.resolved_lines.as_ptr(), line_address);
        assert_eq!(storage.world_scene.commands().as_ptr(), scene_address);
        assert_eq!(storage.world_scene.allocation_bytes(), scene_allocation);

        let empty_metadata = extract_frame_into(
            &mut storage,
            ExtractionParameters::new(generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [],
            [],
            [],
            [],
        )?;
        assert_eq!(empty_metadata.background, Color::BLACK);
        assert!(storage.resolved_circles.is_empty());
        assert!(storage.resolved_rectangles.is_empty());
        assert!(storage.resolved_lines.is_empty());
        assert_eq!(storage.world_scene.command_count(), 0);
        assert_eq!(storage.world_scene.statistics().accepted_commands(), 0);
        assert_eq!(storage.world_scene.background(), Color::BLACK);
        assert_eq!(storage.resolved_circles.capacity(), circle_capacity);
        assert_eq!(storage.resolved_rectangles.capacity(), rectangle_capacity);
        assert_eq!(storage.resolved_lines.capacity(), line_capacity);
        assert_eq!(storage.resolved_circles.as_ptr(), circle_address);
        assert_eq!(storage.resolved_rectangles.as_ptr(), rectangle_address);
        assert_eq!(storage.resolved_lines.as_ptr(), line_address);
        assert_eq!(storage.world_scene.commands().as_ptr(), scene_address);
        assert_eq!(storage.world_scene.allocation_bytes(), scene_allocation);
        Ok(())
    }

    #[test]
    fn invalid_line_staging_preserves_publication_and_retry_clears_partial_records()
    -> Result<(), Box<dyn Error>> {
        let first_generation = generation(1);
        let next_generation = generation(2);
        let mut world = World::new();
        let first = entity(&mut world, first_generation);
        let valid = entity(&mut world, next_generation);
        let invalid = entity(&mut world, next_generation);
        let limits = RenderLimits::default();
        let mut buffers = ExtractionBuffers::new();

        buffers.stage_world_only(
            ExtractionParameters::new(first_generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(first_generation));

        let result = buffers.stage_world_only(
            ExtractionParameters::new(next_generation, Color::WHITE, 0.0, limits),
            [camera()?],
            [],
            [],
            [
                line_source(valid, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
                line_source(
                    invalid,
                    Vec2::splat(f32::MAX),
                    Vec2::splat(f32::MAX),
                    Vec2::ONE,
                    0,
                    1.0,
                ),
            ],
        );
        assert!(matches!(
            result,
            Err(ExtractionError::InvalidResolvedLine { entity }) if entity == invalid
        ));
        let partial = buffers
            .spare
            .as_ref()
            .ok_or("failed line staging should retain its private spare")?;
        assert_eq!(partial.resolved_lines.len(), 1);
        assert_eq!(partial.resolved_lines[0].source(), valid);
        assert_eq!(partial.world_scene.command_count(), 0);

        let published = buffers
            .published()
            .ok_or("failed line staging should preserve the published frame")?;
        assert_eq!(published.world_generation(), first_generation);
        assert_eq!(published.resolved_circles().len(), 1);
        assert!(published.resolved_lines().is_empty());

        buffers.stage_world_only(
            ExtractionParameters::new(next_generation, Color::WHITE, 0.0, limits),
            [camera()?],
            [],
            [],
            [line_source(
                valid,
                Vec2::new(2.0, 3.0),
                Vec2::new(2.0, 3.0),
                Vec2::new(-1.0, 4.0),
                0,
                0.0,
            )],
        )?;
        assert_eq!(buffers.publish(), Some(next_generation));
        let published = buffers
            .published()
            .ok_or("valid line retry should publish")?;
        assert!(published.resolved_circles().is_empty());
        let [line] = published.resolved_lines() else {
            return Err("valid line retry should contain one fresh line".into());
        };
        assert_eq!(line.source(), valid);
        assert_eq!(line.from(), Vec2::new(2.0, 3.0));
        assert_eq!(line.to(), Vec2::new(1.0, 7.0));
        assert_eq!(published.world_scene().command_count(), 1);
        Ok(())
    }

    #[test]
    fn downstream_line_scene_failure_keeps_the_last_complete_snapshot() -> Result<(), Box<dyn Error>>
    {
        let first_generation = generation(1);
        let failed_generation = generation(2);
        let mut world = World::new();
        let first = entity(&mut world, first_generation);
        let failed_first = entity(&mut world, failed_generation);
        let failed_second = entity(&mut world, failed_generation);
        let defaults = RenderLimits::default();
        let scene = defaults.world_scene_budget();
        let one_draw_batch = sim_engine::SceneBudget::new(
            scene.max_commands(),
            scene.max_points(),
            scene.max_tessellated_vertices(),
            scene.max_retained_bytes(),
            scene.max_allocation_bytes(),
            scene.max_upload_bytes(),
            1,
        );
        let limits = RenderLimits::new(
            defaults.max_world_circles(),
            one_draw_batch,
            defaults.frame_limits(),
        )
        .with_max_world_rectangles(defaults.max_world_rectangles())
        .with_max_world_lines(defaults.max_world_lines());
        let mut buffers = ExtractionBuffers::new();

        buffers.stage_world_only(
            ExtractionParameters::new(first_generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(first_generation));

        let result = buffers.stage_world_only(
            ExtractionParameters::new(failed_generation, Color::WHITE, 0.0, limits),
            [camera()?],
            [],
            [],
            [
                line_source(failed_first, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 0, 0.0),
                line_source(failed_second, Vec2::ZERO, Vec2::ZERO, Vec2::ONE, 1, 0.0),
            ],
        );
        assert!(matches!(result, Err(ExtractionError::Scene(_))));
        let partial = buffers
            .spare
            .as_ref()
            .ok_or("failed Scene staging should retain its private spare")?;
        assert_eq!(partial.resolved_lines.len(), 2);
        assert_eq!(partial.world_scene.command_count(), 1);
        assert_eq!(partial.world_scene.statistics().requested_commands(), 2);
        assert_eq!(partial.world_scene.statistics().accepted_commands(), 1);
        assert_eq!(partial.world_scene.statistics().rejected_commands(), 1);
        assert_eq!(partial.world_scene.statistics().estimated_draw_batches(), 1);

        let published = buffers
            .published()
            .ok_or("failed Scene staging should preserve the published frame")?;
        assert_eq!(published.world_generation(), first_generation);
        assert_eq!(published.resolved_circles().len(), 1);
        assert!(published.resolved_lines().is_empty());
        Ok(())
    }

    #[test]
    fn failed_staging_preserves_published_frame_and_retry_has_no_partial_records()
    -> Result<(), Box<dyn Error>> {
        let first_generation = generation(1);
        let failed_generation = generation(2);
        let recovered_generation = generation(3);
        let mut world = World::new();
        let first = entity(&mut world, first_generation);
        let failed_first = entity(&mut world, failed_generation);
        let failed_second = entity(&mut world, failed_generation);
        let recovered = entity(&mut world, recovered_generation);
        let defaults = RenderLimits::default();
        let scene = defaults.world_scene_budget();
        let one_draw_batch = sim_engine::SceneBudget::new(
            scene.max_commands(),
            scene.max_points(),
            scene.max_tessellated_vertices(),
            scene.max_retained_bytes(),
            scene.max_allocation_bytes(),
            scene.max_upload_bytes(),
            1,
        );
        let limits = RenderLimits::new(
            defaults.max_world_circles(),
            one_draw_batch,
            defaults.frame_limits(),
        )
        .with_max_world_rectangles(defaults.max_world_rectangles());
        let mut buffers = ExtractionBuffers::new();

        buffers.stage_world_only(
            ExtractionParameters::new(first_generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(first, Vec2::ONE, Vec2::ONE, 0, 0.0)],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(first_generation));

        let result = buffers.stage_world_only(
            ExtractionParameters::new(failed_generation, Color::WHITE, 0.0, limits),
            [ActiveCamera2d::new(Camera2d::new(Vec2::ONE, 20.0)?)],
            [
                source(failed_first, Vec2::ZERO, Vec2::ZERO, 0, 0.0),
                source(
                    failed_second,
                    Vec2::splat(f32::MAX),
                    Vec2::splat(f32::MAX),
                    1,
                    1.0,
                ),
            ],
            [],
            [],
        );
        assert!(matches!(result, Err(ExtractionError::Scene(_))));
        let partial = buffers
            .spare
            .as_ref()
            .ok_or("failed staging should retain its private spare")?;
        assert_eq!(partial.resolved_circles.len(), 2);
        assert_eq!(partial.world_scene.command_count(), 1);
        assert_eq!(partial.world_scene.statistics().requested_commands(), 2);
        assert_eq!(partial.world_scene.statistics().accepted_commands(), 1);
        let published = buffers
            .published()
            .ok_or("failed staging should preserve the published frame")?;
        assert_eq!(published.world_generation(), first_generation);
        assert_eq!(published.background(), Color::BLACK);
        assert_eq!(published.camera().center(), Vec2::ZERO);
        assert_eq!(published.resolved_circles().len(), 1);
        assert_eq!(published.resolved_circles()[0].source(), first);
        assert_eq!(published.world_scene().command_count(), 1);

        buffers.stage_world_only(
            ExtractionParameters::new(failed_generation, Color::WHITE, 0.0, limits),
            [ActiveCamera2d::new(Camera2d::new(Vec2::ONE, 20.0)?)],
            [source(failed_first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [],
        )?;
        buffers.discard_staged();
        assert_eq!(buffers.publish(), None);
        assert_eq!(
            buffers.published().map(ExtractedFrame::world_generation),
            Some(first_generation),
            "abandoning a validated candidate must not publish its work buffer"
        );

        buffers.stage_world_only(
            ExtractionParameters::new(recovered_generation, Color::WHITE, 0.0, limits),
            [ActiveCamera2d::new(Camera2d::new(
                Vec2::new(-2.0, 3.0),
                20.0,
            )?)],
            [],
            [rectangle_source(
                recovered,
                Vec2::new(4.0, 5.0),
                Vec2::new(4.0, 5.0),
                Vec2::ONE,
                0,
                0.0,
            )],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(recovered_generation));
        let published = buffers
            .published()
            .ok_or("valid retry should publish its complete frame")?;
        assert_eq!(published.world_generation(), recovered_generation);
        assert_eq!(published.background(), Color::WHITE);
        assert_eq!(published.camera().center(), Vec2::new(-2.0, 3.0));
        assert!(published.resolved_circles().is_empty());
        let [rectangle] = published.resolved_rectangles() else {
            return Err("valid retry should contain only its rectangle".into());
        };
        assert_eq!(rectangle.source(), recovered);
        assert_eq!(published.world_scene().command_count(), 1);
        assert_eq!(published.world_scene().statistics().accepted_commands(), 1);
        assert!(matches!(
            published.world_scene().commands()[0].command(),
            DrawCommand::Rect(_)
        ));
        Ok(())
    }

    #[test]
    fn successful_publications_alternate_exactly_two_warmed_buffers() -> Result<(), Box<dyn Error>>
    {
        let first_generation = generation(1);
        let second_generation = generation(2);
        let third_generation = generation(3);
        let mut world = World::new();
        let first = entity(&mut world, first_generation);
        let second = entity(&mut world, second_generation);
        let third = entity(&mut world, third_generation);
        let limits = RenderLimits::default();
        let mut buffers = ExtractionBuffers::new();

        buffers.stage_world_only(
            ExtractionParameters::new(first_generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(first, Vec2::ZERO, Vec2::ZERO, 0, 0.0)],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(first_generation));
        let published = buffers.published().ok_or("first frame was not published")?;
        let first_circle_address = published.storage.resolved_circles.as_ptr();
        let first_scene_address = published.storage.world_scene.commands().as_ptr();

        buffers.stage_world_only(
            ExtractionParameters::new(second_generation, Color::WHITE, 0.0, limits),
            [camera()?],
            [source(second, Vec2::ONE, Vec2::ONE, 0, 0.0)],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(second_generation));
        let published = buffers
            .published()
            .ok_or("second frame was not published")?;
        let second_circle_address = published.storage.resolved_circles.as_ptr();
        let second_scene_address = published.storage.world_scene.commands().as_ptr();
        assert_ne!(second_circle_address, first_circle_address);
        assert_ne!(second_scene_address, first_scene_address);

        buffers.stage_world_only(
            ExtractionParameters::new(third_generation, Color::BLACK, 0.0, limits),
            [camera()?],
            [source(
                third,
                Vec2::new(2.0, 3.0),
                Vec2::new(2.0, 3.0),
                0,
                0.0,
            )],
            [],
            [],
        )?;
        assert_eq!(buffers.publish(), Some(third_generation));
        let published = buffers.published().ok_or("third frame was not published")?;
        assert_eq!(
            published.storage.resolved_circles.as_ptr(),
            first_circle_address
        );
        assert_eq!(
            published.storage.world_scene.commands().as_ptr(),
            first_scene_address
        );
        assert_eq!(published.resolved_circles()[0].source(), third);
        Ok(())
    }
}

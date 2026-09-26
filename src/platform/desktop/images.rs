//! Retained desktop images and ordered composition of CPU screen snapshots.

use std::{error::Error, fmt, mem::size_of};

use sim_engine::{
    BlendMode, Camera2d, FrameBudget, FrameBudgetResource, FrameComposerError, FramePassOptions,
    FrameReport, Image2d, ImageBudget, ImageError, ImageSampling, ImageTexelRect,
    LogicalScreenPosition, LogicalScreenVector, LogicalViewport, Rect, RenderTarget2d,
    RendererFrameError, SceneStatistics, Vec2, WgpuRenderer,
};

use super::draw_plan::{self, DesktopDraw};
use crate::{
    ExtractedFrame, ScreenDraw,
    assets::{ImageAssetId, ImageAssetRegistry},
    identity::LogicEntity,
    screen::ImageFilter,
};

/// Failure while preparing an immutable image for the desktop renderer.
///
/// Preparation happens only after a complete CPU snapshot is published. An
/// earlier successful cache upload may remain cached if later preparation or
/// composition fails; no partially composed frame is presented.
#[derive(Debug)]
pub enum DesktopImageError {
    /// Retained screen geometry could not be prepared.
    Geometry(super::geometry::DesktopScreenError),
    /// A published image did not resolve in the owning application's registry.
    MissingAsset {
        /// Managed visual requesting the unavailable asset.
        source: LogicEntity,
        /// Immutable asset identity requested by that visual.
        image: ImageAssetId,
    },
    /// The renderer rejected the image or its bounded recovery allocation.
    Upload {
        /// Managed visual that first required this uncached image.
        source: LogicEntity,
        /// Immutable asset whose preparation failed.
        image: ImageAssetId,
        /// Concrete Sim;Engine image failure.
        error: ImageError,
    },
    /// The bounded desktop cache could not reserve one metadata record.
    CacheAllocationFailed {
        /// Managed visual requiring the new cache entry.
        source: LogicEntity,
        /// Immutable asset associated with the reservation.
        image: ImageAssetId,
        /// Minimum additional bytes requested for cache metadata.
        requested_bytes: usize,
    },
    /// A private cache invariant would exceed the registered asset count.
    CacheLimitExceeded {
        /// Frozen number of application image registrations.
        limit: usize,
    },
    /// A published draw plan referenced an unavailable image or rectangle run.
    InvalidDrawPlan,
    /// Managed text was enabled for CPU extraction without the `text` GPU bridge.
    TextFeatureRequired,
}

impl fmt::Display for DesktopImageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Geometry(error) => error.fmt(formatter),
            Self::TextFeatureRequired => formatter.write_str("desktop labels require the `text` feature; `headless-text` alone only extracts CPU text"),
            Self::MissingAsset { source, image } => write!(
                formatter,
                "screen image {source:?} references unavailable asset {image:?}"
            ),
            Self::Upload {
                source,
                image,
                error,
            } => write!(
                formatter,
                "screen image {source:?} could not prepare asset {image:?}: {error}"
            ),
            Self::CacheAllocationFailed {
                source,
                image,
                requested_bytes,
            } => write!(
                formatter,
                "screen image {source:?} could not reserve {requested_bytes} cache bytes for {image:?}"
            ),
            Self::CacheLimitExceeded { limit } => write!(
                formatter,
                "desktop image cache exceeded its {limit} registered-asset limit"
            ),
            Self::InvalidDrawPlan => {
                formatter.write_str("published screen draw plan is inconsistent")
            }
        }
    }
}

impl Error for DesktopImageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Geometry(error) => Some(error),
            Self::Upload { error, .. } => Some(error),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub(super) enum ScreenPresentationError {
    Images(DesktopImageError),
    Composition(FrameComposerError),
    ThreeD(super::three_d::DesktopThreeDError),
    #[cfg(feature = "text")]
    Text(super::text::DesktopTextError),
}

#[cfg(feature = "text")]
impl From<super::text::DesktopTextError> for ScreenPresentationError {
    fn from(error: super::text::DesktopTextError) -> Self {
        Self::Text(error)
    }
}

impl From<super::three_d::DesktopThreeDError> for ScreenPresentationError {
    fn from(error: super::three_d::DesktopThreeDError) -> Self {
        Self::ThreeD(error)
    }
}

impl From<DesktopImageError> for ScreenPresentationError {
    fn from(error: DesktopImageError) -> Self {
        Self::Images(error)
    }
}

impl From<FrameComposerError> for ScreenPresentationError {
    fn from(error: FrameComposerError) -> Self {
        Self::Composition(error)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum CacheError<E> {
    Limit { limit: usize },
    Allocation { requested_bytes: usize },
    Create(E),
}

struct ResourceCache<K, V> {
    entries: Vec<(K, V)>,
}

impl<K: Copy + Eq, V> ResourceCache<K, V> {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn get(&self, key: K) -> Option<&V> {
        self.entries
            .iter()
            .find_map(|(stored, value)| (*stored == key).then_some(value))
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn ensure<E>(
        &mut self,
        key: K,
        limit: usize,
        create: impl FnOnce() -> Result<V, E>,
    ) -> Result<(), CacheError<E>> {
        if self.get(key).is_some() {
            return Ok(());
        }
        if self.entries.len() >= limit {
            return Err(CacheError::Limit { limit });
        }
        self.entries
            .try_reserve_exact(1)
            .map_err(|_| CacheError::Allocation {
                requested_bytes: size_of::<(K, V)>(),
            })?;
        let resource = create().map_err(CacheError::Create)?;
        self.entries.push((key, resource));
        Ok(())
    }
}

pub(super) struct DesktopImages {
    pub(super) geometry: super::geometry::DesktopGeometry,
    pub(super) screen_mode: super::geometry::DesktopScreenMode,
    pub(super) text_batching: bool,
    resources: ResourceCache<ImageAssetId, Image2d>,
    referenced: ScreenReferences,
    #[cfg(feature = "text")]
    pub(super) texts: super::text::DesktopText,
}

#[derive(Default)]
struct ScreenReferences {
    images: Vec<ImageAssetId>,
    #[cfg(feature = "text")]
    fonts: Vec<crate::text::TextFont>,
}

impl ScreenReferences {
    fn clear(&mut self) {
        self.images.clear();
        #[cfg(feature = "text")]
        self.fonts.clear();
    }
}

pub(super) fn needs_managed_presentation(
    extracted: &ExtractedFrame,
    _images: &DesktopImages,
) -> bool {
    #[cfg(all(feature = "headless-text", not(feature = "text")))]
    if !extracted.resolved_screen_texts().is_empty() {
        return true;
    }
    #[cfg(feature = "text")]
    if !extracted.resolved_screen_texts().is_empty() || _images.texts.has_runs() {
        return true;
    }
    !extracted.screen_draws().is_empty() || _images.geometry.has_runs()
}

impl DesktopImages {
    pub(super) fn new() -> Self {
        Self {
            geometry: super::geometry::DesktopGeometry::default(),
            screen_mode: super::geometry::DesktopScreenMode::default(),
            text_batching: true,
            resources: ResourceCache::new(),
            referenced: ScreenReferences::default(),
            #[cfg(feature = "text")]
            texts: super::text::DesktopText::default(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.geometry.clear();
        self.resources.clear();
        self.referenced.clear();
        #[cfg(feature = "text")]
        self.texts.clear();
    }

    pub(super) fn preflight_three_d(
        &mut self,
        extracted: &ExtractedFrame,
        registry: &ImageAssetRegistry,
        budget: FrameBudget,
        target_bytes: usize,
    ) -> Result<(), ScreenPresentationError> {
        // The public surface format provides exact color-target bytes before
        // allocation. Engine still validates its private uniform/tessellation work.
        preflight_with_target(
            extracted,
            registry,
            &mut self.referenced,
            budget,
            Some(target_bytes),
            self.screen_mode,
            self.text_batching,
        )
    }

    fn prepare(
        &mut self,
        renderer: &WgpuRenderer,
        registry: &ImageAssetRegistry,
        extracted: &ExtractedFrame,
    ) -> Result<(), DesktopImageError> {
        for visual in extracted.resolved_screen_images() {
            if visual.clip() == crate::screen::ScreenClip::Empty {
                continue;
            }
            let source = visual.source();
            let image = visual.image();
            let asset = registry
                .get(image)
                .ok_or(DesktopImageError::MissingAsset { source, image })?;
            self.resources
                .ensure(image, registry.len(), || {
                    let budget =
                        ImageBudget::new(asset.width(), asset.height(), asset.pixels().len())?;
                    renderer.create_image_rgba8_from_slice(
                        asset.width(),
                        asset.height(),
                        asset.pixels(),
                        budget,
                    )
                })
                .map_err(|error| match error {
                    CacheError::Limit { limit } => DesktopImageError::CacheLimitExceeded { limit },
                    CacheError::Allocation { requested_bytes } => {
                        DesktopImageError::CacheAllocationFailed {
                            source,
                            image,
                            requested_bytes,
                        }
                    }
                    CacheError::Create(error) => DesktopImageError::Upload {
                        source,
                        image,
                        error,
                    },
                })?;
        }
        Ok(())
    }
}

#[derive(Default)]
struct FrameWork {
    passes: usize,
    commands: usize,
    vertices: usize,
    upload_bytes: usize,
    texture_bytes: usize,
    draw_calls: usize,
}

impl FrameWork {
    fn scene(&mut self, statistics: SceneStatistics) {
        self.geometry(statistics, true);
    }

    fn geometry(&mut self, statistics: SceneStatistics, streaming: bool) {
        self.passes = self.passes.saturating_add(1);
        self.commands = self.commands.saturating_add(statistics.accepted_commands());
        self.vertices = self
            .vertices
            .saturating_add(statistics.estimated_tessellated_vertices());
        if streaming {
            self.upload_bytes = self
                .upload_bytes
                .saturating_add(statistics.estimated_upload_bytes());
        }
        self.draw_calls = self
            .draw_calls
            .saturating_add(statistics.estimated_draw_batches());
    }

    fn image(&mut self) {
        self.passes = self.passes.saturating_add(1);
        self.commands = self.commands.saturating_add(1);
        self.vertices = self.vertices.saturating_add(6);
        self.draw_calls = self.draw_calls.saturating_add(1);
    }

    fn validate(&self, budget: FrameBudget) -> Result<(), FrameComposerError> {
        for (resource, limit, actual) in [
            (
                FrameBudgetResource::Passes,
                budget.max_passes(),
                self.passes,
            ),
            (
                FrameBudgetResource::Commands,
                budget.max_commands(),
                self.commands,
            ),
            (
                FrameBudgetResource::Vertices,
                budget.max_vertices(),
                self.vertices,
            ),
            (
                FrameBudgetResource::UploadBytes,
                budget.max_upload_bytes(),
                self.upload_bytes,
            ),
            (
                FrameBudgetResource::TextureBytes,
                budget.max_texture_bytes(),
                self.texture_bytes,
            ),
            (
                FrameBudgetResource::DrawCalls,
                budget.max_draw_calls(),
                self.draw_calls,
            ),
        ] {
            if actual > limit {
                return Err(FrameComposerError::BudgetExceeded {
                    resource,
                    limit,
                    actual,
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
fn preflight(
    extracted: &ExtractedFrame,
    registry: &ImageAssetRegistry,
    referenced: &mut ScreenReferences,
    budget: FrameBudget,
) -> Result<(), ScreenPresentationError> {
    preflight_with_target(
        extracted,
        registry,
        referenced,
        budget,
        None,
        super::geometry::DesktopScreenMode::Streaming,
        false,
    )
}

fn preflight_with_target(
    extracted: &ExtractedFrame,
    registry: &ImageAssetRegistry,
    referenced: &mut ScreenReferences,
    budget: FrameBudget,
    target_bytes: Option<usize>,
    mode: super::geometry::DesktopScreenMode,
    text_batching: bool,
) -> Result<(), ScreenPresentationError> {
    referenced.clear();
    if referenced.images.capacity() < registry.len() {
        referenced
            .images
            .try_reserve_exact(registry.len())
            .map_err(|_| FrameComposerError::AllocationFailed {
                requested_bytes: registry.len().saturating_mul(size_of::<ImageAssetId>()),
            })?;
    }
    let mut work = FrameWork::default();
    work.scene(extracted.world_scene().statistics());
    if let Some(bytes) = target_bytes {
        work.image();
        work.texture_bytes = bytes;
    }
    for draw in draw_plan::draws(extracted, text_batching) {
        #[cfg(not(feature = "text"))]
        let DesktopDraw::Single(draw) = draw?;
        #[cfg(feature = "text")]
        let draw = match draw? {
            DesktopDraw::Single(draw) => draw,
            #[cfg(feature = "text")]
            DesktopDraw::TextBatch(range) => {
                account_texts(
                    &mut work,
                    referenced,
                    &extracted.resolved_screen_texts()[range],
                )?;
                continue;
            }
        };
        match draw {
            #[cfg(all(feature = "headless-text", not(feature = "text")))]
            ScreenDraw::Text { .. } => return Err(DesktopImageError::TextFeatureRequired.into()),
            ScreenDraw::Rectangles { run } | ScreenDraw::Primitives { run } => {
                let scene = extracted
                    .screen_rectangle_run(run)
                    .ok_or(DesktopImageError::InvalidDrawPlan)?;
                // Resource uploads are separately bounded/reported at creation.
                // Vertex/draw admission stays conservative until Engine preflight.
                work.geometry(
                    scene.statistics(),
                    mode == super::geometry::DesktopScreenMode::Streaming,
                );
            }
            ScreenDraw::Image { index } => {
                let visual = extracted
                    .resolved_screen_images()
                    .get(index)
                    .ok_or(DesktopImageError::InvalidDrawPlan)?;
                let image = visual.image();
                if visual.clip() == crate::screen::ScreenClip::Empty {
                    continue;
                }
                let asset = registry.get(image).ok_or(DesktopImageError::MissingAsset {
                    source: visual.source(),
                    image,
                })?;
                if !referenced.images.contains(&image) {
                    // Every identity resolves in the immutable bounded registry,
                    // so the reserved registry-sized vector cannot grow here.
                    referenced.images.push(image);
                    work.texture_bytes = work.texture_bytes.saturating_add(asset.pixels().len());
                }
                work.image();
            }
            #[cfg(feature = "text")]
            ScreenDraw::Text { index } => {
                let text = extracted
                    .resolved_screen_texts()
                    .get(index)
                    .ok_or(super::text::DesktopTextError::InvalidDrawPlan)?;
                if draw_plan::visible(text) {
                    account_texts(&mut work, referenced, std::slice::from_ref(text))?;
                }
            }
        }
    }
    // This preflight covers public CPU statistics. Engine also checks its
    // private uniform sizes and exact tessellation before presenting anything.
    work.validate(budget)?;
    Ok(())
}

#[cfg(feature = "text")]
fn account_texts(
    work: &mut FrameWork,
    referenced: &mut ScreenReferences,
    texts: &[crate::ResolvedScreenText],
) -> Result<(), super::text::DesktopTextError> {
    let first = texts
        .first()
        .ok_or(super::text::DesktopTextError::InvalidDrawPlan)?;
    let font = first.font();
    if !referenced.fonts.contains(font) {
        referenced
            .fonts
            .try_reserve(1)
            .map_err(|_| super::text::DesktopTextError::Allocation {
                requested_bytes: size_of::<crate::text::TextFont>(),
            })?;
        referenced.fonts.push(font.clone());
        let atlas = font.settings().atlas_budget();
        work.texture_bytes = work.texture_bytes.saturating_add(
            (atlas.width() as usize)
                .saturating_mul(atlas.height() as usize)
                .saturating_mul(4),
        );
    }
    work.passes = work.passes.saturating_add(1);
    work.commands = work.commands.saturating_add(1);
    let mut previous_clip = None;
    for text in texts {
        // Shaped count includes spacing glyphs, so both vertices and clip spans
        // are conservative. Engine validates actual raster counts afterward.
        work.vertices = work
            .vertices
            .saturating_add(text.visual().glyph_count().saturating_mul(6));
        let clip = text.visual().clip();
        if previous_clip != Some(clip) {
            work.draw_calls = work.draw_calls.saturating_add(1);
            previous_clip = Some(clip);
        }
    }
    Ok(())
}

fn screen_camera(viewport: LogicalViewport) -> Result<Camera2d, FrameComposerError> {
    Camera2d::new(
        Vec2::new(viewport.width() * 0.5, viewport.height() * -0.5),
        1.0,
    )
    .map_err(|_| RendererFrameError::InvalidGeometryTransform.into())
}

fn screen_rectangle(position: LogicalScreenPosition, size: LogicalScreenVector) -> Rect {
    let position = position.to_vec2();
    let size = size.to_vec2();
    Rect::new(
        Vec2::new(position.x(), -(position.y() + size.y())),
        Vec2::new(position.x() + size.x(), -position.y()),
    )
}

fn image_camera(
    viewport: LogicalViewport,
    position: LogicalScreenPosition,
    size: LogicalScreenVector,
    rotation: f32,
) -> Result<Camera2d, FrameComposerError> {
    let mut camera = screen_camera(viewport)?;
    if rotation == 0.0 {
        return Ok(camera);
    }
    let p = position.to_vec2() + size.to_vec2() * 0.5;
    let pivot = Vec2::new(p.x(), -p.y());
    let delta = camera.center() - pivot;
    let (sin, cos) = rotation.sin_cos();
    let center = pivot
        + Vec2::new(
            delta.x() * cos - delta.y() * sin,
            delta.x() * sin + delta.y() * cos,
        );
    camera
        .set_center(center)
        .map_err(|_| FrameComposerError::Frame(RendererFrameError::InvalidGeometryTransform))?;
    camera
        .set_rotation(-rotation)
        .map_err(|_| FrameComposerError::Frame(RendererFrameError::InvalidGeometryTransform))?;
    Ok(camera)
}

pub(super) fn present(
    renderer: &mut WgpuRenderer,
    extracted: &ExtractedFrame,
    registry: &ImageAssetRegistry,
    images: &mut DesktopImages,
    budget: FrameBudget,
    target: Option<&RenderTarget2d>,
) -> Result<FrameReport, ScreenPresentationError> {
    preflight_with_target(
        extracted,
        registry,
        &mut images.referenced,
        budget,
        target.map(RenderTarget2d::allocation_bytes),
        images.screen_mode,
        images.text_batching,
    )?;
    let viewport = renderer
        .logical_viewport()
        .map_err(|_| FrameComposerError::Frame(RendererFrameError::InvalidViewport))?;
    images.prepare(renderer, registry, extracted)?;
    images
        .geometry
        .prepare(renderer, extracted, images.screen_mode)
        .map_err(DesktopImageError::Geometry)?;
    #[cfg(feature = "text")]
    images
        .texts
        .prepare(renderer, extracted, images.text_batching)?;

    let mut frame = renderer.begin_frame(extracted.background(), budget)?;
    frame.draw_scene(
        extracted.world_scene(),
        extracted.camera(),
        FramePassOptions::new(0),
    )?;
    if let Some(target) = target {
        // Engine targets already contain premultiplied color. Alpha composition
        // reveals the underlying World through a transparent 3D background.
        frame.draw_render_target(target, BlendMode::Alpha, 1.0, FramePassOptions::new(0))?;
    }
    #[cfg(feature = "text")]
    let mut text_batch = 0;
    for draw in draw_plan::draws(extracted, images.text_batching) {
        // FrameComposer keeps insertion order when the integer order is equal.
        // CPU layer/depth/entity sorting therefore survives interleaved types.
        let options = FramePassOptions::new(1);
        #[cfg(not(feature = "text"))]
        let DesktopDraw::Single(draw) = draw?;
        #[cfg(feature = "text")]
        let draw = match draw? {
            DesktopDraw::Single(draw) => draw,
            #[cfg(feature = "text")]
            DesktopDraw::TextBatch(_) => {
                images.texts.draw_batch(&mut frame, text_batch, options)?;
                text_batch += 1;
                continue;
            }
        };
        match draw {
            #[cfg(all(feature = "headless-text", not(feature = "text")))]
            ScreenDraw::Text { .. } => return Err(DesktopImageError::TextFeatureRequired.into()),
            ScreenDraw::Rectangles { run } | ScreenDraw::Primitives { run } => {
                let scene = extracted
                    .screen_rectangle_run(run)
                    .ok_or(DesktopImageError::InvalidDrawPlan)?;
                if images.screen_mode == super::geometry::DesktopScreenMode::Streaming {
                    frame.draw_screen_scene(scene, options)?;
                } else {
                    images.geometry.draw(&mut frame, run, options)?;
                }
            }
            ScreenDraw::Image { index } => {
                let visual = extracted
                    .resolved_screen_images()
                    .get(index)
                    .ok_or(DesktopImageError::InvalidDrawPlan)?;
                if visual.clip() == crate::screen::ScreenClip::Empty {
                    continue;
                }
                let image = images.resources.get(visual.image()).ok_or(
                    DesktopImageError::MissingAsset {
                        source: visual.source(),
                        image: visual.image(),
                    },
                )?;
                let options = match visual.clip() {
                    crate::screen::ScreenClip::Empty => continue,
                    crate::screen::ScreenClip::Unclipped => options,
                    crate::screen::ScreenClip::Rectangle(clip) => options.with_clip(clip),
                };
                let region = visual
                    .source_region()
                    .map(|region| {
                        ImageTexelRect::new(region.x(), region.y(), region.width(), region.height())
                    })
                    .transpose()
                    .map_err(|error| DesktopImageError::Upload {
                        source: visual.source(),
                        image: visual.image(),
                        error,
                    })?;
                let sampling = match visual.filter() {
                    ImageFilter::Nearest => ImageSampling::Nearest,
                    ImageFilter::Linear => ImageSampling::Linear,
                };
                frame.draw_world_image(
                    image,
                    region,
                    screen_rectangle(visual.position(), visual.size()),
                    0.0,
                    image_camera(
                        viewport,
                        visual.position(),
                        visual.size(),
                        visual.rotation(),
                    )?,
                    visual.tint(),
                    sampling,
                    options,
                )?;
            }
            #[cfg(feature = "text")]
            ScreenDraw::Text { index } => {
                let text = extracted
                    .resolved_screen_texts()
                    .get(index)
                    .ok_or(super::text::DesktopTextError::InvalidDrawPlan)?;
                if !text.text().is_empty() {
                    let options = match text.visual().clip() {
                        crate::screen::ScreenClip::Empty => continue,
                        crate::screen::ScreenClip::Unclipped => options,
                        crate::screen::ScreenClip::Rectangle(clip) => options.with_clip(clip),
                    };
                    images.texts.draw(&mut frame, text, options)?;
                }
            }
        }
    }
    Ok(frame.present()?)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn rotated_images_keep_center_and_clockwise_logical_geometry_for_any_viewport()
    -> crate::LogicResult {
        let position = LogicalScreenPosition::new(100.0, 80.0);
        let size = LogicalScreenVector::new(100.0, 20.0);
        let pivot = Vec2::new(150.0, -90.0);
        for (width, height) in [(800.0, 600.0), (640.0, 480.0), (300.0, 900.0)] {
            let viewport = LogicalViewport::new(width, height)?;
            for angle in [0.0, std::f32::consts::FRAC_PI_2, 0.4, -0.7] {
                let camera = image_camera(viewport, position, size, angle)?;
                let center = camera.world_to_screen(pivot, viewport)?.to_vec2();
                assert!((center.x() - 150.0).abs() < 0.0002);
                assert!((center.y() - 90.0).abs() < 0.0002);
                let right = camera
                    .world_to_screen(pivot + Vec2::new(50.0, 0.0), viewport)?
                    .to_vec2();
                assert!((right.x() - (150.0 + 50.0 * angle.cos())).abs() < 0.0002);
                assert!((right.y() - (90.0 + 50.0 * angle.sin())).abs() < 0.0002);
            }
        }
        Ok(())
    }

    #[test]
    fn cache_reuses_resources_until_renderer_invalidation() {
        let uploads = Cell::new(0);
        let mut cache = ResourceCache::new();
        let create = || {
            uploads.set(uploads.get() + 1);
            Ok::<_, ()>(uploads.get())
        };
        cache.ensure(7, 2, create).unwrap();
        cache.ensure(7, 2, create).unwrap();
        cache.ensure(9, 2, create).unwrap();
        assert_eq!(uploads.get(), 2);
        assert_eq!(cache.get(7), Some(&1));
        assert_eq!(cache.get(9), Some(&2));
        let capacity = cache.entries.capacity();
        cache.clear();
        assert!(cache.get(7).is_none());
        assert_eq!(cache.entries.capacity(), capacity);
        cache.ensure(7, 2, create).unwrap();
        assert_eq!(uploads.get(), 3);
        assert_eq!(cache.get(7), Some(&3));
    }

    #[test]
    fn failed_resource_preparation_preserves_existing_entries_and_retries() {
        let mut cache = ResourceCache::new();
        cache.ensure(1, 2, || Ok::<_, &'static str>(10)).unwrap();
        assert_eq!(
            cache.ensure(2, 2, || Err("upload failed")),
            Err(CacheError::Create("upload failed"))
        );
        assert_eq!(cache.get(1), Some(&10));
        assert_eq!(cache.get(2), None);
        cache.ensure(2, 2, || Ok::<_, &'static str>(20)).unwrap();
        assert_eq!(cache.get(2), Some(&20));
        assert_eq!(
            cache.ensure(3, 2, || Ok::<_, &'static str>(30)),
            Err(CacheError::Limit { limit: 2 })
        );
        assert_eq!(cache.entries.len(), 2);
    }

    #[cfg(feature = "text")]
    #[test]
    fn glyph_group_preflight_counts_one_item_but_preserves_clip_draw_limits() -> crate::LogicResult
    {
        use crate::prelude::*;
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Action {}
        let mut config = AppConfig::default();
        config.set_render_limits(
            RenderLimits::default()
                .with_max_screen_texts(3)
                .with_max_screen_text_bytes(64)
                .with_max_screen_text_glyphs(16),
        );
        let mut app = Application::<Action>::new(config)?;
        let font = app.register_font(
            include_bytes!("../../../tests/assets/text/DejaVuSans.ttf").to_vec(),
            TextSettings::new(18.0)?,
        )?;
        let a = ScreenTextVisual::new(font, "A", LogicalScreenPosition::new(3.0, 30.0))?;
        let mut b = a.clone();
        b.set_clip(ScreenClip::Rectangle(sim_engine::ScreenClipRect::new(
            LogicalScreenPosition::new(0.0, 0.0),
            LogicalScreenPosition::new(40.0, 40.0),
        )?));
        let camera = ActiveCamera2d::centered(1.0)?;
        let initial = app.register_world("group-budget", move |world| {
            world.spawn(camera)?;
            world.spawn(a.clone())?;
            world.spawn(b.clone())?;
            world.spawn(a.clone())?;
            Ok(())
        })?;
        let runner = app.build_headless(initial)?;
        let snapshot = runner.extracted_frame().unwrap();
        let mut references = ScreenReferences::default();
        let check = |references: &mut ScreenReferences, batching, draws| {
            preflight_with_target(
                snapshot,
                runner.image_assets(),
                references,
                FrameBudget::new(2, 8, 100, 65_536, 16 * 1024 * 1024, draws),
                None,
                super::super::geometry::DesktopScreenMode::Prepared,
                batching,
            )
        };
        check(&mut references, true, 3).unwrap();
        assert!(matches!(
            check(&mut references, false, 3),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::Passes,
                    actual: 4,
                    ..
                }
            ))
        ));
        assert!(matches!(
            check(&mut references, true, 2),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::DrawCalls,
                    actual: 3,
                    ..
                }
            ))
        ));
        Ok(())
    }

    #[test]
    fn screen_image_camera_preserves_offscreen_corners_and_vertical_orientation() {
        for (width, height) in [(640.0, 360.0), (301.0, 901.0), (1.0, 1.0)] {
            let viewport = LogicalViewport::new(width, height).unwrap();
            let camera = screen_camera(viewport).unwrap();
            let position = LogicalScreenPosition::new(-10.0, -20.0);
            let size = LogicalScreenVector::new(width + 40.0, height + 60.0);
            let rectangle = screen_rectangle(position, size);
            let top_left = camera
                .world_to_screen(
                    Vec2::new(rectangle.min().x(), rectangle.max().y()),
                    viewport,
                )
                .unwrap();
            let bottom_right = camera
                .world_to_screen(
                    Vec2::new(rectangle.max().x(), rectangle.min().y()),
                    viewport,
                )
                .unwrap();
            assert_eq!(top_left, position);
            assert_eq!(bottom_right.to_vec2(), position.to_vec2() + size.to_vec2());
        }
    }

    #[test]
    fn mixed_plan_preflight_counts_every_run_and_each_shared_texture_once() -> crate::LogicResult {
        use crate::prelude::*;

        let mut config = AppConfig::default();
        config.set_render_limits(RenderLimits::default().with_max_screen_images(2));
        let mut application = Application::<u8>::new(config)?;
        let asset = application.register_image_rgba8(4, 1, &[255; 16])?;
        let position = LogicalScreenPosition::new(-2.0, 4.0);
        let size = LogicalScreenVector::new(12.0, 8.0);
        let rectangle = ScreenRectangleVisual::new(position, size, Color::WHITE)?;
        let image = ScreenImageVisual::new(asset, position, size)?;
        let camera = ActiveCamera2d::centered(1.0)?;
        let world = application.register_world("desktop-image-plan", move |world| {
            world.spawn(camera)?;
            for depth in [0.0, 2.0, 4.0] {
                let mut visual = rectangle;
                visual
                    .set_draw_order_depth(depth)
                    .map_err(|error| crate::world::WorldBuildError::user(error.to_string()))?;
                world.spawn(visual)?;
            }
            for depth in [1.0, 3.0] {
                let mut visual = image;
                visual
                    .set_draw_order_depth(depth)
                    .map_err(|error| crate::world::WorldBuildError::user(error.to_string()))?;
                world.spawn(visual)?;
            }
            Ok(())
        })?;
        let runner = application.build_headless(world)?;
        let extracted = runner.extracted_frame().unwrap();
        assert_eq!(
            extracted.screen_draws(),
            &[
                ScreenDraw::Rectangles { run: 0 },
                ScreenDraw::Image { index: 0 },
                ScreenDraw::Rectangles { run: 1 },
                ScreenDraw::Image { index: 1 },
                ScreenDraw::Rectangles { run: 2 },
            ]
        );
        let mut referenced = ScreenReferences::default();
        let enough = FrameBudget::new(6, 100, 1000, 10000, 16, 100);
        preflight(extracted, runner.image_assets(), &mut referenced, enough).unwrap();
        assert_eq!(referenced.images, [asset]);
        let passes = FrameBudget::new(5, 100, 1000, 10000, 16, 100);
        assert!(matches!(
            preflight(extracted, runner.image_assets(), &mut referenced, passes),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::Passes,
                    limit: 5,
                    actual: 6
                }
            ))
        ));
        let textures = FrameBudget::new(6, 100, 1000, 10000, 15, 100);
        assert!(matches!(
            preflight(extracted, runner.image_assets(), &mut referenced, textures),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::TextureBytes,
                    limit: 15,
                    actual: 16
                }
            ))
        ));
        let three_d = FrameBudget::new(7, 100, 1000, 10000, 80, 100);
        preflight_with_target(
            extracted,
            runner.image_assets(),
            &mut referenced,
            three_d,
            Some(64),
            super::super::geometry::DesktopScreenMode::Streaming,
            false,
        )
        .unwrap();
        assert!(matches!(
            preflight_with_target(
                extracted,
                runner.image_assets(),
                &mut referenced,
                enough,
                Some(64),
                super::super::geometry::DesktopScreenMode::Streaming,
                false,
            ),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::Passes,
                    actual: 7,
                    ..
                }
            ))
        ));
        let short_texture = FrameBudget::new(7, 100, 1000, 10000, 79, 100);
        assert!(matches!(
            preflight_with_target(
                extracted,
                runner.image_assets(),
                &mut referenced,
                short_texture,
                Some(64),
                super::super::geometry::DesktopScreenMode::Streaming,
                false,
            ),
            Err(ScreenPresentationError::Composition(
                FrameComposerError::BudgetExceeded {
                    resource: FrameBudgetResource::TextureBytes,
                    actual: 80,
                    ..
                }
            ))
        ));
        Ok(())
    }
}

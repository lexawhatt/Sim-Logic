use std::mem::size_of;

use sim_engine::{
    Color, Layer, LogicalScreenPosition, LogicalScreenVector, SceneBudgetResource, SceneError,
    ScreenScene,
};

use crate::{
    identity::{LogicEntity, WorldGeneration},
    render::RenderLimits,
    screen::ScreenRectangleVisual,
};

use super::{ExtractionError, compare_visual_order};

#[path = "screen_cache.rs"]
mod cache;
pub use cache::ScreenExtractionUpdates;

#[path = "images.rs"]
mod images;
pub use images::ResolvedScreenImage;
pub(crate) use images::ScreenImageSource;
#[cfg(feature = "headless-text")]
#[path = "text.rs"]
mod text;
#[cfg(feature = "headless-text")]
pub use text::ResolvedScreenText;
#[cfg(feature = "headless-text")]
pub(crate) use text::ScreenTextSource;
#[path = "composition.rs"]
mod composition;
use composition::RectangleRun;
pub use composition::ScreenDraw;
#[path = "primitives.rs"]
mod primitives;
pub use primitives::ResolvedScreenPrimitive;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ScreenSource {
    Rectangle(ScreenRectangleSource),
    Line(LogicEntity, crate::screen::ScreenLineVisual),
    Circle(LogicEntity, crate::screen::ScreenCircleVisual),
    Polyline(LogicEntity, crate::screen::ScreenPolylineVisual),
    Image(ScreenImageSource),
    #[cfg(feature = "headless-text")]
    Text(ScreenTextSource),
}

impl From<ScreenRectangleSource> for ScreenSource {
    fn from(source: ScreenRectangleSource) -> Self {
        Self::Rectangle(source)
    }
}

/// One screen-fixed rectangle retained for headless inspection.
///
/// The rectangle uses logical pixels with a top-left origin and downward y.
/// It is sampled directly, independently of camera motion or fixed-time alpha.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedScreenRectangle {
    source: LogicEntity,
    visual: ScreenRectangleVisual,
}

impl ResolvedScreenRectangle {
    /// Returns the exact sampled component, including radius, outline and clip.
    pub const fn visual(&self) -> &ScreenRectangleVisual {
        &self.visual
    }
    /// Returns the managed entity that produced this screen rectangle.
    pub const fn source(self) -> LogicEntity {
        self.source
    }

    /// Returns the top-left position in logical screen pixels.
    pub const fn position(self) -> LogicalScreenPosition {
        self.visual.position()
    }

    /// Returns the full positive width and height in logical pixels.
    pub const fn size(self) -> LogicalScreenVector {
        self.visual.size()
    }

    /// Returns the normalized straight-linear RGBA fill color.
    pub const fn color(self) -> Color {
        self.visual.color()
    }

    /// Returns the primary ordering layer within the screen scene.
    pub const fn layer(self) -> Layer {
        self.visual.layer()
    }

    /// Returns the finite secondary ordering value within its screen layer.
    ///
    /// This is draw order, not camera depth. All screen layers follow world content.
    pub const fn draw_order_depth(self) -> f32 {
        self.visual.draw_order_depth()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScreenRectangleSource {
    entity: LogicEntity,
    visual: ScreenRectangleVisual,
}

impl ScreenRectangleSource {
    pub(crate) const fn new(entity: LogicEntity, visual: &ScreenRectangleVisual) -> Self {
        Self {
            entity,
            visual: *visual,
        }
    }
}

pub(super) struct ScreenExtractionBuffer {
    resolved: Vec<ResolvedScreenRectangle>,
    primitives: Vec<ResolvedScreenPrimitive>,
    scene: ScreenScene,
    images: Vec<ResolvedScreenImage>,
    #[cfg(feature = "headless-text")]
    texts: Vec<ResolvedScreenText>,
    draws: Vec<ScreenDraw>,
    runs: Vec<RectangleRun>,
    source_keys: Vec<ScreenSource>,
    source_scratch: Vec<ScreenSource>,
    cache_scope: Option<(WorldGeneration, RenderLimits)>,
    pub(super) updates: ScreenExtractionUpdates,
}

impl ScreenExtractionBuffer {
    pub(super) fn new(limits: RenderLimits) -> Result<Self, ExtractionError> {
        Ok(Self {
            resolved: Vec::new(),
            primitives: Vec::new(),
            images: Vec::new(),
            #[cfg(feature = "headless-text")]
            texts: Vec::new(),
            draws: Vec::new(),
            runs: Vec::new(),
            source_keys: Vec::new(),
            source_scratch: Vec::new(),
            cache_scope: None,
            updates: ScreenExtractionUpdates::default(),
            scene: ScreenScene::with_budget(Color::TRANSPARENT, limits.screen_scene_budget())
                .map_err(ExtractionError::ScreenScene)?,
        })
    }

    pub(super) fn clear(&mut self) {
        self.cache_scope = None;
        self.source_keys.clear();
        self.source_scratch.clear();
        self.clear_sources();
        self.scene.clear();
        self.runs.clear();
    }

    fn clear_sources(&mut self) {
        self.resolved.clear();
        self.primitives.clear();
        self.images.clear();
        #[cfg(feature = "headless-text")]
        self.texts.clear();
        self.draws.clear();
    }

    pub(super) fn resolved(&self) -> &[ResolvedScreenRectangle] {
        &self.resolved
    }

    pub(super) fn geometry(&self, index: usize) -> Option<ResolvedScreenPrimitive> {
        if self.primitives.is_empty() {
            self.resolved
                .get(index)
                .copied()
                .map(ResolvedScreenPrimitive::Rectangle)
        } else {
            self.primitives.get(index).cloned()
        }
    }

    pub(super) fn geometry_len(&self) -> usize {
        if self.primitives.is_empty() {
            self.resolved.len()
        } else {
            self.primitives.len()
        }
    }

    pub(super) fn primitive_records(&self) -> impl Iterator<Item = ResolvedScreenPrimitive> + '_ {
        (0..self.geometry_len()).filter_map(|index| self.geometry(index))
    }

    pub(super) fn primitive_run_records(
        &self,
        run: usize,
    ) -> Option<impl Iterator<Item = ResolvedScreenPrimitive> + '_> {
        let range = if self.has_non_rectangles() {
            let run = self.runs.get(run)?;
            run.start..run.end
        } else if run == 0 && self.geometry_len() != 0 {
            0..self.geometry_len()
        } else {
            return None;
        };
        Some(range.filter_map(|index| self.geometry(index)))
    }

    fn geometry_draw(&self, run: usize) -> ScreenDraw {
        if self.primitives.is_empty() {
            ScreenDraw::Rectangles { run }
        } else {
            ScreenDraw::Primitives { run }
        }
    }

    pub(super) fn images(&self) -> &[ResolvedScreenImage] {
        &self.images
    }
    #[cfg(feature = "headless-text")]
    pub(super) fn texts(&self) -> &[ResolvedScreenText] {
        &self.texts
    }

    fn has_non_rectangles(&self) -> bool {
        #[cfg(feature = "headless-text")]
        if !self.texts.is_empty() {
            return true;
        }
        !self.images.is_empty()
    }
    pub(super) fn draws(&self) -> &[ScreenDraw] {
        &self.draws
    }
    pub(super) fn run_records(&self, run: usize) -> Option<&[ResolvedScreenRectangle]> {
        if !self.has_non_rectangles() {
            return (run == 0 && self.geometry_len() != 0).then_some(self.resolved.as_slice());
        }
        let run = self.runs.get(run)?;
        if self.primitives.is_empty() {
            return self.resolved.get(run.start..run.end);
        }
        let start = self.primitives[..run.start]
            .iter()
            .filter(|value| matches!(value, ResolvedScreenPrimitive::Rectangle(_)))
            .count();
        let count = self.primitives[run.start..run.end]
            .iter()
            .filter(|value| matches!(value, ResolvedScreenPrimitive::Rectangle(_)))
            .count();
        self.resolved.get(start..start + count)
    }

    #[cfg(feature = "desktop")]
    pub(super) fn run_scene(&self, run: usize) -> Option<&ScreenScene> {
        if !self.has_non_rectangles() {
            return (run == 0 && self.geometry_len() != 0).then_some(&self.scene);
        }
        self.runs.get(run).map(|run| &run.scene)
    }

    #[cfg(feature = "desktop")]
    pub(super) const fn scene(&self) -> &ScreenScene {
        &self.scene
    }

    fn collect_sources(
        &mut self,
        generation: WorldGeneration,
        limits: RenderLimits,
        sources: impl IntoIterator<Item = ScreenSource>,
    ) -> Result<(), ExtractionError> {
        debug_assert_eq!(self.scene.budget(), Some(limits.screen_scene_budget()));
        let mut line_count = 0usize;
        let mut circle_count = 0usize;
        let mut path_count = 0usize;
        let mut path_points = 0usize;
        #[cfg(feature = "headless-text")]
        let mut text_bytes = 0usize;
        #[cfg(feature = "headless-text")]
        let mut text_glyphs = 0usize;
        for source in sources {
            let key = source.clone();
            let source = match source {
                ScreenSource::Rectangle(source) => source,
                ScreenSource::Polyline(entity, visual) => {
                    if entity.world_generation() != generation {
                        return Err(ExtractionError::ForeignEntity {
                            entity,
                            expected: generation,
                        });
                    }
                    path_count += 1;
                    path_points = path_points.checked_add(visual.points().len()).ok_or(
                        ExtractionError::ScreenPolylinePointsLimitExceeded {
                            entity,
                            limit: limits.max_screen_polyline_points(),
                            requested: usize::MAX,
                        },
                    )?;
                    if path_points > limits.max_screen_polyline_points() {
                        return Err(ExtractionError::ScreenPolylinePointsLimitExceeded {
                            entity,
                            limit: limits.max_screen_polyline_points(),
                            requested: path_points,
                        });
                    }
                    self.push_vector(
                        generation,
                        entity,
                        ResolvedScreenPrimitive::Polyline {
                            source: entity,
                            visual,
                        },
                        path_count,
                        limits.max_screen_polylines(),
                        limits.screen_scene_budget().max_commands(),
                    )?;
                    self.remember_source(key)?;
                    continue;
                }
                ScreenSource::Line(entity, visual) => {
                    line_count += 1;
                    self.push_vector(
                        generation,
                        entity,
                        ResolvedScreenPrimitive::Line {
                            source: entity,
                            visual,
                        },
                        line_count,
                        limits.max_screen_lines(),
                        limits.screen_scene_budget().max_commands(),
                    )?;
                    self.remember_source(key)?;
                    continue;
                }
                ScreenSource::Circle(entity, visual) => {
                    circle_count += 1;
                    self.push_vector(
                        generation,
                        entity,
                        ResolvedScreenPrimitive::Circle {
                            source: entity,
                            visual,
                        },
                        circle_count,
                        limits.max_screen_circles(),
                        limits.screen_scene_budget().max_commands(),
                    )?;
                    self.remember_source(key)?;
                    continue;
                }
                ScreenSource::Image(source) => {
                    if self.images.len() == limits.max_screen_images() {
                        return Err(ExtractionError::ScreenImageLimitExceeded {
                            limit: limits.max_screen_images(),
                        });
                    }
                    let image = source.resolve(generation)?;
                    self.images
                        .try_reserve(1)
                        .map_err(|_| ExtractionError::AllocationFailed {
                            requested_bytes: size_of::<ResolvedScreenImage>(),
                        })?;
                    self.images.push(image);
                    self.remember_source(key)?;
                    continue;
                }
                #[cfg(feature = "headless-text")]
                ScreenSource::Text(source) => {
                    if self.texts.len() == limits.max_screen_texts() {
                        return Err(ExtractionError::ScreenTextLimitExceeded {
                            limit: limits.max_screen_texts(),
                        });
                    }
                    let text = source.resolve(generation)?;
                    text_bytes = text_bytes.checked_add(text.text().len()).ok_or(
                        ExtractionError::ScreenTextBytesLimitExceeded {
                            limit: limits.max_screen_text_bytes(),
                            requested: usize::MAX,
                        },
                    )?;
                    if text_bytes > limits.max_screen_text_bytes() {
                        return Err(ExtractionError::ScreenTextBytesLimitExceeded {
                            limit: limits.max_screen_text_bytes(),
                            requested: text_bytes,
                        });
                    }
                    text_glyphs = text_glyphs.checked_add(text.visual().glyph_count()).ok_or(
                        ExtractionError::ScreenTextGlyphLimitExceeded {
                            limit: limits.max_screen_text_glyphs(),
                            requested: usize::MAX,
                        },
                    )?;
                    if text_glyphs > limits.max_screen_text_glyphs() {
                        return Err(ExtractionError::ScreenTextGlyphLimitExceeded {
                            limit: limits.max_screen_text_glyphs(),
                            requested: text_glyphs,
                        });
                    }
                    self.texts
                        .try_reserve(1)
                        .map_err(|_| ExtractionError::AllocationFailed {
                            requested_bytes: size_of::<ResolvedScreenText>(),
                        })?;
                    self.texts.push(text);
                    self.remember_source(key)?;
                    continue;
                }
            };
            if source.entity.world_generation() != generation {
                return Err(ExtractionError::ForeignEntity {
                    entity: source.entity,
                    expected: generation,
                });
            }
            if self.resolved.len() == limits.max_screen_rectangles() {
                return Err(ExtractionError::ScreenRectangleLimitExceeded {
                    limit: limits.max_screen_rectangles(),
                });
            }
            let command_limit = limits.screen_scene_budget().max_commands();
            if self.resolved.len().saturating_add(self.primitives.len()) >= command_limit {
                return Err(ExtractionError::ScreenScene(SceneError::BudgetExceeded {
                    resource: SceneBudgetResource::Commands,
                    limit: command_limit,
                    requested: self
                        .resolved
                        .len()
                        .saturating_add(self.primitives.len())
                        .saturating_add(1),
                }));
            }
            source
                .visual
                .validate()
                .map_err(|error| ExtractionError::InvalidScreenVisual {
                    entity: source.entity,
                    error,
                })?;
            self.resolved
                .try_reserve(1)
                .map_err(|_| ExtractionError::AllocationFailed {
                    requested_bytes: size_of::<ResolvedScreenRectangle>(),
                })?;
            self.resolved.push(ResolvedScreenRectangle {
                source: source.entity,
                visual: source.visual,
            });
            self.remember_source(key)?;
        }
        self.resolved.sort_unstable_by(|left, right| {
            compare_visual_order(
                left.layer(),
                left.draw_order_depth(),
                left.source,
                right.layer(),
                right.draw_order_depth(),
                right.source,
            )
        });
        let count = self.primitives.len().saturating_add(self.resolved.len());
        if count > limits.screen_scene_budget().max_commands() {
            return Err(ExtractionError::ScreenScene(SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands,
                limit: limits.screen_scene_budget().max_commands(),
                requested: count,
            }));
        }
        if !self.primitives.is_empty() {
            self.primitives
                .try_reserve(self.resolved.len())
                .map_err(|_| ExtractionError::AllocationFailed {
                    requested_bytes: self
                        .resolved
                        .len()
                        .saturating_mul(size_of::<ResolvedScreenPrimitive>()),
                })?;
            self.primitives.extend(
                self.resolved
                    .iter()
                    .copied()
                    .map(ResolvedScreenPrimitive::Rectangle),
            );
            self.primitives.sort_unstable_by(|a, b| {
                compare_visual_order(
                    a.layer(),
                    a.draw_order_depth(),
                    a.source(),
                    b.layer(),
                    b.draw_order_depth(),
                    b.source(),
                )
                .then_with(|| a.kind_order().cmp(&b.kind_order()))
            });
        }
        self.images.sort_unstable_by(|left, right| {
            compare_visual_order(
                left.layer(),
                left.draw_order_depth(),
                left.source(),
                right.layer(),
                right.draw_order_depth(),
                right.source(),
            )
        });
        #[cfg(feature = "headless-text")]
        self.texts.sort_unstable_by(|left, right| {
            compare_visual_order(
                left.layer(),
                left.draw_order_depth(),
                left.source(),
                right.layer(),
                right.draw_order_depth(),
                right.source(),
            )
        });
        Ok(())
    }

    fn push_vector(
        &mut self,
        generation: WorldGeneration,
        entity: LogicEntity,
        primitive: ResolvedScreenPrimitive,
        count: usize,
        limit: usize,
        command_limit: usize,
    ) -> Result<(), ExtractionError> {
        if entity.world_generation() != generation {
            return Err(ExtractionError::ForeignEntity {
                entity,
                expected: generation,
            });
        }
        if count > limit {
            return Err(ExtractionError::ScreenPrimitiveLimitExceeded { entity, limit });
        }
        let requested = self
            .resolved
            .len()
            .saturating_add(self.primitives.len())
            .saturating_add(1);
        if requested > command_limit {
            return Err(ExtractionError::ScreenScene(SceneError::BudgetExceeded {
                resource: SceneBudgetResource::Commands,
                limit: command_limit,
                requested,
            }));
        }
        self.primitives
            .try_reserve(1)
            .map_err(|_| ExtractionError::AllocationFailed {
                requested_bytes: size_of::<ResolvedScreenPrimitive>(),
            })?;
        self.primitives.push(primitive);
        Ok(())
    }
}

#[cfg(test)]
#[path = "screen_tests.rs"]
mod tests;

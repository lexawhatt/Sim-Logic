use std::mem::size_of;

use sim_engine::{
    Color, Layer, LogicalScreenPosition, LogicalScreenVector, SceneBudgetResource, SceneError,
    ScreenScene, ShapeStyle,
};

use crate::{
    identity::{LogicEntity, WorldGeneration},
    render::RenderLimits,
    screen::ScreenRectangleVisual,
};

use super::{ExtractionError, compare_visual_order};

#[path = "images.rs"]
mod images;
pub use images::ResolvedScreenImage;
pub(crate) use images::ScreenImageSource;
#[path = "composition.rs"]
mod composition;
use composition::RectangleRun;
pub use composition::ScreenDraw;

#[derive(Debug, Clone, Copy)]
pub(crate) enum ScreenSource {
    Rectangle(ScreenRectangleSource),
    Image(ScreenImageSource),
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

#[derive(Debug, Clone, Copy)]
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
    scene: ScreenScene,
    images: Vec<ResolvedScreenImage>,
    draws: Vec<ScreenDraw>,
    runs: Vec<RectangleRun>,
}

impl ScreenExtractionBuffer {
    pub(super) fn new(limits: RenderLimits) -> Result<Self, ExtractionError> {
        Ok(Self {
            resolved: Vec::new(),
            images: Vec::new(),
            draws: Vec::new(),
            runs: Vec::new(),
            scene: ScreenScene::with_budget(Color::TRANSPARENT, limits.screen_scene_budget())
                .map_err(ExtractionError::ScreenScene)?,
        })
    }

    pub(super) fn clear(&mut self) {
        self.resolved.clear();
        self.scene.clear();
        self.images.clear();
        self.draws.clear();
    }

    pub(super) fn resolved(&self) -> &[ResolvedScreenRectangle] {
        &self.resolved
    }

    pub(super) fn images(&self) -> &[ResolvedScreenImage] {
        &self.images
    }
    pub(super) fn draws(&self) -> &[ScreenDraw] {
        &self.draws
    }
    pub(super) fn run_records(&self, run: usize) -> Option<&[ResolvedScreenRectangle]> {
        if self.images.is_empty() {
            return (run == 0 && !self.resolved.is_empty()).then_some(self.resolved.as_slice());
        }
        let run = self.runs.get(run)?;
        self.resolved.get(run.start..run.end)
    }

    #[cfg(feature = "desktop")]
    pub(super) fn run_scene(&self, run: usize) -> Option<&ScreenScene> {
        if self.images.is_empty() {
            return (run == 0 && !self.resolved.is_empty()).then_some(&self.scene);
        }
        self.runs.get(run).map(|run| &run.scene)
    }

    #[cfg(feature = "desktop")]
    pub(super) const fn scene(&self) -> &ScreenScene {
        &self.scene
    }

    pub(super) fn extract(
        &mut self,
        generation: WorldGeneration,
        limits: RenderLimits,
        sources: impl IntoIterator<Item = ScreenSource>,
    ) -> Result<(), ExtractionError> {
        debug_assert_eq!(self.scene.budget(), Some(limits.screen_scene_budget()));
        for source in sources {
            let source = match source {
                ScreenSource::Rectangle(source) => source,
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
            if self.resolved.len() == command_limit {
                return Err(ExtractionError::ScreenScene(SceneError::BudgetExceeded {
                    resource: SceneBudgetResource::Commands,
                    limit: command_limit,
                    requested: self.resolved.len().saturating_add(1),
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
        for rectangle in &self.resolved {
            self.scene
                .try_square_rect_on_layer(
                    rectangle.layer(),
                    rectangle.position(),
                    rectangle.size(),
                    ShapeStyle::filled(rectangle.color()),
                )
                .map_err(ExtractionError::ScreenScene)?;
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
        self.compose(limits.screen_scene_budget())?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "screen_tests.rs"]
mod tests;

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
}

impl ScreenExtractionBuffer {
    pub(super) fn new(limits: RenderLimits) -> Result<Self, ExtractionError> {
        Ok(Self {
            resolved: Vec::new(),
            scene: ScreenScene::with_budget(Color::TRANSPARENT, limits.screen_scene_budget())
                .map_err(ExtractionError::ScreenScene)?,
        })
    }

    pub(super) fn clear(&mut self) {
        self.resolved.clear();
        self.scene.clear();
    }

    pub(super) fn resolved(&self) -> &[ResolvedScreenRectangle] {
        &self.resolved
    }

    #[cfg(feature = "desktop")]
    pub(super) const fn scene(&self) -> &ScreenScene {
        &self.scene
    }

    pub(super) fn extract(
        &mut self,
        generation: WorldGeneration,
        limits: RenderLimits,
        sources: impl IntoIterator<Item = ScreenRectangleSource>,
    ) -> Result<(), ExtractionError> {
        debug_assert_eq!(self.scene.budget(), Some(limits.screen_scene_budget()));
        for source in sources {
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
        Ok(())
    }
}

#[cfg(test)]
#[path = "screen_tests.rs"]
mod tests;

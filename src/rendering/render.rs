//! Renderer-independent limits for visual extraction and presentation.

use bevy_ecs::{
    prelude::{Res, Resource},
    system::SystemParam,
};
use sim_engine::{LogicalViewport, SceneBudget};

/// Validated logical viewport supplied to the current application frame.
///
/// FrameUpdate systems may use this for camera or screen-layout decisions.
/// Headless callers provide the same value explicitly through `FrameRequest`.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub(crate) struct FrameViewportState(LogicalViewport);

impl FrameViewportState {
    pub const fn new(viewport: LogicalViewport) -> Self {
        Self(viewport)
    }

    pub const fn logical(self) -> LogicalViewport {
        self.0
    }
}

/// Read-only validated logical viewport for the current FrameUpdate.
///
/// It is intentionally unavailable to FixedUpdate systems so a resize cannot
/// silently affect fixed simulation rules.
#[derive(SystemParam)]
pub struct FrameViewport<'w> {
    state: Res<'w, FrameViewportState>,
}

impl FrameViewport<'_> {
    /// Returns the current logical pixel dimensions.
    pub fn logical(&self) -> LogicalViewport {
        self.state.logical()
    }
}

/// Maximum world-space circles extracted by the default setup.
pub const DEFAULT_MAX_WORLD_CIRCLES: usize = 10_000;

/// Maximum world-space rectangles extracted by the default setup.
pub const DEFAULT_MAX_WORLD_RECTANGLES: usize = 10_000;

/// Maximum world-space line sources extracted by the default setup.
pub const DEFAULT_MAX_WORLD_LINES: usize = 10_000;

/// Maximum logical-screen rectangles extracted by the default setup.
pub const DEFAULT_MAX_SCREEN_RECTANGLES: usize = 256;

/// Bounded work accepted by the default world-space scene.
pub const DEFAULT_WORLD_SCENE_BUDGET: SceneBudget = SceneBudget::new(
    10_000,
    0,
    2_000_000,
    8 * 1024 * 1024,
    16 * 1024 * 1024,
    128 * 1024 * 1024,
    10_000,
);

/// Bounded work accepted by the default logical-screen scene.
pub const DEFAULT_SCREEN_SCENE_BUDGET: SceneBudget =
    SceneBudget::new(256, 0, 3_072, 256 * 1024, 512 * 1024, 512 * 1024, 256);

/// Renderer-independent form of the default heterogeneous-frame budget.
///
/// Allows a world source and a screen source, with the sum of their default
/// command, vertex, upload, and draw limits. No textures are included.
pub const DEFAULT_FRAME_LIMITS: FrameLimits = FrameLimits::new(
    2,
    10_000 + 256,
    2_000_000 + 3_072,
    128 * 1024 * 1024 + 512 * 1024,
    0,
    10_000 + 256,
);

/// Work limits for a single composed presentation frame.
///
/// This type deliberately does not mention `wgpu` or Sim;Engine's
/// renderer-only `FrameBudget`, so the same application configuration remains
/// available in a headless build. The desktop bridge converts these values at
/// its feature boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameLimits {
    max_passes: usize,
    max_commands: usize,
    max_vertices: usize,
    max_upload_bytes: usize,
    max_texture_bytes: usize,
    max_draw_calls: usize,
}

impl FrameLimits {
    /// Creates exact limits for one composed frame.
    pub const fn new(
        max_passes: usize,
        max_commands: usize,
        max_vertices: usize,
        max_upload_bytes: usize,
        max_texture_bytes: usize,
        max_draw_calls: usize,
    ) -> Self {
        Self {
            max_passes,
            max_commands,
            max_vertices,
            max_upload_bytes,
            max_texture_bytes,
            max_draw_calls,
        }
    }

    /// Returns the maximum number of ordered render sources.
    pub const fn max_passes(self) -> usize {
        self.max_passes
    }

    /// Returns the maximum number of referenced scene commands.
    pub const fn max_commands(self) -> usize {
        self.max_commands
    }

    /// Returns the maximum number of referenced or generated vertices.
    pub const fn max_vertices(self) -> usize {
        self.max_vertices
    }

    /// Returns the maximum bytes uploaded while preparing a frame.
    pub const fn max_upload_bytes(self) -> usize {
        self.max_upload_bytes
    }

    /// Returns the maximum nominal retained texture bytes referenced by a frame.
    pub const fn max_texture_bytes(self) -> usize {
        self.max_texture_bytes
    }

    /// Returns the maximum conservative draw-call count.
    pub const fn max_draw_calls(self) -> usize {
        self.max_draw_calls
    }
}

impl Default for FrameLimits {
    fn default() -> Self {
        DEFAULT_FRAME_LIMITS
    }
}

/// Configurable CPU extraction and presentation limits for the first slice.
///
/// All values are fixed before the application starts. Zero per-kind limits
/// are valid. The world Scene command budget limits circles, rectangles, and
/// nonzero lines together. Screen rectangles use a separate ScreenScene budget;
/// the desktop compositor applies FrameLimits to their combined presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderLimits {
    max_world_circles: usize,
    max_world_rectangles: usize,
    max_world_lines: usize,
    max_screen_rectangles: usize,
    world_scene_budget: SceneBudget,
    screen_scene_budget: SceneBudget,
    frame_limits: FrameLimits,
}

impl RenderLimits {
    /// Creates limits with an explicit world circle cap and default remaining caps.
    ///
    /// Use [`RenderLimits::with_max_world_rectangles`] and
    /// [`RenderLimits::with_max_world_lines`] to replace those caps without
    /// changing this source-compatible constructor. Screen rectangles start
    /// with [`DEFAULT_MAX_SCREEN_RECTANGLES`] and [`DEFAULT_SCREEN_SCENE_BUDGET`].
    pub const fn new(
        max_world_circles: usize,
        world_scene_budget: SceneBudget,
        frame_limits: FrameLimits,
    ) -> Self {
        Self {
            max_world_circles,
            max_world_rectangles: DEFAULT_MAX_WORLD_RECTANGLES,
            max_world_lines: DEFAULT_MAX_WORLD_LINES,
            max_screen_rectangles: DEFAULT_MAX_SCREEN_RECTANGLES,
            world_scene_budget,
            screen_scene_budget: DEFAULT_SCREEN_SCENE_BUDGET,
            frame_limits,
        }
    }

    /// Returns the maximum number of extracted world circles.
    pub const fn max_world_circles(self) -> usize {
        self.max_world_circles
    }

    /// Returns the maximum number of extracted world rectangles.
    pub const fn max_world_rectangles(self) -> usize {
        self.max_world_rectangles
    }

    /// Returns the maximum number of enabled managed line sources inspected.
    ///
    /// Zero-vector sources count toward this limit even though they emit no
    /// resolved record or Scene command.
    pub const fn max_world_lines(self) -> usize {
        self.max_world_lines
    }

    /// Returns the maximum number of enabled managed screen rectangles extracted.
    pub const fn max_screen_rectangles(self) -> usize {
        self.max_screen_rectangles
    }

    /// Replaces the rectangle staging limit without changing other budgets.
    ///
    /// Zero is valid and disables managed rectangle extraction. The Scene
    /// command budget still limits the combined primitive count.
    pub const fn with_max_world_rectangles(mut self, limit: usize) -> Self {
        self.max_world_rectangles = limit;
        self
    }

    /// Replaces the line-source staging limit without changing other budgets.
    ///
    /// Zero is valid and disables managed line extraction. The Scene command
    /// budget still limits all nonzero primitives together.
    pub const fn with_max_world_lines(mut self, limit: usize) -> Self {
        self.max_world_lines = limit;
        self
    }

    /// Replaces the screen rectangle staging cap without changing other limits.
    ///
    /// Zero disables screen rectangle extraction. The screen Scene command
    /// budget may impose a smaller cap independently.
    pub const fn with_max_screen_rectangles(mut self, limit: usize) -> Self {
        self.max_screen_rectangles = limit;
        self
    }

    /// Replaces the logical-screen Scene budget without changing other limits.
    ///
    /// Zero budget fields are valid and are enforced during extraction.
    /// This does not change the desktop compositor's aggregate FrameLimits.
    pub const fn with_screen_scene_budget(mut self, budget: SceneBudget) -> Self {
        self.screen_scene_budget = budget;
        self
    }

    /// Returns the budget applied to the world-space `Scene`.
    pub const fn world_scene_budget(self) -> SceneBudget {
        self.world_scene_budget
    }

    /// Returns the bounded work allowance for the logical-screen `ScreenScene`.
    pub const fn screen_scene_budget(self) -> SceneBudget {
        self.screen_scene_budget
    }

    /// Returns renderer-independent limits for one composed frame.
    pub const fn frame_limits(self) -> FrameLimits {
        self.frame_limits
    }
}

impl Default for RenderLimits {
    fn default() -> Self {
        Self::new(
            DEFAULT_MAX_WORLD_CIRCLES,
            DEFAULT_WORLD_SCENE_BUDGET,
            DEFAULT_FRAME_LIMITS,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_first_slice_contract() {
        let limits = RenderLimits::default();
        let world = limits.world_scene_budget();
        let screen = limits.screen_scene_budget();
        let frame = limits.frame_limits();

        assert_eq!(limits.max_world_circles(), 10_000);
        assert_eq!(limits.max_world_rectangles(), 10_000);
        assert_eq!(limits.max_world_lines(), 10_000);
        assert_eq!(limits.max_screen_rectangles(), 256);
        assert_eq!(world.max_commands(), 10_000);
        assert_eq!(world.max_points(), 0);
        assert_eq!(world.max_tessellated_vertices(), 2_000_000);
        assert_eq!(world.max_retained_bytes(), 8 * 1024 * 1024);
        assert_eq!(world.max_allocation_bytes(), 16 * 1024 * 1024);
        assert_eq!(world.max_upload_bytes(), 128 * 1024 * 1024);
        assert_eq!(world.max_draw_batches(), 10_000);

        assert_eq!(screen.max_commands(), 256);
        assert_eq!(screen.max_points(), 0);
        assert_eq!(screen.max_tessellated_vertices(), 3_072);
        assert_eq!(screen.max_retained_bytes(), 256 * 1024);
        assert_eq!(screen.max_allocation_bytes(), 512 * 1024);
        assert_eq!(screen.max_upload_bytes(), 512 * 1024);
        assert_eq!(screen.max_draw_batches(), 256);

        assert_eq!(frame.max_passes(), 2);
        assert_eq!(
            frame.max_commands(),
            world.max_commands() + screen.max_commands()
        );
        assert_eq!(
            frame.max_vertices(),
            world.max_tessellated_vertices() + screen.max_tessellated_vertices()
        );
        assert_eq!(
            frame.max_upload_bytes(),
            world.max_upload_bytes() + screen.max_upload_bytes()
        );
        assert_eq!(frame.max_texture_bytes(), 0);
        assert_eq!(
            frame.max_draw_calls(),
            world.max_draw_batches() + screen.max_draw_batches()
        );
    }

    #[test]
    fn rectangle_limit_builder_is_additive_and_allows_zero() {
        let original = RenderLimits::new(7, DEFAULT_WORLD_SCENE_BUDGET, FrameLimits::default());
        let changed = original.with_max_world_rectangles(0);

        assert_eq!(changed.max_world_circles(), 7);
        assert_eq!(changed.max_world_rectangles(), 0);
        assert_eq!(changed.world_scene_budget(), original.world_scene_budget());
        assert_eq!(changed.frame_limits(), original.frame_limits());
    }

    #[test]
    fn line_limit_builder_is_additive_and_allows_zero() {
        let original = RenderLimits::new(7, DEFAULT_WORLD_SCENE_BUDGET, FrameLimits::default());
        let changed = original.with_max_world_lines(0);

        assert_eq!(changed.max_world_circles(), 7);
        assert_eq!(
            changed.max_world_rectangles(),
            original.max_world_rectangles()
        );
        assert_eq!(changed.max_world_lines(), 0);
        assert_eq!(changed.world_scene_budget(), original.world_scene_budget());
        assert_eq!(changed.frame_limits(), original.frame_limits());
    }

    #[test]
    fn screen_limit_builders_preserve_world_and_custom_frame_limits() {
        let frame = FrameLimits::new(1, 2, 3, 4, 5, 6);
        let world = SceneBudget::new(7, 8, 9, 10, 11, 12, 13);
        let original = RenderLimits::new(14, world, frame)
            .with_max_world_rectangles(15)
            .with_max_world_lines(16);
        let zero = SceneBudget::new(0, 0, 0, 0, 0, 0, 0);
        let changed = original
            .with_max_screen_rectangles(0)
            .with_screen_scene_budget(zero);
        assert_eq!(changed.max_screen_rectangles(), 0);
        assert_eq!(changed.screen_scene_budget(), zero);
        assert_eq!(changed.max_world_circles(), original.max_world_circles());
        assert_eq!(
            changed.max_world_rectangles(),
            original.max_world_rectangles()
        );
        assert_eq!(changed.max_world_lines(), original.max_world_lines());
        assert_eq!(changed.world_scene_budget(), world);
        assert_eq!(changed.frame_limits(), frame);
        assert_eq!(
            original.with_max_screen_rectangles(0).screen_scene_budget(),
            original.screen_scene_budget()
        );
        assert_eq!(
            original
                .with_screen_scene_budget(zero)
                .max_screen_rectangles(),
            original.max_screen_rectangles()
        );
    }

    #[test]
    fn default_screen_budget_fits_every_allowed_square_rectangle()
    -> Result<(), sim_engine::SceneError> {
        use sim_engine::{
            Color, LogicalScreenPosition, LogicalScreenVector, ScreenScene, ShapeStyle,
        };

        let limits = RenderLimits::default();
        let mut scene = ScreenScene::with_budget(Color::TRANSPARENT, limits.screen_scene_budget())?;
        for _ in 0..limits.max_screen_rectangles() {
            scene.try_square_rect(
                LogicalScreenPosition::default(),
                LogicalScreenVector::new(16.0, 8.0),
                ShapeStyle::filled(Color::WHITE),
            )?;
        }
        assert_eq!(scene.command_count(), 256);
        // Engine uses a conservative four-triangle estimate even for square corners.
        assert_eq!(scene.statistics().estimated_tessellated_vertices(), 3_072);
        assert_eq!(scene.statistics().estimated_draw_batches(), 256);
        Ok(())
    }
}

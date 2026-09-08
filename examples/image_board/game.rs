//! A neutral image board using immutable assets and managed screen visuals.

use sim_engine::SceneBudget;
use sim_logic::prelude::*;

/// Keyboard actions shared by desktop input and headless tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemoAction {
    /// Move the selected image left in logical screen pixels.
    Left,
    /// Move the selected image right in logical screen pixels.
    Right,
    /// Move the selected image toward the top of the window.
    Up,
    /// Move the selected image toward the bottom of the window.
    Down,
    /// Toggle the selected image between the full asset and its upper half.
    ToggleCrop,
    /// Replace the initial World with the alternate board.
    NextWorld,
    /// Exit through the shared application command path.
    Exit,
}

/// The single image controlled by FrameUpdate; it starts partially offscreen.
#[derive(Component)]
pub struct MovableImage;

const MOVEMENT: DigitalAxis2d<DemoAction> = DigitalAxis2d::new(
    DemoAction::Left,
    DemoAction::Right,
    DemoAction::Down,
    DemoAction::Up,
);
const IMAGE_BYTES: usize = 8 * 8 * 4;

fn update_board(
    input: FrameInput<DemoAction>,
    time: FrameTime,
    viewport: FrameViewport,
    mut images: Query<(&MovableImage, &mut ScreenImageVisual)>,
    mut commands: Commands,
) -> LogicResult {
    if input.has_press_occurrence(DemoAction::Exit) {
        commands.request_exit()?;
        return Ok(());
    }
    let movement = input.normalized_digital_axis(MOVEMENT);
    let delta = Vec2::new(movement.x(), -movement.y()) * (240.0 * time.seconds_f32().min(0.1));
    let toggle_crop = input.pressed(DemoAction::ToggleCrop).count() % 2 == 1;
    for (_marker, mut visual) in &mut images {
        if movement != Vec2::ZERO {
            let position = visual.position().to_vec2() + delta;
            let size = visual.size().to_vec2();
            // Leave half the image reachable beyond every window edge, so
            // clipping is visible and the image cannot be lost completely.
            let x = position
                .x()
                .clamp(-size.x() * 0.5, viewport.logical().width() - size.x() * 0.5);
            let y = position.y().clamp(
                -size.y() * 0.5,
                viewport.logical().height() - size.y() * 0.5,
            );
            visual.set_position(LogicalScreenPosition::new(x, y))?;
        }
        if toggle_crop {
            let source = if visual.source_region().is_some() {
                None
            } else {
                Some(ImageRegion::new(0, 0, 8, 4)?)
            };
            visual.set_source_region(source)?;
        }
    }
    Ok(())
}

fn quadrant_pixels() -> [u8; IMAGE_BYTES] {
    let mut pixels = [0; IMAGE_BYTES];
    for y in 0..8 {
        for x in 0..8 {
            let mut color: [u8; 4] = match (x < 4, y < 4) {
                (true, true) => [216, 62, 55, 255],
                (false, true) => [47, 180, 104, 255],
                (true, false) => [51, 112, 220, 255],
                (false, false) => [234, 180, 48, 255],
            };
            if (x + y) % 2 == 0 {
                for channel in &mut color[..3] {
                    *channel = channel.saturating_add(24);
                }
            }
            if (x == 3 || x == 4) && (y == 3 || y == 4) {
                color[3] = 80;
            }
            let offset = (y * 8 + x) * 4;
            pixels[offset..offset + 4].copy_from_slice(&color);
        }
    }
    pixels
}

struct Board {
    camera: ActiveCamera2d,
    background: WorldBackground,
    rectangles: [ScreenRectangleVisual; 7],
    images: [ScreenImageVisual; 4],
    movable: ScreenImageVisual,
}

impl Board {
    fn new(asset: ImageAssetId, alternate: bool) -> LogicResult<Self> {
        let background = WorldBackground::new(if alternate {
            Color::rgb8(43, 35, 31)
        } else {
            Color::rgb8(19, 26, 36)
        })?;
        let panel = if alternate {
            Color::rgb8(82, 65, 52)
        } else {
            Color::rgb8(49, 62, 78)
        };
        let shift = if alternate { 28.0 } else { 0.0 };
        let rectangle = |x, y, width, height, color, depth| -> LogicResult<ScreenRectangleVisual> {
            let mut visual = ScreenRectangleVisual::new(
                LogicalScreenPosition::new(x, y),
                LogicalScreenVector::new(width, height),
                color,
            )?;
            visual.set_draw_order_depth(depth)?;
            Ok(visual)
        };
        let image = |x, y, size, depth| -> LogicResult<ScreenImageVisual> {
            let mut visual = ScreenImageVisual::new(
                asset,
                LogicalScreenPosition::new(x, y),
                LogicalScreenVector::new(size, size),
            )?;
            visual.set_draw_order_depth(depth)?;
            Ok(visual)
        };
        let rectangles = [
            rectangle(68.0, 100.0 + shift, 288.0, 288.0, panel, 0.0)?,
            rectangle(384.0, 100.0 + shift, 288.0, 288.0, panel, 0.0)?,
            rectangle(716.0, 100.0 + shift, 256.0, 256.0, panel, 0.0)?,
            rectangle(972.0, 100.0 + shift, 256.0, 256.0, panel, 0.0)?,
            // This stripe covers the first image but passes below the second.
            rectangle(
                48.0,
                238.0 + shift,
                660.0,
                22.0,
                Color::rgb8(219, 231, 236),
                20.0,
            )?,
            // A second stripe covers both the cropped and tinted copies.
            rectangle(
                696.0,
                266.0 + shift,
                552.0,
                18.0,
                Color::rgb8(235, 150, 83),
                60.0,
            )?,
            rectangle(24.0, 432.0, 1232.0, 2.0, Color::rgb8(92, 111, 126), 65.0)?,
        ];
        let nearest = image(84.0, 116.0 + shift, 256.0, 10.0)?;
        let mut linear = image(400.0, 116.0 + shift, 256.0, 30.0)?;
        linear.set_filter(ImageFilter::Linear);
        let mut cropped = image(732.0, 116.0 + shift, 224.0, 40.0)?;
        cropped.set_source_region(Some(ImageRegion::new(0, 0, 8, 4)?))?;
        let mut tinted = image(988.0, 116.0 + shift, 224.0, 50.0)?;
        tinted.set_tint(Color::rgb8(151, 211, 255).with_alpha(0.78))?;
        let movable = image(if alternate { 900.0 } else { -48.0 }, 468.0, 224.0, 70.0)?;
        Ok(Self {
            camera: ActiveCamera2d::centered(1.0)?,
            background,
            rectangles,
            images: [nearest, linear, cropped, tinted],
            movable,
        })
    }

    fn spawn(&self, world: &mut WorldBuilder) -> Result<(), WorldBuildError> {
        world.insert_resource(self.background)?;
        world.spawn(self.camera)?;
        for &rectangle in &self.rectangles {
            world.spawn(rectangle)?;
        }
        for &image in &self.images {
            world.spawn(image)?;
        }
        world.spawn((MovableImage, self.movable))?;
        Ok(())
    }
}

/// Builds the same bounded image board for a desktop window or HeadlessRunner.
/// One 256-byte asset is shared by five visuals and survives World replacement.
pub fn build_application() -> LogicResult<(Application<DemoAction>, WorldFactoryId)> {
    let mut config = AppConfig::default();
    config.set_entity_limit(32)?;
    config.set_image_asset_limits(ImageAssetLimits::new(1, 8, 8, IMAGE_BYTES));
    let scene = SceneBudget::new(32, 0, 384, 32 * 1024, 64 * 1024, 64 * 1024, 32);
    // Up to eight images can split rectangles into nine screen runs. Include
    // one world source, and count the shared 8x8 texture only once per frame.
    let frame = FrameLimits::new(18, 40, 512, 128 * 1024, IMAGE_BYTES, 40);
    config.set_render_limits(
        RenderLimits::new(0, scene, frame)
            .with_max_world_rectangles(0)
            .with_max_world_lines(0)
            .with_max_screen_rectangles(32)
            .with_max_screen_images(8)
            .with_screen_scene_budget(scene),
    );
    let mut application = Application::new(config)?;
    application.approve_component::<MovableImage>()?;
    application.bind_wasd_and_arrows(MOVEMENT)?;
    application.bind_key(PhysicalKeyCode::Space, DemoAction::ToggleCrop)?;
    application.bind_key(PhysicalKeyCode::Enter, DemoAction::NextWorld)?;
    application.bind_key(PhysicalKeyCode::Escape, DemoAction::Exit)?;
    application.add_world_replacement_on_press_system();
    application.add_fallible_frame_system(update_board);

    let asset = application.register_image_rgba8(8, 8, &quadrant_pixels())?;
    let alternate_board = Board::new(asset, true)?;
    let alternate = application.register_world("image-board-alternate", move |world| {
        alternate_board.spawn(world)
    })?;
    let initial_board = Board::new(asset, false)?;
    let initial = application.register_world("image-board", move |world| {
        initial_board.spawn(world)?;
        world.insert_resource(WorldReplacementOnPress::new(
            DemoAction::NextWorld,
            alternate,
        ))?;
        Ok(())
    })?;
    Ok((application, initial))
}

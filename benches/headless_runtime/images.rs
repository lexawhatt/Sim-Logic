//! Warmed CPU extraction for a stable mixed image/rectangle composition.
//!
//! This fixture measures the shared headless runtime and its checks. It does
//! not initialize GPU resources or make desktop allocation/timing claims.

use super::*;

const SCREEN_IMAGES: usize = 64;
const SCREEN_RECTANGLES: usize = SCREEN_IMAGES * 2;
const PIXELS: [u8; 16] = [
    255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 128,
];

#[derive(Component)]
struct ImageSlot(usize);

#[derive(Component)]
struct RectangleSlot(usize);

#[derive(Resource, Default)]
struct Updates {
    fixed_ticks: usize,
    frames: usize,
}

fn fixed_update(mut updates: ResMut<Updates>) {
    updates.fixed_ticks += 1;
}

fn frame_update(
    mut updates: ResMut<Updates>,
    mut images: Query<(&ImageSlot, &mut ScreenImageVisual)>,
    mut rectangles: Query<(&RectangleSlot, &mut ScreenRectangleVisual)>,
) -> LogicResult {
    updates.frames += 1;
    let phase = if updates.frames.is_multiple_of(2) {
        0.75
    } else {
        0.25
    };
    for (slot, mut image) in &mut images {
        image.set_position(LogicalScreenPosition::new(
            8.0 + slot.0 as f32 * 3.0 + updates.frames as f32 * 0.5,
            16.0,
        ))?;
        image.set_tint(Color::rgba(0.5, 0.75, 1.0, phase))?;
    }
    for (slot, mut rectangle) in &mut rectangles {
        rectangle.set_position(LogicalScreenPosition::new(
            4.0 + slot.0 as f32 * 2.0 + updates.fixed_ticks as f32 * 0.25,
            24.0,
        ))?;
        rectangle.set_color(Color::rgb(phase, 0.25, 0.5))?;
    }
    Ok(())
}

fn advance(
    runner: &mut HeadlessRunner<BenchAction>,
    image_asset: ImageAssetId,
    frame_number: usize,
) -> LogicResult<u64> {
    let viewport = LogicalViewport::new(1280.0, 720.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport))
    else {
        return Err(io::Error::other("image allocation frame was rejected").into());
    };
    if report.failure().is_some()
        || report.fixed_ticks_attempted() != 1
        || report.spawned() != 0
        || report.despawned() != 0
        || report.exit_requested()
        || !matches!(report.transition(), FrameTransition::None)
        || report.extracted_generation() != Some(runner.world_generation())
    {
        return Err(
            io::Error::other("image allocation frame violated its lifecycle contract").into(),
        );
    }
    let updates = runner
        .resource::<Updates>()
        .ok_or("image update counters missing")?;
    if updates.fixed_ticks != frame_number
        || updates.frames != frame_number
        || runner.image_asset_count() != 1
        || runner.image_pixel_bytes() != PIXELS.len()
        || runner
            .image_asset(image_asset)
            .ok_or("shared image missing")?
            .pixels()
            != PIXELS
    {
        return Err(io::Error::other("image tick or shared-asset accounting changed").into());
    }
    let frame = runner.extracted_frame().ok_or("image frame missing")?;
    let images = frame.resolved_screen_images();
    let rectangles = frame.resolved_screen_rectangles();
    if images.len() != SCREEN_IMAGES
        || rectangles.len() != SCREEN_RECTANGLES
        || frame.screen_draws().len() != SCREEN_IMAGES * 2
        || !frame.resolved_circles().is_empty()
        || !frame.resolved_rectangles().is_empty()
        || !frame.resolved_lines().is_empty()
    {
        return Err(io::Error::other("mixed image/rectangle cardinality changed").into());
    }
    let phase = if frame_number.is_multiple_of(2) {
        0.75
    } else {
        0.25
    };
    let mut checksum = 0;
    for (index, image) in images.iter().enumerate() {
        if image.image() != image_asset
            || image.position()
                != LogicalScreenPosition::new(
                    8.0 + index as f32 * 3.0 + frame_number as f32 * 0.5,
                    16.0,
                )
            || image.size() != LogicalScreenVector::new(12.0, 16.0)
            || image.tint() != Color::rgba(0.5, 0.75, 1.0, phase)
            || image.draw_order_depth() != (index * 3 + 2) as f32
            || image.source_region() != Some(ImageRegion::new((index % 2) as u32, 0, 1, 2)?)
            || image.filter() != ImageFilter::Nearest
        {
            return Err(io::Error::other("image mutation/order checksum mismatch").into());
        }
        let position = image.position().to_vec2();
        checksum += image.draw_order_depth() as u64
            + (position.x() * 4.0) as u64
            + (position.y() * 4.0) as u64
            + (image.tint().alpha() * 4.0) as u64;
    }
    for (index, rectangle) in rectangles.iter().enumerate() {
        let depth = (index / 2) * 3 + index % 2;
        if rectangle.position()
            != LogicalScreenPosition::new(
                4.0 + index as f32 * 2.0 + frame_number as f32 * 0.25,
                24.0,
            )
            || rectangle.size() != LogicalScreenVector::new(4.0, 4.0)
            || rectangle.color() != Color::rgb(phase, 0.25, 0.5)
            || rectangle.draw_order_depth() != depth as f32
        {
            return Err(io::Error::other("rectangle mutation/order checksum mismatch").into());
        }
        let position = rectangle.position().to_vec2();
        checksum += rectangle.draw_order_depth() as u64
            + (position.x() * 4.0) as u64
            + (position.y() * 4.0) as u64
            + (rectangle.color().red() * 4.0) as u64;
    }
    for index in 0..SCREEN_IMAGES {
        if frame.screen_draws()[index * 2] != (ScreenDraw::Rectangles { run: index })
            || frame.screen_draws()[index * 2 + 1] != (ScreenDraw::Image { index })
            || frame.screen_rectangle_run_records(index)
                != Some(&rectangles[index * 2..index * 2 + 2])
        {
            return Err(io::Error::other("mixed screen composition lost interleaving").into());
        }
    }
    // Closed-form oracle from declared slot counts and depths, independent of
    // ECS iteration and the actual extracted list. Coordinates are quarter
    // pixels and tint steps are quarters, so every term is exact here.
    let image_count = SCREEN_IMAGES as u64;
    let rectangle_count = SCREEN_RECTANGLES as u64;
    let image_indices = image_count * (image_count - 1) / 2;
    let rectangle_indices = rectangle_count * (rectangle_count - 1) / 2;
    let phase_quarters = if frame_number.is_multiple_of(2) { 3 } else { 1 };
    let expected_images =
        (98 + 2 * frame_number as u64 + phase_quarters) * image_count + 15 * image_indices;
    let expected_rectangles = (112 + frame_number as u64 + phase_quarters) * rectangle_count
        + 8 * rectangle_indices
        + 6 * image_indices
        + image_count;
    if checksum != expected_images + expected_rectangles {
        return Err(io::Error::other("mixed image numeric checksum mismatch").into());
    }
    black_box(report);
    Ok(checksum)
}

pub(super) fn run_allocation_case() -> LogicResult {
    let mut config = AppConfig::default();
    config.set_image_asset_limits(ImageAssetLimits::new(1, 2, 2, PIXELS.len()));
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_images(SCREEN_IMAGES)
            .with_max_screen_rectangles(SCREEN_RECTANGLES)
            .with_frame_limits(FrameLimits::new(
                1 + SCREEN_IMAGES * 2,
                SCREEN_IMAGES + SCREEN_RECTANGLES,
                SCREEN_IMAGES * 6 + SCREEN_RECTANGLES * 12,
                1024 * 1024,
                PIXELS.len(),
                SCREEN_IMAGES + SCREEN_RECTANGLES,
            )),
    );
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(ImageSlot, RectangleSlot)>()?;
    application.add_fixed_system(fixed_update);
    application.add_fallible_frame_system(frame_update);
    let image_asset = application.register_image_rgba8(2, 2, &PIXELS)?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let world = application.register_world("mixed-screen-images", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Updates::default())?;
        for index in 0..SCREEN_IMAGES {
            let mut image = ScreenImageVisual::new(
                image_asset,
                LogicalScreenPosition::new(8.0 + index as f32 * 3.0, 16.0),
                LogicalScreenVector::new(12.0, 16.0),
            )
            .map_err(|error| WorldBuildError::user(error.to_string()))?;
            image
                .set_draw_order_depth((index * 3 + 2) as f32)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            image
                .set_source_region(Some(
                    ImageRegion::new((index % 2) as u32, 0, 1, 2)
                        .map_err(|error| WorldBuildError::user(error.to_string()))?,
                ))
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            world.spawn((ImageSlot(index), image))?;
            for offset in 0..2 {
                let rectangle_index = index * 2 + offset;
                let mut rectangle = ScreenRectangleVisual::new(
                    LogicalScreenPosition::new(4.0 + rectangle_index as f32 * 2.0, 24.0),
                    LogicalScreenVector::new(4.0, 4.0),
                    Color::WHITE,
                )
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
                rectangle
                    .set_draw_order_depth((index * 3 + offset) as f32)
                    .map_err(|error| WorldBuildError::user(error.to_string()))?;
                world.spawn((RectangleSlot(rectangle_index), rectangle))?;
            }
        }
        Ok(())
    })?;
    let mut runner = application.build_headless(world)?;
    // Twenty alternating publications warm both extraction buffers and every
    // stable two-rectangle run before the allocation counter is enabled.
    for frame in 1..=WARM_UP_FRAMES {
        advance(&mut runner, image_asset, frame)?;
    }
    let before_x = runner
        .extracted_frame()
        .ok_or("image frame missing")?
        .resolved_screen_images()[0]
        .position()
        .to_vec2()
        .x();
    begin_allocation_count();
    let measured = (|| -> LogicResult<u64> {
        let mut checksum = 0;
        for frame in 1..=ALLOCATION_CHECK_FRAMES {
            checksum += advance(&mut runner, image_asset, WARM_UP_FRAMES + frame)?;
        }
        Ok(checksum)
    })();
    let allocations = end_allocation_count();
    let checksum = measured?;
    let after_x = runner
        .extracted_frame()
        .ok_or("image frame missing")?
        .resolved_screen_images()[0]
        .position()
        .to_vec2()
        .x();
    let motion = after_x - before_x;
    if motion != ALLOCATION_CHECK_FRAMES as f32 * 0.5 {
        return Err(io::Error::other("image allocation fixture did not move").into());
    }
    println!(
        "warmed_screen_images frames={ALLOCATION_CHECK_FRAMES} images={SCREEN_IMAGES} rectangles={SCREEN_RECTANGLES} shared_assets=1 rectangle_runs={SCREEN_IMAGES} fixed_ticks_per_frame=1 allocation_calls={allocations} checksum={checksum} first_image_x_delta={motion}"
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed mixed image/rectangle CPU frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

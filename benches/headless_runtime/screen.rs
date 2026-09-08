use super::*;

const SCREEN_RECTANGLES: usize = 256;
const WORLD_CIRCLES: usize = 100;

fn advance_screen_frame(
    runner: &mut HeadlessRunner<BenchAction>,
    frame_number: usize,
) -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
    else {
        return Err(io::Error::other("screen allocation frame was rejected").into());
    };
    if report.failure().is_some()
        || report.exit_requested()
        || report.extracted_generation() != Some(runner.world_generation())
    {
        return Err(io::Error::other("screen allocation frame failed to publish").into());
    }
    let frame = runner
        .extracted_frame()
        .ok_or_else(|| io::Error::other("screen frame missing"))?;
    if frame.resolved_circles().len() != WORLD_CIRCLES
        || frame.resolved_screen_rectangles().len() != SCREEN_RECTANGLES
    {
        return Err(io::Error::other("mixed world/screen cardinality changed").into());
    }
    for (index, rectangle) in frame.resolved_screen_rectangles().iter().enumerate() {
        let expected_x = 8.0 + index as f32 * 2.0 + frame_number as f32 * 0.5;
        if rectangle.position() != LogicalScreenPosition::new(expected_x, 16.0)
            || rectangle.size() != LogicalScreenVector::new(4.0, 4.0)
            || rectangle.draw_order_depth() != index as f32
        {
            return Err(io::Error::other("screen mutation/order checksum mismatch").into());
        }
    }
    Ok(())
}

pub(super) fn run_allocation_case() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::centered(32.0)?;
    let circle = CircleVisual::new(1.0, Color::WHITE)?;
    let world = application.register_world("mixed-world-screen", move |world| {
        world.spawn(camera)?;
        for _ in 0..WORLD_CIRCLES {
            world.spawn(circle)?;
        }
        for index in 0..SCREEN_RECTANGLES {
            let mut panel = ScreenRectangleVisual::new(
                LogicalScreenPosition::new(8.0 + index as f32 * 2.0, 16.0),
                LogicalScreenVector::new(4.0, 4.0),
                Color::WHITE,
            )
            .map_err(|error| WorldBuildError::user(error.to_string()))?;
            // Stable identity order need not equal spawn order. Give this
            // checksum a declared order independent of raw ECS row allocation.
            panel
                .set_draw_order_depth(index as f32)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            world.spawn(panel)?;
        }
        Ok(())
    })?;
    application.add_fallible_frame_system(
        |mut panels: Query<&mut ScreenRectangleVisual>| -> Result<(), ScreenVisualError> {
            for mut panel in &mut panels {
                let previous = panel.position().to_vec2();
                panel.set_position(LogicalScreenPosition::new(previous.x() + 0.5, previous.y()))?;
            }
            Ok(())
        },
    );
    let mut runner = application.build_headless(world)?;
    for frame in 1..=WARM_UP_FRAMES {
        advance_screen_frame(&mut runner, frame)?;
    }
    begin_allocation_count();
    let measured = (|| -> Result<(), Box<dyn Error>> {
        for frame in 1..=ALLOCATION_CHECK_FRAMES {
            advance_screen_frame(&mut runner, WARM_UP_FRAMES + frame)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured?;
    println!(
        "warmed_screen_hud frames={ALLOCATION_CHECK_FRAMES} world_circles={WORLD_CIRCLES} screen_rectangles={SCREEN_RECTANGLES} allocation_calls={allocations}"
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed mixed world/screen frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

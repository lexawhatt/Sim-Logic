use super::*;

const MANAGED_TRANSFORMS: usize = 1_000;

fn advance_pause_frame(
    runner: &mut HeadlessRunner<BenchAction>,
    viewport: LogicalViewport,
    expected_paused: bool,
) -> Result<(), Box<dyn Error>> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
    else {
        return Err(io::Error::other("pause allocation frame was rejected").into());
    };
    if report.failure().is_some()
        || report.exit_requested()
        || report.fixed_ticks_attempted() != 0
        || !matches!(report.transition(), FrameTransition::None)
        || report.extracted_generation() != Some(runner.world_generation())
        || runner.is_paused() != expected_paused
    {
        return Err(io::Error::other("pause allocation frame violated its state contract").into());
    }
    Ok(())
}

pub(super) fn run_allocation_case() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::centered(32.0)?;
    let transform = Transform2d::from_xy(3.0, -2.0)?;
    let world = application.register_world("pause-allocation", move |world| {
        world.spawn(camera)?;
        for _ in 0..MANAGED_TRANSFORMS {
            world.spawn(transform)?;
        }
        Ok(())
    })?;
    application.add_fallible_frame_system(
        |time: FrameTime, mut commands: Commands| -> Result<(), CommandEnqueueError> {
            commands.set_paused(!time.is_paused())
        },
    );
    let mut runner = application.build_headless(world)?;
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    for index in 0..WARM_UP_FRAMES {
        advance_pause_frame(&mut runner, viewport, index % 2 == 0)?;
    }

    // An even warmup leaves the first measured frame entering pause, ensuring
    // that both the cached snap scans and resume are measured on every pair.
    if runner.is_paused() {
        return Err(io::Error::other("pause allocation warmup must end running").into());
    }
    begin_allocation_count();
    let measured = (|| -> Result<(), Box<dyn Error>> {
        for index in 0..ALLOCATION_CHECK_FRAMES {
            advance_pause_frame(&mut runner, viewport, index % 2 == 0)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured?;

    if runner.components::<Transform2d>().count() != MANAGED_TRANSFORMS
        || runner.components::<ActiveCamera2d>().count() != 1
        || runner.is_paused()
    {
        return Err(io::Error::other("pause allocation fixture changed shape or parity").into());
    }
    println!(
        "warmed_pause_resume frames={ALLOCATION_CHECK_FRAMES} transforms={MANAGED_TRANSFORMS} allocation_calls={allocations}"
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed pause/resume frames unexpectedly allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

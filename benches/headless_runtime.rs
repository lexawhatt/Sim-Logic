use std::{
    alloc::{GlobalAlloc, Layout, System},
    error::Error,
    hint::black_box,
    io,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use sim_engine::SceneBudget;
use sim_logic::bevy_ecs::entity_disabling::Disabled;
use sim_logic::prelude::*;

#[path = "headless_runtime/pause.rs"]
mod pause;
#[path = "headless_runtime/screen.rs"]
mod screen;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BenchAction {
    Move,
    NegativeX,
    PositiveX,
    NegativeY,
}

const INPUT_AXIS: DigitalAxis2d<BenchAction> = DigitalAxis2d::new(
    BenchAction::NegativeX,
    BenchAction::PositiveX,
    BenchAction::NegativeY,
    BenchAction::Move,
);

const FIXED_STEP: Duration = Duration::from_nanos(16_666_667);
const WARM_UP_FRAMES: usize = 20;
const MEASURED_FRAMES: usize = 120;
const MANAGED_IDLE_ENTITIES: usize = 100_000;
const CHURN_LIVE_ENTITIES: usize = 10_000;
const CHURN_PER_TICK: usize = 1_000;
const CHURN_TIMING_SAMPLES: usize = 7;
const INSERT_TARGETS: usize = 1_000;
const REMOVE_TARGETS: usize = 1_000;
const ENABLEMENT_TARGETS: usize = 1_000;
const DESPAWN_ONLY_PER_TICK: usize = 100;
const DESPAWN_ONLY_ENTITIES: usize = ALLOCATION_CHECK_FRAMES * DESPAWN_ONLY_PER_TICK;
const EVENTS_PER_TICK: usize = 1_000;
const RESOURCE_ACCESS_FRAMES: usize = 100_000;
const OVERLAP_TARGET_CHECKS: usize = 2_000_000;
const SPATIAL_BUILD_ENTITIES: usize = 100_000;
const SPATIAL_BUILD_SAMPLES: usize = 7;
const LINEAR_MOTION_ENTITIES: usize = 10_000;
const LINEAR_MOTION_VELOCITY: Vec2 = Vec2::new(6.0, -3.0);
const ACCELERATION_MOTION_ENTITIES: usize = 10_000;
const ACCELERATION_MOTION_INITIAL_VELOCITY: Vec2 = Vec2::new(1.0, -0.5);
const ACCELERATION_MOTION_ACCELERATION: Vec2 = Vec2::new(0.75, -1.25);
const DIGITAL_MOTION_ENTITIES: usize = 10_000;
const DIGITAL_MOTION_SPEED: f32 = 6.0;
const ALLOCATION_CHECK_FRAMES: usize = 100;
const EMPTY_FIXED_PAIR_FRAMES: usize = 500;

struct CountingAllocator;

static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);
static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
            ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

fn begin_allocation_count() {
    ALLOCATION_CALLS.store(0, Ordering::SeqCst);
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
}

fn allocation_calls() -> usize {
    ALLOCATION_CALLS.load(Ordering::SeqCst)
}

fn end_allocation_count() -> usize {
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    allocation_calls()
}

#[derive(Component)]
struct IdleMarker;

#[derive(Component, Clone, Copy)]
struct ChurnMarker {
    _payload: u8,
}

#[derive(Component)]
struct DespawnOnlyMarker;

#[derive(Component)]
struct InsertBenchTarget;

#[derive(Component)]
struct InsertBenchValue(u64);

#[derive(Resource, Default)]
struct InsertBenchChecksum(u64);

#[derive(Component)]
struct RemoveBenchValue(u64);

#[derive(Resource)]
struct RemoveBenchTargets(Vec<LogicEntity>);

#[derive(Component)]
struct EnablementBenchTarget;

#[derive(Resource)]
struct EnablementBenchState {
    targets: Vec<LogicEntity>,
    disable_next: bool,
    commands_completed: u64,
}

#[derive(Clone, Copy)]
struct BenchEvent(u32);

#[derive(Resource, Default)]
struct EventChecksum(u64);

struct AppCounter(u64);

#[derive(Resource)]
struct WorldCounter(u64);

#[derive(Component)]
struct OverlapSource;

#[derive(Component)]
struct OverlapCandidate;

#[derive(Component)]
struct RectangleBenchMover;

#[derive(Component)]
struct RectangleBenchWall;

#[derive(Component)]
struct LinearMotionBenchBody;

#[derive(Component)]
struct AccelerationMotionBenchBody;

#[derive(Component)]
struct DigitalMotionBenchBody;

#[derive(Resource, Default)]
struct RectangleCollisionChecksum(u64);

type RectangleMoverQuery<'world, 'state> = Query<
    'world,
    'state,
    (
        LogicEntityRef,
        &'static mut Transform2d,
        &'static CircleCollider2d,
    ),
    (With<RectangleBenchMover>, Without<RectangleBenchWall>),
>;
type RectangleWallOverlaps<'world, 'state> = RectangleOverlapEntities<
    'world,
    'state,
    (With<RectangleBenchWall>, Without<RectangleBenchMover>),
>;

#[derive(Resource)]
struct OverlapBenchState {
    source: LogicEntity,
    scans_per_frame: usize,
    checksum: u64,
}

#[derive(Debug, Clone, Copy)]
enum CircleOverlapLayout {
    Dense,
    BoundaryTangent,
    SparseX,
    SparseY,
    DiagonalNearMiss,
    Mixed,
}

#[derive(Debug, Clone, Copy)]
enum RectangleOverlapLayout {
    Dense,
    EdgeTangent,
    CornerTangent,
    SparseX,
    SparseY,
    CornerNearMiss,
    Mixed,
}

#[derive(Debug, Clone, Copy)]
enum MixedOverlapLayout {
    Contained,
    FaceTangent,
    CornerTangent,
    SparseX,
    SparseY,
    CornerNearMiss,
    Mixed,
}

impl MixedOverlapLayout {
    const fn label(self) -> &'static str {
        match self {
            Self::Contained => "contained",
            Self::FaceTangent => "face-tangent",
            Self::CornerTangent => "corner-tangent",
            Self::SparseX => "sparse-x",
            Self::SparseY => "sparse-y",
            Self::CornerNearMiss => "corner-near-miss",
            Self::Mixed => "mixed",
        }
    }

    fn position(self, index: usize) -> Vec2 {
        match self {
            Self::Contained => Vec2::ZERO,
            Self::FaceTangent => Vec2::new(2.25, 0.0),
            Self::CornerTangent => Vec2::new(1.75, 2.0),
            Self::SparseX => Vec2::new(4.0, 0.0),
            Self::SparseY => Vec2::new(0.0, 4.0),
            Self::CornerNearMiss => Vec2::new(1.75, 2.000_1),
            Self::Mixed => match index.wrapping_mul(7) % 20 {
                0 => Vec2::ZERO,
                1..=9 => Vec2::new(4.0, 0.0),
                10..=15 => Vec2::new(0.0, 4.0),
                _ => Vec2::new(1.75, 2.000_1),
            },
        }
    }

    fn hits_per_scan(self, candidate_count: usize) -> usize {
        match self {
            Self::Contained | Self::FaceTangent | Self::CornerTangent => candidate_count,
            Self::Mixed => (0..candidate_count)
                .filter(|index| index.wrapping_mul(7) % 20 == 0)
                .count(),
            Self::SparseX | Self::SparseY | Self::CornerNearMiss => 0,
        }
    }
}

impl RectangleOverlapLayout {
    const fn label(self) -> &'static str {
        match self {
            Self::Dense => "dense",
            Self::EdgeTangent => "edge-tangent",
            Self::CornerTangent => "corner-tangent",
            Self::SparseX => "sparse-x",
            Self::SparseY => "sparse-y",
            Self::CornerNearMiss => "corner-near-miss",
            Self::Mixed => "mixed",
        }
    }

    fn position(self, index: usize) -> Vec2 {
        match self {
            Self::Dense => Vec2::ZERO,
            Self::EdgeTangent => Vec2::new(1.5, 0.0),
            Self::CornerTangent => Vec2::new(1.5, 1.5),
            Self::SparseX => Vec2::new(4.0, 0.0),
            Self::SparseY => Vec2::new(0.0, 4.0),
            Self::CornerNearMiss => Vec2::new(1.5, 1.500_1),
            Self::Mixed => match index.wrapping_mul(7) % 20 {
                0 => Vec2::ZERO,
                1..=9 => Vec2::new(4.0, 0.0),
                10..=15 => Vec2::new(0.0, 4.0),
                _ => Vec2::new(1.5, 1.500_1),
            },
        }
    }

    fn hits_per_scan(self, candidate_count: usize) -> usize {
        match self {
            Self::Dense | Self::EdgeTangent | Self::CornerTangent => candidate_count,
            Self::Mixed => (0..candidate_count)
                .filter(|index| index.wrapping_mul(7) % 20 == 0)
                .count(),
            Self::SparseX | Self::SparseY | Self::CornerNearMiss => 0,
        }
    }
}

impl CircleOverlapLayout {
    const fn label(self) -> &'static str {
        match self {
            Self::Dense => "dense",
            Self::BoundaryTangent => "boundary-tangent",
            Self::SparseX => "sparse-x",
            Self::SparseY => "sparse-y",
            Self::DiagonalNearMiss => "diagonal-near-miss",
            Self::Mixed => "mixed",
        }
    }

    fn position(self, index: usize) -> Vec2 {
        match self {
            Self::Dense => Vec2::ZERO,
            Self::BoundaryTangent => Vec2::new(1.5, 0.0),
            Self::SparseX => Vec2::new(4.0, 0.0),
            Self::SparseY => Vec2::new(0.0, 4.0),
            Self::DiagonalNearMiss => Vec2::new(1.1, 1.1),
            Self::Mixed => match index.wrapping_mul(7) % 20 {
                0 => Vec2::ZERO,
                1..=9 => Vec2::new(4.0, 0.0),
                10..=15 => Vec2::new(0.0, 4.0),
                _ => Vec2::new(1.1, 1.1),
            },
        }
    }

    fn hits_per_scan(self, candidate_count: usize) -> usize {
        match self {
            Self::Dense | Self::BoundaryTangent => candidate_count,
            Self::Mixed => (0..candidate_count)
                .filter(|index| index.wrapping_mul(7) % 20 == 0)
                .count(),
            Self::SparseX | Self::SparseY | Self::DiagonalNearMiss => 0,
        }
    }
}

fn build_runner(circle_count: usize) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::new(
        10_000,
        SceneBudget::new(
            10_000,
            0,
            2_000_000,
            8 * 1024 * 1024,
            16 * 1024 * 1024,
            256 * 1024 * 1024,
            10_000,
        ),
        FrameLimits::default(),
    ));
    let mut application = Application::<BenchAction>::new(config)?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let circle = CircleVisual::new(0.25, Color::WHITE)?;
    let world = application.register_world("benchmark", move |world| {
        world.spawn(camera)?;
        for _ in 0..circle_count {
            world.spawn((Transform2d::default(), circle))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance(runner: &mut HeadlessRunner<BenchAction>) -> Result<usize, Box<dyn Error>> {
    let report = advance_report(runner)?;
    black_box(report);
    let circles = runner
        .extracted_frame()
        .ok_or_else(|| io::Error::other("benchmark frame was not extracted"))?
        .resolved_circles()
        .len();
    Ok(circles)
}

fn advance_report(
    runner: &mut HeadlessRunner<BenchAction>,
) -> Result<LogicFrameReport, Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(FIXED_STEP, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(io::Error::other("benchmark frame was rejected").into());
    };
    if let Some(failure) = report.failure() {
        return Err(io::Error::other(format!("benchmark frame failed: {failure}")).into());
    }
    if report.fixed_ticks_attempted() != 1 {
        return Err(io::Error::other(format!(
            "benchmark expected one fixed tick, got {}",
            report.fixed_ticks_attempted()
        ))
        .into());
    }
    Ok(report)
}

fn run_case(circle_count: usize) -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut runner = build_runner(circle_count)?;
    let build_elapsed = build_start.elapsed();

    for _ in 0..WARM_UP_FRAMES {
        black_box(advance(&mut runner)?);
    }

    let measure_start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..MEASURED_FRAMES {
        checksum = checksum.wrapping_add(black_box(advance(&mut runner)?));
    }
    let measured = measure_start.elapsed();
    black_box(checksum);

    let nanoseconds_per_frame = measured.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let nanoseconds_per_circle = if circle_count == 0 {
        0.0
    } else {
        nanoseconds_per_frame / circle_count as f64
    };
    println!(
        "circles={circle_count:>5} build_ms={:>9.3} frame_us={:>9.3} ns_per_circle={nanoseconds_per_circle:>9.3}",
        build_elapsed.as_secs_f64() * 1_000.0,
        nanoseconds_per_frame / 1_000.0,
    );
    Ok(())
}

fn build_visual_runner(
    circle_count: usize,
    rectangle_count: usize,
    line_count: usize,
    interleaved_depths: bool,
    zero_line_vectors: bool,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let total = circle_count
        .checked_add(rectangle_count)
        .and_then(|count| count.checked_add(line_count))
        .ok_or_else(|| io::Error::other("visual benchmark entity count overflowed"))?;
    let mut config = AppConfig::default();
    config.set_render_limits(
        RenderLimits::new(
            circle_count,
            SceneBudget::new(
                total,
                0,
                2_000_000,
                8 * 1024 * 1024,
                16 * 1024 * 1024,
                256 * 1024 * 1024,
                total,
            ),
            FrameLimits::default(),
        )
        .with_max_world_rectangles(rectangle_count)
        .with_max_world_lines(line_count),
    );
    let mut application = Application::<BenchAction>::new(config)?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let circle = CircleVisual::new(0.25, Color::WHITE)?;
    let mut rectangle = RectangleVisual::new(Vec2::new(0.5, 0.5), Color::WHITE)?;
    rectangle.set_corner_radius(0.1)?;
    let line_vector = if zero_line_vectors {
        Vec2::ZERO
    } else {
        Vec2::new(1.0, -0.5)
    };
    let line = LineVisual::new(line_vector, 2.0, Color::WHITE)?;
    let world = application.register_world("visual-extraction", move |world| {
        world.spawn(camera)?;
        for index in 0..circle_count {
            let mut visual = circle;
            if interleaved_depths {
                let stride = if line_count == 0 { 2 } else { 3 };
                let offset = usize::from(line_count != 0);
                visual
                    .set_draw_order_depth((index * stride + offset) as f32)
                    .expect("bounded benchmark depth should be finite");
            }
            world.spawn((Transform2d::default(), visual))?;
        }
        for index in 0..rectangle_count {
            let mut visual = rectangle;
            if interleaved_depths {
                let stride = if line_count == 0 { 2 } else { 3 };
                let offset = usize::from(line_count == 0);
                visual
                    .set_draw_order_depth((index * stride + offset) as f32)
                    .expect("bounded benchmark depth should be finite");
            }
            world.spawn((Transform2d::default(), visual))?;
        }
        for index in 0..line_count {
            let mut visual = line;
            if interleaved_depths {
                visual
                    .set_draw_order_depth((index * 3 + 2) as f32)
                    .expect("bounded benchmark depth should be finite");
            }
            world.spawn((Transform2d::default(), visual))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_visual_case(
    label: &str,
    circle_count: usize,
    rectangle_count: usize,
    line_count: usize,
    interleaved_depths: bool,
    zero_line_vectors: bool,
) -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut runner = build_visual_runner(
        circle_count,
        rectangle_count,
        line_count,
        interleaved_depths,
        zero_line_vectors,
    )?;
    let build_elapsed = build_start.elapsed();
    let total = circle_count + rectangle_count + line_count;

    for _ in 0..WARM_UP_FRAMES {
        advance_frame_only(&mut runner)?;
        let extracted = runner
            .extracted_frame()
            .ok_or_else(|| io::Error::other("visual benchmark frame was not extracted"))?;
        black_box((
            extracted.resolved_circles().len(),
            extracted.resolved_rectangles().len(),
            extracted.resolved_lines().len(),
        ));
    }

    let measure_start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..MEASURED_FRAMES {
        advance_frame_only(&mut runner)?;
        let extracted = runner
            .extracted_frame()
            .ok_or_else(|| io::Error::other("visual benchmark frame was not extracted"))?;
        let circles = extracted.resolved_circles().len();
        let rectangles = extracted.resolved_rectangles().len();
        let lines = extracted.resolved_lines().len();
        let expected_lines = if zero_line_vectors { 0 } else { line_count };
        if circles != circle_count || rectangles != rectangle_count || lines != expected_lines {
            return Err(io::Error::other("visual benchmark count changed").into());
        }
        checksum = checksum
            .wrapping_add(circles)
            .wrapping_add(rectangles)
            .wrapping_add(lines);
    }
    let measured = measure_start.elapsed();
    let expected_lines = if zero_line_vectors { 0 } else { line_count };
    let expected_checksum = circle_count
        .saturating_add(rectangle_count)
        .saturating_add(expected_lines)
        .saturating_mul(MEASURED_FRAMES);
    if checksum != expected_checksum {
        return Err(io::Error::other(format!(
            "visual benchmark checksum changed: expected {expected_checksum}, got {checksum}"
        ))
        .into());
    }
    black_box(checksum);
    let nanoseconds_per_frame = measured.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let nanoseconds_per_visual = nanoseconds_per_frame / total as f64;
    println!(
        "visual_extract={label:>18} circles={circle_count:>5} rectangles={rectangle_count:>5} line_sources={line_count:>5} build_ms={:>9.3} frame_us={:>9.3} ns_per_source={nanoseconds_per_visual:>9.3} checksum={checksum}",
        build_elapsed.as_secs_f64() * 1_000.0,
        nanoseconds_per_frame / 1_000.0,
    );
    Ok(())
}

fn run_visual_allocation_case(
    label: &str,
    circle_count: usize,
    rectangle_count: usize,
    line_count: usize,
    interleaved_depths: bool,
    zero_line_vectors: bool,
) -> Result<(), Box<dyn Error>> {
    let mut runner = build_visual_runner(
        circle_count,
        rectangle_count,
        line_count,
        interleaved_depths,
        zero_line_vectors,
    )?;
    // Warm both extraction buffers and the two retained `bevy_ecs` removal
    // tracker generations before measuring the stable-size steady state.
    for _ in 0..WARM_UP_FRAMES {
        advance_frame_only(&mut runner)?;
    }

    let mut per_frame = [0usize; ALLOCATION_CHECK_FRAMES];
    begin_allocation_count();
    for frame_allocations in &mut per_frame {
        let before = allocation_calls();
        advance_frame_only(&mut runner)?;
        *frame_allocations = allocation_calls().saturating_sub(before);
    }
    let calls = end_allocation_count();

    let extracted = runner
        .extracted_frame()
        .ok_or_else(|| io::Error::other("allocation check lost its extracted frame"))?;
    if extracted.resolved_circles().len() != circle_count
        || extracted.resolved_rectangles().len() != rectangle_count
        || extracted.resolved_lines().len() != if zero_line_vectors { 0 } else { line_count }
    {
        return Err(io::Error::other("allocation check visual count changed").into());
    }
    println!(
        "stable_allocations={label:>18} frames={ALLOCATION_CHECK_FRAMES:>3} circles={circle_count:>5} rectangles={rectangle_count:>5} line_sources={line_count:>5} allocation_calls={calls}"
    );
    if calls != 0 {
        println!("allocation_calls_per_frame={per_frame:?}");
        return Err(io::Error::other(format!(
            "warmed stable extraction unexpectedly allocated {calls} times"
        ))
        .into());
    }
    Ok(())
}

fn run_visual_suite() -> Result<(), Box<dyn Error>> {
    run_visual_case("circle-only-10k", 10_000, 0, 0, false, false)?;
    run_visual_case("rectangle-10k", 0, 10_000, 0, false, false)?;
    run_visual_case("line-only-10k", 0, 0, 10_000, false, false)?;
    run_visual_case("zero-lines-10k", 0, 0, 10_000, false, true)?;
    run_visual_case("mixed-5k-5k", 5_000, 5_000, 0, true, false)?;
    run_visual_case("mixed-three-10k", 3_334, 3_333, 3_333, true, false)?;
    run_visual_allocation_case("background-only", 0, 0, 0, false, false)?;
    run_visual_allocation_case("circle-only-10k", 10_000, 0, 0, false, false)?;
    run_visual_allocation_case("rectangle-10k", 0, 10_000, 0, false, false)?;
    run_visual_allocation_case("line-only-10k", 0, 0, 10_000, false, false)?;
    run_visual_allocation_case("zero-lines-10k", 0, 0, 10_000, false, true)?;
    run_visual_allocation_case("mixed-5k-5k", 5_000, 5_000, 0, true, false)?;
    run_visual_allocation_case("mixed-three-10k", 3_334, 3_333, 3_333, true, false)
}

fn advance_fixed_ticks(
    runner: &mut HeadlessRunner<BenchAction>,
    tick_count: u32,
) -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let elapsed = FIXED_STEP.saturating_mul(tick_count);
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(io::Error::other("fixed allocation frame was rejected").into());
    };
    if let Some(failure) = report.failure() {
        return Err(io::Error::other(format!("fixed allocation frame failed: {failure}")).into());
    }
    if report.fixed_ticks_attempted() != tick_count {
        return Err(io::Error::other(format!(
            "fixed allocation frame expected {tick_count} fixed ticks, got {}",
            report.fixed_ticks_attempted()
        ))
        .into());
    }
    black_box(report);
    Ok(())
}

fn run_fixed_frame_allocation_case(label: &str, tick_count: u32) -> Result<(), Box<dyn Error>> {
    let mut runner = build_visual_runner(1, 0, 0, false, false)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_fixed_ticks(&mut runner, tick_count)?;
    }

    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        advance_fixed_ticks(&mut runner, tick_count)?;
    }
    let calls = end_allocation_count();
    println!(
        "stable_fixed_allocations={label:>20} frames={ALLOCATION_CHECK_FRAMES:>3} allocation_calls={calls}"
    );
    if calls != 0 {
        return Err(io::Error::other(format!(
            "warmed stable fixed frames unexpectedly allocated {calls} times"
        ))
        .into());
    }
    Ok(())
}

fn build_empty_fixed_runner(
    transform_count: usize,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(transform_count.saturating_add(1))?;
    let mut application = Application::<BenchAction>::new(config)?;
    let camera = ActiveCamera2d::centered(32.0)?;
    let world = application.register_world("empty-fixed", move |world| {
        world.spawn(camera)?;
        for _ in 0..transform_count {
            world.spawn(Transform2d::default())?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance_empty_fixed(
    runner: &mut HeadlessRunner<BenchAction>,
    elapsed: Duration,
    expected_ticks: u32,
) -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(io::Error::other("empty fixed benchmark frame was rejected").into());
    };
    if let Some(failure) = report.failure() {
        return Err(
            io::Error::other(format!("empty fixed benchmark frame failed: {failure}")).into(),
        );
    }
    if report.fixed_ticks_attempted() != expected_ticks {
        return Err(io::Error::other(format!(
            "empty fixed benchmark expected {expected_ticks} ticks, got {}",
            report.fixed_ticks_attempted()
        ))
        .into());
    }
    black_box(report);
    Ok(())
}

fn run_empty_fixed_pair(
    transform_count: usize,
    constant_tick_delta_ns: f64,
) -> Result<f64, Box<dyn Error>> {
    let mut runner = build_empty_fixed_runner(transform_count)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_empty_fixed(&mut runner, Duration::ZERO, 0)?;
        advance_empty_fixed(&mut runner, FIXED_STEP, 1)?;
    }

    let mut zero_elapsed = Duration::ZERO;
    let mut tick_elapsed = Duration::ZERO;
    for pair in 0..EMPTY_FIXED_PAIR_FRAMES {
        if pair % 2 == 0 {
            let started = Instant::now();
            advance_empty_fixed(&mut runner, Duration::ZERO, 0)?;
            zero_elapsed += started.elapsed();
            let started = Instant::now();
            advance_empty_fixed(&mut runner, FIXED_STEP, 1)?;
            tick_elapsed += started.elapsed();
        } else {
            let started = Instant::now();
            advance_empty_fixed(&mut runner, FIXED_STEP, 1)?;
            tick_elapsed += started.elapsed();
            let started = Instant::now();
            advance_empty_fixed(&mut runner, Duration::ZERO, 0)?;
            zero_elapsed += started.elapsed();
        }
    }

    let zero_ns = zero_elapsed.as_nanos() as f64 / EMPTY_FIXED_PAIR_FRAMES as f64;
    let tick_ns = tick_elapsed.as_nanos() as f64 / EMPTY_FIXED_PAIR_FRAMES as f64;
    let tick_delta_ns = tick_ns - zero_ns;
    let corrected_per_transform = if transform_count == 0 {
        0.0
    } else {
        (tick_delta_ns - constant_tick_delta_ns) / transform_count as f64
    };
    println!(
        "empty_fixed transforms={transform_count:>5} pairs={EMPTY_FIXED_PAIR_FRAMES} zero_us={:>9.3} one_tick_us={:>9.3} tick_delta_us={:>9.3} corrected_ns_per_transform={corrected_per_transform:>9.3}",
        zero_ns / 1_000.0,
        tick_ns / 1_000.0,
        tick_delta_ns / 1_000.0,
    );
    Ok(tick_delta_ns)
}

fn run_empty_fixed_suite() -> Result<(), Box<dyn Error>> {
    let constant_tick_delta_ns = run_empty_fixed_pair(0, 0.0)?;
    for transform_count in [100, 1_000, 10_000] {
        run_empty_fixed_pair(transform_count, constant_tick_delta_ns)?;
    }

    let mut runner = build_empty_fixed_runner(10_000)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_empty_fixed(&mut runner, FIXED_STEP, 1)?;
    }
    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        advance_empty_fixed(&mut runner, FIXED_STEP, 1)?;
    }
    let allocations = end_allocation_count();
    println!(
        "stable_empty_fixed_allocations transforms=10000 frames={ALLOCATION_CHECK_FRAMES} allocation_calls={allocations}"
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed empty fixed stage allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn move_rectangle_until_blocked(
    mut mover: RectangleMoverQuery,
    walls: RectangleWallOverlaps,
    mut checksum: ResMut<RectangleCollisionChecksum>,
) -> Result<(), Box<dyn Error>> {
    let (entity, mut transform, collider) = mover.single_mut()?;
    let proposed = transform.translated_by(Vec2::new(0.25, 0.0))?;
    if walls.has_overlap_with_circle(entity.handle(), &proposed, collider)? {
        checksum.0 = checksum.0.wrapping_add(1);
    } else {
        transform.set_translation(proposed.translation())?;
    }
    Ok(())
}

fn build_rectangle_collision_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    application.approve_components::<(RectangleBenchMover, RectangleBenchWall)>()?;
    application.add_fallible_fixed_system(move_rectangle_until_blocked);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let mover = CircleCollider2d::new(0.5)?;
    let horizontal_wall = RectangleCollider2d::new(Vec2::new(10.0, 1.0))?;
    let vertical_wall = RectangleCollider2d::new(Vec2::new(1.0, 10.0))?;
    let bottom = Transform2d::new(Vec2::new(0.0, -3.0))?;
    let top = Transform2d::new(Vec2::new(0.0, 3.0))?;
    let left = Transform2d::new(Vec2::new(-3.0, 0.0))?;
    let right = Transform2d::new(Vec2::new(3.0, 0.0))?;
    let world = application.register_world("rectangle-collision-allocation", move |world| {
        world.spawn(camera)?;
        world.spawn((RectangleBenchMover, mover))?;
        world.spawn((RectangleBenchWall, bottom, horizontal_wall))?;
        world.spawn((RectangleBenchWall, top, horizontal_wall))?;
        world.spawn((RectangleBenchWall, left, vertical_wall))?;
        world.spawn((RectangleBenchWall, right, vertical_wall))?;
        world.insert_resource(RectangleCollisionChecksum::default())?;
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_rectangle_collision_allocation_case() -> Result<(), Box<dyn Error>> {
    let mut runner = build_rectangle_collision_runner()?;
    for _ in 0..WARM_UP_FRAMES {
        advance_fixed_ticks(&mut runner, 1)?;
    }
    let checksum_before = runner
        .resource::<RectangleCollisionChecksum>()
        .ok_or_else(|| io::Error::other("rectangle collision checksum disappeared"))?
        .0;

    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        advance_fixed_ticks(&mut runner, 1)?;
    }
    let calls = end_allocation_count();
    let checksum_delta = runner
        .resource::<RectangleCollisionChecksum>()
        .ok_or_else(|| io::Error::other("rectangle collision checksum disappeared"))?
        .0
        .wrapping_sub(checksum_before);
    let mover = runner
        .components::<RectangleBenchMover>()
        .next()
        .map(|(entity, _)| entity)
        .ok_or_else(|| io::Error::other("rectangle collision mover disappeared"))?;
    let position = runner.component::<Transform2d>(mover)?.translation();
    black_box((checksum_delta, position));
    println!(
        "stable_mixed_collision frames={ALLOCATION_CHECK_FRAMES:>3} walls=4 circle_bytes={} rectangle_bytes={} allocation_calls={calls} blocked_checksum={checksum_delta}",
        std::mem::size_of::<CircleCollider2d>(),
        std::mem::size_of::<RectangleCollider2d>()
    );
    if checksum_delta != ALLOCATION_CHECK_FRAMES as u64 {
        return Err(io::Error::other(format!(
            "rectangle collision expected {ALLOCATION_CHECK_FRAMES} blocked moves, got {checksum_delta}"
        ))
        .into());
    }
    if (position.x() - 1.75).abs() > f32::EPSILON || position.y() != 0.0 {
        return Err(
            io::Error::other(format!("rectangle collision mover escaped to {position:?}")).into(),
        );
    }
    if calls != 0 {
        return Err(io::Error::other(format!(
            "warmed mixed collision unexpectedly allocated {calls} times"
        ))
        .into());
    }
    Ok(())
}

fn build_linear_motion_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    application.approve_component::<LinearMotionBenchBody>()?;
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(32.0)?;
    let velocity = LinearVelocity2d::new(LINEAR_MOTION_VELOCITY)?;
    let world = application.register_world("linear-motion", move |world| {
        world.spawn(camera)?;
        for _ in 0..LINEAR_MOTION_ENTITIES {
            world.spawn((LinearMotionBenchBody, velocity))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_linear_motion_case() -> Result<(), Box<dyn Error>> {
    let mut runner = build_linear_motion_runner()?;
    for _ in 0..WARM_UP_FRAMES {
        black_box(advance_report(&mut runner)?);
    }

    begin_allocation_count();
    let measure_start = Instant::now();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            black_box(advance_report(&mut runner)?);
        }
        Ok(())
    })();
    let measured = measure_start.elapsed();
    let allocations = end_allocation_count();
    measured_result?;

    let step = LINEAR_MOTION_VELOCITY * FIXED_STEP.as_secs_f32();
    let mut expected_previous = Vec2::ZERO;
    let mut expected_current = Vec2::ZERO;
    for _ in 0..(WARM_UP_FRAMES + ALLOCATION_CHECK_FRAMES) {
        expected_previous = expected_current;
        expected_current += step;
    }
    let mut transform_count = 0usize;
    for (_, transform) in runner.components::<Transform2d>() {
        let current = transform.translation();
        let previous = transform.previous_translation();
        if current.x().to_bits() != expected_current.x().to_bits()
            || current.y().to_bits() != expected_current.y().to_bits()
            || previous.x().to_bits() != expected_previous.x().to_bits()
            || previous.y().to_bits() != expected_previous.y().to_bits()
        {
            return Err(io::Error::other(
                "linear-motion benchmark body did not match exact current/previous endpoints",
            )
            .into());
        }
        transform_count += 1;
    }
    let velocity_count = runner.components::<LinearVelocity2d>().count();
    if transform_count != LINEAR_MOTION_ENTITIES || velocity_count != LINEAR_MOTION_ENTITIES {
        return Err(io::Error::other(format!(
            "linear-motion cardinality was transforms={transform_count}, velocities={velocity_count}"
        ))
        .into());
    }
    println!(
        "stable_linear_motion frames={ALLOCATION_CHECK_FRAMES} entities={LINEAR_MOTION_ENTITIES} frame_us={:.3} allocation_calls={allocations} final={expected_current:?}",
        measured.as_secs_f64() * 1_000_000.0 / ALLOCATION_CHECK_FRAMES as f64,
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed linear-motion frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn build_acceleration_motion_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    application.approve_component::<AccelerationMotionBenchBody>()?;
    application.add_linear_acceleration2d_system();
    application.add_linear_velocity2d_system();
    let camera = ActiveCamera2d::centered(32.0)?;
    let acceleration = LinearAcceleration2d::new(ACCELERATION_MOTION_ACCELERATION)?;
    let velocity = LinearVelocity2d::new(ACCELERATION_MOTION_INITIAL_VELOCITY)?;
    let world = application.register_world("acceleration-motion", move |world| {
        world.spawn(camera)?;
        for _ in 0..ACCELERATION_MOTION_ENTITIES {
            world.spawn((AccelerationMotionBenchBody, acceleration, velocity))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_acceleration_motion_case() -> Result<(), Box<dyn Error>> {
    let mut runner = build_acceleration_motion_runner()?;
    for _ in 0..WARM_UP_FRAMES {
        black_box(advance_report(&mut runner)?);
    }

    begin_allocation_count();
    let measure_start = Instant::now();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            black_box(advance_report(&mut runner)?);
        }
        Ok(())
    })();
    let measured = measure_start.elapsed();
    let allocations = end_allocation_count();
    measured_result?;

    let seconds = FIXED_STEP.as_secs_f32();
    let mut expected_velocity = ACCELERATION_MOTION_INITIAL_VELOCITY;
    let mut expected_previous = Vec2::ZERO;
    let mut expected_current = Vec2::ZERO;
    for _ in 0..(WARM_UP_FRAMES + ALLOCATION_CHECK_FRAMES) {
        expected_velocity += ACCELERATION_MOTION_ACCELERATION * seconds;
        expected_previous = expected_current;
        expected_current += expected_velocity * seconds;
    }

    let mut transform_count = 0usize;
    for (entity, transform) in runner.components::<Transform2d>() {
        let current = transform.translation();
        let previous = transform.previous_translation();
        if current.x().to_bits() != expected_current.x().to_bits()
            || current.y().to_bits() != expected_current.y().to_bits()
            || previous.x().to_bits() != expected_previous.x().to_bits()
            || previous.y().to_bits() != expected_previous.y().to_bits()
        {
            return Err(io::Error::other(
                "acceleration benchmark body did not match exact current/previous endpoints",
            )
            .into());
        }
        let velocity = runner.component::<LinearVelocity2d>(entity)?.velocity();
        if velocity.x().to_bits() != expected_velocity.x().to_bits()
            || velocity.y().to_bits() != expected_velocity.y().to_bits()
        {
            return Err(io::Error::other(
                "acceleration benchmark body did not match exact current velocity",
            )
            .into());
        }
        transform_count += 1;
    }
    let velocity_count = runner.components::<LinearVelocity2d>().count();
    let acceleration_count = runner.components::<LinearAcceleration2d>().count();
    if transform_count != ACCELERATION_MOTION_ENTITIES
        || velocity_count != ACCELERATION_MOTION_ENTITIES
        || acceleration_count != ACCELERATION_MOTION_ENTITIES
    {
        return Err(io::Error::other(format!(
            "acceleration-motion cardinality was transforms={transform_count}, velocities={velocity_count}, accelerations={acceleration_count}"
        ))
        .into());
    }
    println!(
        "stable_acceleration_motion frames={ALLOCATION_CHECK_FRAMES} entities={ACCELERATION_MOTION_ENTITIES} frame_us={:.3} allocation_calls={allocations} velocity={expected_velocity:?} position={expected_current:?}",
        measured.as_secs_f64() * 1_000_000.0 / ALLOCATION_CHECK_FRAMES as f64,
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed acceleration-motion frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn build_digital_motion_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    application.approve_component::<DigitalMotionBenchBody>()?;
    application.bind_wasd(INPUT_AXIS)?;
    application.add_digital_movement2d_system();
    let camera = ActiveCamera2d::centered(32.0)?;
    let movement = DigitalMovement2d::new(INPUT_AXIS, DIGITAL_MOTION_SPEED)?;
    let world = application.register_world("digital-motion", move |world| {
        world.spawn(camera)?;
        for _ in 0..DIGITAL_MOTION_ENTITIES {
            world.spawn((DigitalMotionBenchBody, movement))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_digital_motion_case() -> Result<(), Box<dyn Error>> {
    let mut runner = build_digital_motion_runner()?;
    let pressed = [
        InputEvent::key(PhysicalKeyCode::KeyD, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyW, ButtonState::Pressed),
    ];
    advance_input_snapshot_frame(&mut runner, Duration::ZERO, &pressed, 0)?;
    for _ in 0..WARM_UP_FRAMES {
        black_box(advance_report(&mut runner)?);
    }

    begin_allocation_count();
    let measure_start = Instant::now();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            black_box(advance_report(&mut runner)?);
        }
        Ok(())
    })();
    let measured = measure_start.elapsed();
    let allocations = end_allocation_count();
    measured_result?;

    let direction = Vec2::ONE.normalized();
    let step = direction * (DIGITAL_MOTION_SPEED * FIXED_STEP.as_secs_f32());
    let mut expected_previous = Vec2::ZERO;
    let mut expected_current = Vec2::ZERO;
    for _ in 0..(WARM_UP_FRAMES + ALLOCATION_CHECK_FRAMES) {
        expected_previous = expected_current;
        expected_current += step;
    }
    let mut transform_count = 0usize;
    for (_, transform) in runner.components::<Transform2d>() {
        let current = transform.translation();
        let previous = transform.previous_translation();
        if current.x().to_bits() != expected_current.x().to_bits()
            || current.y().to_bits() != expected_current.y().to_bits()
            || previous.x().to_bits() != expected_previous.x().to_bits()
            || previous.y().to_bits() != expected_previous.y().to_bits()
        {
            return Err(io::Error::other(
                "digital-motion benchmark body did not match exact current/previous endpoints",
            )
            .into());
        }
        transform_count += 1;
    }
    let movement_count = runner
        .components::<DigitalMovement2d<BenchAction>>()
        .count();
    if transform_count != DIGITAL_MOTION_ENTITIES || movement_count != DIGITAL_MOTION_ENTITIES {
        return Err(io::Error::other(format!(
            "digital-motion cardinality was transforms={transform_count}, movements={movement_count}"
        ))
        .into());
    }
    println!(
        "stable_digital_motion frames={ALLOCATION_CHECK_FRAMES} entities={DIGITAL_MOTION_ENTITIES} frame_us={:.3} allocation_calls={allocations} final={expected_current:?}",
        measured.as_secs_f64() * 1_000_000.0 / ALLOCATION_CHECK_FRAMES as f64,
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed digital-motion frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

#[derive(Resource, Default)]
struct InputSnapshotChecksum(u64);

fn observe_fixed_input(
    input: FixedInput<BenchAction>,
    mut checksum: ResMut<InputSnapshotChecksum>,
) {
    checksum.0 = checksum
        .0
        .wrapping_add(input.held(BenchAction::Move) as u64)
        .wrapping_add((input.normalized_digital_axis(INPUT_AXIS) == Vec2::Y) as u64)
        .wrapping_add(input.has_press_occurrence(BenchAction::Move) as u64)
        .wrapping_add(input.has_release_occurrence(BenchAction::Move) as u64)
        .wrapping_add(input.pressed(BenchAction::Move).count() as u64)
        .wrapping_add(input.released(BenchAction::Move).count() as u64);
}

fn observe_frame_input(
    input: FrameInput<BenchAction>,
    mut checksum: ResMut<InputSnapshotChecksum>,
) {
    checksum.0 = checksum
        .0
        .wrapping_add(input.held(BenchAction::Move) as u64)
        .wrapping_add((input.normalized_digital_axis(INPUT_AXIS) == Vec2::Y) as u64)
        .wrapping_add(input.has_press_occurrence(BenchAction::Move) as u64)
        .wrapping_add(input.has_release_occurrence(BenchAction::Move) as u64)
        .wrapping_add(input.pressed(BenchAction::Move).count() as u64)
        .wrapping_add(input.released(BenchAction::Move).count() as u64);
}

fn build_input_snapshot_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    application.bind_key(PhysicalKeyCode::KeyW, BenchAction::Move)?;
    application.bind_key(PhysicalKeyCode::ArrowUp, BenchAction::Move)?;
    application.add_system(Stage::FixedUpdate, observe_fixed_input);
    application.add_system(Stage::FrameUpdate, observe_frame_input);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 20.0)?);
    let world = application.register_world("input-snapshot-allocation", move |world| {
        world.spawn(camera)?;
        world.insert_resource(InputSnapshotChecksum::default())?;
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance_input_snapshot_frame(
    runner: &mut HeadlessRunner<BenchAction>,
    elapsed: Duration,
    events: &[InputEvent],
    expected_ticks: u32,
) -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(elapsed, events, viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(io::Error::other("input allocation frame was rejected").into());
    };
    if let Some(failure) = report.failure() {
        return Err(io::Error::other(format!("input allocation frame failed: {failure}")).into());
    }
    if report.fixed_ticks_attempted() != expected_ticks {
        return Err(io::Error::other(format!(
            "input allocation frame expected {expected_ticks} fixed ticks, got {}",
            report.fixed_ticks_attempted()
        ))
        .into());
    }
    black_box(report);
    Ok(())
}

fn input_checksum(runner: &HeadlessRunner<BenchAction>) -> Result<u64, Box<dyn Error>> {
    Ok(runner
        .resource::<InputSnapshotChecksum>()
        .ok_or_else(|| io::Error::other("input allocation checksum disappeared"))?
        .0)
}

fn verify_input_allocation_case(
    label: &str,
    calls: usize,
    checksum_delta: u64,
    expected_checksum_delta: u64,
) -> Result<(), Box<dyn Error>> {
    println!(
        "stable_input_allocations={label:>20} frames={ALLOCATION_CHECK_FRAMES:>3} allocation_calls={calls} checksum_delta={checksum_delta}"
    );
    if checksum_delta != expected_checksum_delta {
        return Err(io::Error::other(format!(
            "input allocation case expected checksum delta {expected_checksum_delta}, got {checksum_delta}"
        ))
        .into());
    }
    if calls != 0 {
        return Err(io::Error::other(format!(
            "warmed stable input frames unexpectedly allocated {calls} times"
        ))
        .into());
    }
    Ok(())
}

fn run_held_input_allocation_case(
    label: &str,
    key: PhysicalKeyCode,
    elapsed: Duration,
    expected_ticks: u32,
    expected_checksum_delta: u64,
) -> Result<(), Box<dyn Error>> {
    let mut runner = build_input_snapshot_runner()?;
    let pressed = [InputEvent::key(key, ButtonState::Pressed)];
    advance_input_snapshot_frame(&mut runner, elapsed, &pressed, expected_ticks)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_input_snapshot_frame(&mut runner, elapsed, &[], expected_ticks)?;
    }
    let checksum_before = input_checksum(&runner)?;

    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        advance_input_snapshot_frame(&mut runner, elapsed, &[], expected_ticks)?;
    }
    let calls = end_allocation_count();
    let checksum_delta = input_checksum(&runner)?.wrapping_sub(checksum_before);
    black_box(checksum_delta);
    verify_input_allocation_case(label, calls, checksum_delta, expected_checksum_delta)
}

fn run_edge_input_allocation_case(
    label: &str,
    key: PhysicalKeyCode,
    elapsed: Duration,
    expected_ticks: u32,
    expected_checksum_delta: u64,
) -> Result<(), Box<dyn Error>> {
    let mut runner = build_input_snapshot_runner()?;
    let mut pressed = false;
    for _ in 0..WARM_UP_FRAMES {
        pressed = !pressed;
        let event = [InputEvent::key(
            key,
            if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
        )];
        advance_input_snapshot_frame(&mut runner, elapsed, &event, expected_ticks)?;
    }
    let checksum_before = input_checksum(&runner)?;

    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        pressed = !pressed;
        let event = [InputEvent::key(
            key,
            if pressed {
                ButtonState::Pressed
            } else {
                ButtonState::Released
            },
        )];
        advance_input_snapshot_frame(&mut runner, elapsed, &event, expected_ticks)?;
    }
    let calls = end_allocation_count();
    let checksum_delta = input_checksum(&runner)?.wrapping_sub(checksum_before);
    black_box(checksum_delta);
    verify_input_allocation_case(label, calls, checksum_delta, expected_checksum_delta)
}

fn run_input_allocation_suite() -> Result<(), Box<dyn Error>> {
    for (label, key) in [
        ("wasd", PhysicalKeyCode::KeyW),
        ("arrow", PhysicalKeyCode::ArrowUp),
    ] {
        run_held_input_allocation_case(&format!("{label}-held-zero"), key, Duration::ZERO, 0, 200)?;
        run_held_input_allocation_case(&format!("{label}-held-one"), key, FIXED_STEP, 1, 400)?;
        run_edge_input_allocation_case(&format!("{label}-edge-one"), key, FIXED_STEP, 1, 600)?;
        run_edge_input_allocation_case(
            &format!("{label}-edge-four"),
            key,
            FIXED_STEP.saturating_mul(4),
            4,
            900,
        )?;
    }
    Ok(())
}

fn build_managed_idle_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(MANAGED_IDLE_ENTITIES)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_component::<IdleMarker>()?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("managed-idle-100k", move |world| {
        world.spawn(camera)?;
        for _ in 1..MANAGED_IDLE_ENTITIES {
            world.spawn(IdleMarker)?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_managed_idle_case() -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut runner = build_managed_idle_runner()?;
    let build_elapsed = build_start.elapsed();
    let idle_entities = runner.components::<IdleMarker>().count();
    if idle_entities + 1 != MANAGED_IDLE_ENTITIES {
        return Err(io::Error::other(format!(
            "managed idle build produced {idle_entities} markers"
        ))
        .into());
    }

    for _ in 0..WARM_UP_FRAMES {
        black_box(advance(&mut runner)?);
    }

    let measure_start = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..MEASURED_FRAMES {
        checksum = checksum.wrapping_add(black_box(advance(&mut runner)?));
    }
    let measured = measure_start.elapsed();
    black_box(checksum);

    let nanoseconds_per_frame = measured.as_nanos() as f64 / MEASURED_FRAMES as f64;
    println!(
        "managed_idle_100k entities={MANAGED_IDLE_ENTITIES} circles=0 build_ms={:>9.3} frame_us={:>9.3}",
        build_elapsed.as_secs_f64() * 1_000.0,
        nanoseconds_per_frame / 1_000.0,
    );
    Ok(())
}

fn build_spatial_runner(
    implicit_transform: bool,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(SPATIAL_BUILD_ENTITIES + 1)?;
    let mut application = Application::<BenchAction>::new(config)?;
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let collider = CircleCollider2d::new(0.25)?;
    let world = application.register_world("spatial-build", move |world| {
        world.spawn(camera)?;
        if implicit_transform {
            for _ in 0..SPATIAL_BUILD_ENTITIES {
                world.spawn(collider)?;
            }
        } else {
            for _ in 0..SPATIAL_BUILD_ENTITIES {
                world.spawn((Transform2d::default(), collider))?;
            }
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn measure_spatial_build(implicit_transform: bool) -> Result<Duration, Box<dyn Error>> {
    let started = Instant::now();
    let runner = build_spatial_runner(implicit_transform)?;
    let elapsed = started.elapsed();
    if runner.components::<Transform2d>().count() != SPATIAL_BUILD_ENTITIES
        || runner.components::<CircleCollider2d>().count() != SPATIAL_BUILD_ENTITIES
    {
        return Err(io::Error::other("spatial build produced unexpected component counts").into());
    }
    Ok(elapsed)
}

fn run_spatial_build_case() -> Result<(), Box<dyn Error>> {
    let mut implicit = Vec::with_capacity(SPATIAL_BUILD_SAMPLES);
    let mut explicit = Vec::with_capacity(SPATIAL_BUILD_SAMPLES);
    for sample in 0..SPATIAL_BUILD_SAMPLES {
        if sample % 2 == 0 {
            implicit.push(measure_spatial_build(true)?);
            explicit.push(measure_spatial_build(false)?);
        } else {
            explicit.push(measure_spatial_build(false)?);
            implicit.push(measure_spatial_build(true)?);
        }
    }
    implicit.sort_unstable();
    explicit.sort_unstable();
    let implicit = implicit[SPATIAL_BUILD_SAMPLES / 2];
    let explicit = explicit[SPATIAL_BUILD_SAMPLES / 2];
    let relative = implicit.as_secs_f64() / explicit.as_secs_f64();
    println!(
        "spatial_build_100k entities={SPATIAL_BUILD_ENTITIES} samples={SPATIAL_BUILD_SAMPLES} implicit_ms={:>9.3} explicit_ms={:>9.3} implicit_over_explicit={relative:>7.3}",
        implicit.as_secs_f64() * 1_000.0,
        explicit.as_secs_f64() * 1_000.0,
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum ChurnSpawnMode {
    Individual,
    Copies,
}

fn churn_system(
    mode: ChurnSpawnMode,
    mut commands: Commands,
    entities: Query<LogicEntityRef, With<ChurnMarker>>,
) {
    for entity in entities.iter().take(CHURN_PER_TICK) {
        commands
            .despawn(entity.handle())
            .expect("churn command budget should fit all despawns");
    }
    let marker = ChurnMarker {
        _payload: black_box(1),
    };
    match mode {
        ChurnSpawnMode::Individual => {
            for _ in 0..CHURN_PER_TICK {
                commands
                    .spawn(marker)
                    .expect("churn command budget should fit all singular spawns");
            }
        }
        ChurnSpawnMode::Copies => commands
            .spawn_copies(marker, CHURN_PER_TICK)
            .expect("churn command budget should fit the repeated spawn"),
    }
}

fn build_churn_runner(mode: ChurnSpawnMode) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(CHURN_LIVE_ENTITIES)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_component::<ChurnMarker>()?;
    application.add_system(
        Stage::FixedUpdate,
        move |commands: Commands, entities: Query<LogicEntityRef, With<ChurnMarker>>| {
            churn_system(mode, commands, entities);
        },
    );
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("balanced-churn-10k", move |world| {
        world.spawn(camera)?;
        for _ in 1..CHURN_LIVE_ENTITIES {
            world.spawn(ChurnMarker { _payload: 1 })?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance_churn(runner: &mut HeadlessRunner<BenchAction>) -> Result<usize, Box<dyn Error>> {
    let report = advance_report(runner)?;
    if report.spawned() != CHURN_PER_TICK || report.despawned() != CHURN_PER_TICK {
        return Err(io::Error::other(format!(
            "unbalanced churn: spawned={}, despawned={}",
            report.spawned(),
            report.despawned()
        ))
        .into());
    }
    Ok(report.spawned().wrapping_add(report.despawned()))
}

fn verify_churn_state(
    runner: &HeadlessRunner<BenchAction>,
    label: &str,
) -> Result<(), Box<dyn Error>> {
    let mut markers = 0usize;
    for (_entity, marker) in runner.components::<ChurnMarker>() {
        markers += 1;
        if marker._payload != 1 {
            return Err(io::Error::other(format!(
                "{label} retained an unexpected marker payload {}",
                marker._payload
            ))
            .into());
        }
    }
    let live_entities = markers + 1;
    if live_entities != CHURN_LIVE_ENTITIES {
        return Err(
            io::Error::other(format!("{label} retained {live_entities} managed entities")).into(),
        );
    }
    Ok(())
}

fn measure_churn_frames(
    runner: &mut HeadlessRunner<BenchAction>,
) -> Result<Duration, Box<dyn Error>> {
    let started = Instant::now();
    let mut checksum = 0usize;
    for _ in 0..MEASURED_FRAMES {
        checksum = checksum.wrapping_add(black_box(advance_churn(runner)?));
    }
    let measured = started.elapsed();
    let expected = MEASURED_FRAMES * CHURN_PER_TICK.saturating_mul(2);
    if checksum != expected {
        return Err(io::Error::other(format!(
            "balanced churn timing checksum {checksum} did not match {expected}"
        ))
        .into());
    }
    black_box(checksum);
    Ok(measured)
}

fn measure_churn_allocations(
    runner: &mut HeadlessRunner<BenchAction>,
) -> Result<usize, Box<dyn Error>> {
    begin_allocation_count();
    let mut checksum = 0usize;
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        checksum = checksum.wrapping_add(advance_churn(runner)?);
    }
    let allocations = end_allocation_count();
    let expected = ALLOCATION_CHECK_FRAMES * CHURN_PER_TICK.saturating_mul(2);
    if checksum != expected {
        return Err(io::Error::other(format!(
            "balanced churn allocation checksum {checksum} did not match {expected}"
        ))
        .into());
    }
    black_box(checksum);
    Ok(allocations)
}

fn run_churn_case() -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut individual = build_churn_runner(ChurnSpawnMode::Individual)?;
    let individual_build = build_start.elapsed();
    let build_start = Instant::now();
    let mut copies = build_churn_runner(ChurnSpawnMode::Copies)?;
    let copies_build = build_start.elapsed();

    for _ in 0..WARM_UP_FRAMES {
        black_box(advance_churn(&mut individual)?);
        black_box(advance_churn(&mut copies)?);
    }

    let individual_allocations = measure_churn_allocations(&mut individual)?;
    let expected_individual_allocations = ALLOCATION_CHECK_FRAMES * CHURN_PER_TICK;
    if individual_allocations != expected_individual_allocations {
        return Err(io::Error::other(format!(
            "singular churn allocated {individual_allocations} times after warmup; expected {expected_individual_allocations} boxed spawn payloads"
        ))
        .into());
    }
    let copy_allocations = measure_churn_allocations(&mut copies)?;
    if copy_allocations != ALLOCATION_CHECK_FRAMES {
        return Err(io::Error::other(format!(
            "copy churn allocated {copy_allocations} times after warmup; expected one non-ZST erased payload for each of {ALLOCATION_CHECK_FRAMES} calls"
        ))
        .into());
    }

    verify_churn_state(&individual, "singular churn")?;
    verify_churn_state(&copies, "copy churn")?;

    let mut individual_samples = Vec::with_capacity(CHURN_TIMING_SAMPLES);
    let mut copy_samples = Vec::with_capacity(CHURN_TIMING_SAMPLES);
    for sample in 0..CHURN_TIMING_SAMPLES {
        if sample % 2 == 0 {
            individual_samples.push(measure_churn_frames(&mut individual)?);
            copy_samples.push(measure_churn_frames(&mut copies)?);
        } else {
            copy_samples.push(measure_churn_frames(&mut copies)?);
            individual_samples.push(measure_churn_frames(&mut individual)?);
        }
    }
    individual_samples.sort_unstable();
    copy_samples.sort_unstable();
    let individual_median = individual_samples[CHURN_TIMING_SAMPLES / 2];
    let copy_median = copy_samples[CHURN_TIMING_SAMPLES / 2];
    verify_churn_state(&individual, "timed singular churn")?;
    verify_churn_state(&copies, "timed copy churn")?;

    let individual_ns_per_frame = individual_median.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let copy_ns_per_frame = copy_median.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let signed_copy_delta_per_spawn =
        (copy_ns_per_frame - individual_ns_per_frame) / CHURN_PER_TICK as f64;
    println!(
        "balanced_churn managed_live={CHURN_LIVE_ENTITIES} despawn_per_tick={CHURN_PER_TICK} spawn_per_tick={CHURN_PER_TICK} allocation_frames={ALLOCATION_CHECK_FRAMES} individual_allocations={individual_allocations} copy_allocations={copy_allocations} individual_build_ms={:>9.3} copy_build_ms={:>9.3} individual_median_frame_us={:>9.3} copy_median_frame_us={:>9.3} signed_copy_minus_individual_ns_per_spawn={signed_copy_delta_per_spawn:>9.3}",
        individual_build.as_secs_f64() * 1_000.0,
        copies_build.as_secs_f64() * 1_000.0,
        individual_ns_per_frame / 1_000.0,
        copy_ns_per_frame / 1_000.0,
    );
    Ok(())
}

fn build_despawn_only_runner()
-> Result<(HeadlessRunner<BenchAction>, Arc<AtomicBool>), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(DESPAWN_ONLY_ENTITIES + 1)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_component::<DespawnOnlyMarker>()?;
    let refill = Arc::new(AtomicBool::new(false));
    let refill_system = Arc::clone(&refill);
    application.add_system(
        Stage::FixedUpdate,
        move |mut commands: Commands, entities: Query<LogicEntityRef, With<DespawnOnlyMarker>>| {
            if refill_system.load(Ordering::Relaxed) {
                for _ in 0..DESPAWN_ONLY_PER_TICK {
                    commands
                        .spawn(DespawnOnlyMarker)
                        .expect("despawn-only refill should fit the command budget");
                }
            } else {
                for entity in entities.iter().take(DESPAWN_ONLY_PER_TICK) {
                    commands
                        .despawn(entity.handle())
                        .expect("despawn-only command budget should fit every despawn");
                }
            }
        },
    );
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("despawn-only", move |world| {
        world.spawn(camera)?;
        for _ in 0..DESPAWN_ONLY_ENTITIES {
            world.spawn(DespawnOnlyMarker)?;
        }
        Ok(())
    })?;
    Ok((application.build_headless(world)?, refill))
}

fn advance_despawn_only(
    runner: &mut HeadlessRunner<BenchAction>,
    expected_spawned: usize,
    expected_despawned: usize,
) -> Result<usize, Box<dyn Error>> {
    let report = advance_report(runner)?;
    if report.spawned() != expected_spawned || report.despawned() != expected_despawned {
        return Err(io::Error::other(format!(
            "unexpected despawn-only batch: spawned={}, despawned={}",
            report.spawned(),
            report.despawned()
        ))
        .into());
    }
    Ok(report.spawned().wrapping_add(report.despawned()))
}

fn run_despawn_only_allocation_case() -> Result<(), Box<dyn Error>> {
    let (mut runner, refill) = build_despawn_only_runner()?;

    // Prime Bevy's entity free-buffer high-water mark outside the measured
    // window, then consume those free slots while preserving its capacity.
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        black_box(advance_despawn_only(&mut runner, 0, DESPAWN_ONLY_PER_TICK)?);
    }
    refill.store(true, Ordering::Relaxed);
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        black_box(advance_despawn_only(&mut runner, DESPAWN_ONLY_PER_TICK, 0)?);
    }
    refill.store(false, Ordering::Relaxed);

    begin_allocation_count();
    let mut checksum = 0usize;
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        checksum =
            checksum.wrapping_add(advance_despawn_only(&mut runner, 0, DESPAWN_ONLY_PER_TICK)?);
    }
    let allocations = end_allocation_count();
    black_box(checksum);
    let expected_checksum = ALLOCATION_CHECK_FRAMES * DESPAWN_ONLY_PER_TICK;
    if checksum != expected_checksum {
        return Err(io::Error::other(format!(
            "despawn-only checksum {checksum} did not match {expected_checksum}"
        ))
        .into());
    }
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "despawn-only command processing allocated {allocations} times after preconditioning"
        ))
        .into());
    }

    let remaining = runner.components::<DespawnOnlyMarker>().count();
    if remaining != 0 {
        return Err(io::Error::other(format!(
            "despawn-only benchmark retained {remaining} marker entities"
        ))
        .into());
    }
    println!(
        "despawn_only despawn_per_tick={DESPAWN_ONLY_PER_TICK} allocation_frames={ALLOCATION_CHECK_FRAMES} allocations={allocations}"
    );
    Ok(())
}

fn replace_insert_targets(
    mut commands: Commands,
    targets: Query<(LogicEntityRef, &InsertBenchValue), With<InsertBenchTarget>>,
    mut checksum: ResMut<InsertBenchChecksum>,
) {
    let mut tick_checksum = 0u64;
    for (target, value) in &targets {
        tick_checksum = tick_checksum.wrapping_add(value.0);
        commands
            .insert(target.handle(), InsertBenchValue(value.0.wrapping_add(1)))
            .expect("insert benchmark command budget should fit every target");
    }
    checksum.0 = checksum.0.wrapping_add(black_box(tick_checksum));
}

fn scan_insert_targets(
    targets: Query<(LogicEntityRef, &InsertBenchValue), With<InsertBenchTarget>>,
    mut checksum: ResMut<InsertBenchChecksum>,
) {
    let mut tick_checksum = 0u64;
    for (target, value) in &targets {
        black_box(target.handle());
        tick_checksum = tick_checksum.wrapping_add(value.0);
    }
    checksum.0 = checksum.0.wrapping_add(black_box(tick_checksum));
}

fn build_insert_runner(apply_inserts: bool) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_command_limit(INSERT_TARGETS)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(InsertBenchTarget, InsertBenchValue)>()?;
    if apply_inserts {
        application.add_system(Stage::FixedUpdate, replace_insert_targets);
    } else {
        application.add_system(Stage::FixedUpdate, scan_insert_targets);
    }
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("component-insert-1k", move |world| {
        world.spawn(camera)?;
        world.insert_resource(InsertBenchChecksum::default())?;
        for _ in 0..INSERT_TARGETS {
            world.spawn((InsertBenchTarget, InsertBenchValue(0)))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance_insert(runner: &mut HeadlessRunner<BenchAction>) -> Result<(), Box<dyn Error>> {
    let report = advance_report(runner)?;
    if report.spawned() != 0 || report.despawned() != 0 {
        return Err(io::Error::other(format!(
            "component insert changed entity counts: spawned={}, despawned={}",
            report.spawned(),
            report.despawned()
        ))
        .into());
    }
    Ok(())
}

fn verify_insert_values(
    runner: &HeadlessRunner<BenchAction>,
    expected: u64,
) -> Result<(), Box<dyn Error>> {
    let mut count = 0usize;
    for (_, value) in runner.components::<InsertBenchValue>() {
        if value.0 != expected {
            return Err(io::Error::other(format!(
                "component insert retained value {}, expected {expected}",
                value.0
            ))
            .into());
        }
        count += 1;
    }
    if count != INSERT_TARGETS {
        return Err(io::Error::other(format!(
            "component insert retained {count} targets, expected {INSERT_TARGETS}"
        ))
        .into());
    }
    Ok(())
}

fn run_insert_case() -> Result<(), Box<dyn Error>> {
    let mut inserted = build_insert_runner(true)?;
    let mut baseline = build_insert_runner(false)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_insert(&mut inserted)?;
        advance_insert(&mut baseline)?;
    }

    begin_allocation_count();
    for _ in 0..ALLOCATION_CHECK_FRAMES {
        advance_insert(&mut inserted)?;
    }
    let allocations = end_allocation_count();
    let expected_allocations = ALLOCATION_CHECK_FRAMES * INSERT_TARGETS;
    if allocations != expected_allocations {
        return Err(io::Error::other(format!(
            "component insert allocated {allocations} times after warmup; expected exactly {expected_allocations} boxed payloads"
        ))
        .into());
    }
    verify_insert_values(
        &inserted,
        u64::try_from(WARM_UP_FRAMES + ALLOCATION_CHECK_FRAMES)?,
    )?;

    let mut inserted_elapsed = Duration::ZERO;
    let mut baseline_elapsed = Duration::ZERO;
    for sample in 0..MEASURED_FRAMES {
        if sample % 2 == 0 {
            let started = Instant::now();
            advance_insert(&mut inserted)?;
            inserted_elapsed = inserted_elapsed.saturating_add(started.elapsed());
            let started = Instant::now();
            advance_insert(&mut baseline)?;
            baseline_elapsed = baseline_elapsed.saturating_add(started.elapsed());
        } else {
            let started = Instant::now();
            advance_insert(&mut baseline)?;
            baseline_elapsed = baseline_elapsed.saturating_add(started.elapsed());
            let started = Instant::now();
            advance_insert(&mut inserted)?;
            inserted_elapsed = inserted_elapsed.saturating_add(started.elapsed());
        }
    }
    verify_insert_values(
        &inserted,
        u64::try_from(WARM_UP_FRAMES + ALLOCATION_CHECK_FRAMES + MEASURED_FRAMES)?,
    )?;
    black_box(
        inserted
            .resource::<InsertBenchChecksum>()
            .ok_or_else(|| io::Error::other("component insert checksum disappeared"))?
            .0,
    );
    black_box(
        baseline
            .resource::<InsertBenchChecksum>()
            .ok_or_else(|| io::Error::other("component insert baseline checksum disappeared"))?
            .0,
    );

    let inserted_ns = inserted_elapsed.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let baseline_ns = baseline_elapsed.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let delta_ns = inserted_ns - baseline_ns;
    println!(
        "component_insert targets={INSERT_TARGETS} allocation_frames={ALLOCATION_CHECK_FRAMES} allocations={allocations} baseline_frame_us={:>9.3} insert_frame_us={:>9.3} amortized_delta_ns_per_insert={:>9.3}",
        baseline_ns / 1_000.0,
        inserted_ns / 1_000.0,
        delta_ns / INSERT_TARGETS as f64,
    );
    Ok(())
}

fn build_remove_runner() -> Result<(HeadlessRunner<BenchAction>, Arc<AtomicBool>), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_command_limit(REMOVE_TARGETS)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_component::<RemoveBenchValue>()?;
    let remove = Arc::new(AtomicBool::new(true));
    let remove_mode = Arc::clone(&remove);
    application.add_system(
        Stage::FixedUpdate,
        move |targets: Res<RemoveBenchTargets>, mut commands: Commands| {
            if remove_mode.load(Ordering::Relaxed) {
                for target in &targets.0 {
                    commands
                        .remove::<RemoveBenchValue>(*target)
                        .expect("remove benchmark command budget should fit every target");
                }
            } else {
                for target in &targets.0 {
                    commands
                        .insert(*target, RemoveBenchValue(1))
                        .expect("remove benchmark refill should fit every target");
                }
            }
        },
    );
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("component-remove-1k", move |world| {
        world.spawn(camera)?;
        let mut targets = Vec::with_capacity(REMOVE_TARGETS);
        for value in 0..REMOVE_TARGETS {
            targets.push(world.spawn(RemoveBenchValue(value as u64))?);
        }
        world.insert_resource(RemoveBenchTargets(targets))?;
        Ok(())
    })?;
    Ok((application.build_headless(world)?, remove))
}

fn advance_remove(runner: &mut HeadlessRunner<BenchAction>) -> Result<(), Box<dyn Error>> {
    let report = advance_report(runner)?;
    if report.spawned() != 0 || report.despawned() != 0 {
        return Err(io::Error::other(format!(
            "component removal changed entity counts: spawned={}, despawned={}",
            report.spawned(),
            report.despawned()
        ))
        .into());
    }
    Ok(())
}

fn verify_remove_values(
    runner: &HeadlessRunner<BenchAction>,
    expected_present: usize,
) -> Result<(), Box<dyn Error>> {
    let mut present = 0usize;
    let mut checksum = 0u64;
    for (_, value) in runner.components::<RemoveBenchValue>() {
        present += 1;
        checksum = checksum.wrapping_add(value.0);
    }
    black_box(checksum);
    if present != expected_present {
        return Err(io::Error::other(format!(
            "component removal retained {present} values, expected {expected_present}"
        ))
        .into());
    }
    Ok(())
}

fn run_remove_case() -> Result<(), Box<dyn Error>> {
    let (mut runner, remove) = build_remove_runner()?;

    for _ in 0..WARM_UP_FRAMES {
        remove.store(true, Ordering::Relaxed);
        advance_remove(&mut runner)?;
        verify_remove_values(&runner, 0)?;
        remove.store(false, Ordering::Relaxed);
        advance_remove(&mut runner)?;
        verify_remove_values(&runner, REMOVE_TARGETS)?;
    }

    let mut allocations = 0usize;
    let mut removal_elapsed = Duration::ZERO;
    for frame in 0..ALLOCATION_CHECK_FRAMES {
        remove.store(true, Ordering::Relaxed);
        begin_allocation_count();
        let started = Instant::now();
        let result = advance_remove(&mut runner);
        removal_elapsed = removal_elapsed.saturating_add(started.elapsed());
        allocations = allocations.wrapping_add(end_allocation_count());
        result?;
        verify_remove_values(&runner, 0)?;

        if frame + 1 != ALLOCATION_CHECK_FRAMES {
            remove.store(false, Ordering::Relaxed);
            advance_remove(&mut runner)?;
            verify_remove_values(&runner, REMOVE_TARGETS)?;
        }
    }

    let expected_removals = ALLOCATION_CHECK_FRAMES * REMOVE_TARGETS;
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "{expected_removals} warmed component removals allocated {allocations} times"
        ))
        .into());
    }
    let nanoseconds_per_frame = removal_elapsed.as_nanos() as f64 / ALLOCATION_CHECK_FRAMES as f64;
    println!(
        "component_remove targets={REMOVE_TARGETS} allocation_frames={ALLOCATION_CHECK_FRAMES} actual_removals={expected_removals} allocations={allocations} full_frame_us={:>9.3} amortized_full_frame_ns_per_remove={:>9.3}",
        nanoseconds_per_frame / 1_000.0,
        nanoseconds_per_frame / REMOVE_TARGETS as f64,
    );
    Ok(())
}

fn toggle_enablement_targets(
    mut state: ResMut<EnablementBenchState>,
    mut commands: Commands,
) -> Result<(), CommandEnqueueError> {
    if state.disable_next {
        for target in &state.targets {
            commands.disable(*target)?;
        }
    } else {
        for target in &state.targets {
            commands.enable(*target)?;
        }
    }
    state.disable_next = !state.disable_next;
    state.commands_completed = state
        .commands_completed
        .wrapping_add(state.targets.len() as u64);
    Ok(())
}

fn build_enablement_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_command_limit(ENABLEMENT_TARGETS)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_component::<EnablementBenchTarget>()?;
    application.add_fallible_fixed_system(toggle_enablement_targets);
    let camera = ActiveCamera2d::centered(32.0)?;
    let world = application.register_world("entity-enablement-1k", move |world| {
        world.spawn(camera)?;
        let mut targets = Vec::with_capacity(ENABLEMENT_TARGETS);
        for _ in 0..ENABLEMENT_TARGETS {
            targets.push(world.spawn(EnablementBenchTarget)?);
        }
        world.insert_resource(EnablementBenchState {
            targets,
            disable_next: true,
            commands_completed: 0,
        })?;
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn advance_enablement(runner: &mut HeadlessRunner<BenchAction>) -> Result<(), Box<dyn Error>> {
    let report = advance_report(runner)?;
    if report.spawned() != 0 || report.despawned() != 0 {
        return Err(io::Error::other(format!(
            "entity enablement changed entity counts: spawned={}, despawned={}",
            report.spawned(),
            report.despawned()
        ))
        .into());
    }
    Ok(())
}

fn run_enablement_case() -> Result<(), Box<dyn Error>> {
    let mut runner = build_enablement_runner()?;
    for _ in 0..WARM_UP_FRAMES {
        advance_enablement(&mut runner)?;
    }
    if runner.components::<Disabled>().count() != 0 {
        return Err(io::Error::other("enablement warm-up did not end enabled").into());
    }
    let commands_before = runner
        .resource::<EnablementBenchState>()
        .ok_or_else(|| io::Error::other("enablement benchmark state disappeared"))?
        .commands_completed;

    begin_allocation_count();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for frame in 0..ALLOCATION_CHECK_FRAMES {
            advance_enablement(&mut runner)?;
            let disabled = runner.components::<Disabled>().count();
            let expected_disabled = if frame % 2 == 0 {
                ENABLEMENT_TARGETS
            } else {
                0
            };
            if disabled != expected_disabled {
                return Err(io::Error::other(format!(
                    "enablement frame {frame} retained {disabled} disabled targets, expected {expected_disabled}"
                ))
                .into());
            }
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured_result?;

    let commands_delta = runner
        .resource::<EnablementBenchState>()
        .ok_or_else(|| io::Error::other("enablement benchmark state disappeared"))?
        .commands_completed
        .wrapping_sub(commands_before);
    let expected_commands = (ALLOCATION_CHECK_FRAMES * ENABLEMENT_TARGETS) as u64;
    let targets = runner.components::<EnablementBenchTarget>().count();
    let disabled = runner.components::<Disabled>().count();
    println!(
        "stable_entity_enablement frames={ALLOCATION_CHECK_FRAMES} targets={targets} toggle_commands={commands_delta} final_disabled={disabled} allocation_calls={allocations}"
    );
    if commands_delta != expected_commands || targets != ENABLEMENT_TARGETS || disabled != 0 {
        return Err(io::Error::other(format!(
            "enablement final state mismatch: commands={commands_delta}/{expected_commands}, targets={targets}/{ENABLEMENT_TARGETS}, disabled={disabled}/0"
        ))
        .into());
    }
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed entity enablement allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn run_command_suite() -> Result<(), Box<dyn Error>> {
    run_churn_case()?;
    run_despawn_only_allocation_case()?;
    run_insert_case()?;
    run_remove_case()?;
    run_enablement_case()
}

fn write_events(mut events: EventWriter<BenchEvent>) {
    for value in 0..EVENTS_PER_TICK {
        events
            .send(BenchEvent(value as u32))
            .expect("event benchmark limit should fit every record");
    }
}

fn read_events(events: EventReader<BenchEvent>, mut checksum: ResMut<EventChecksum>) {
    for event in &events {
        checksum.0 = checksum.0.wrapping_add(u64::from(event.0));
    }
}

fn build_event_runner() -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_event_limit(EVENTS_PER_TICK)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.add_system(Stage::FixedUpdate, write_events);
    application.add_system(Stage::FixedUpdate, read_events);
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("event-throughput", move |world| {
        world.spawn(camera)?;
        world.insert_resource(EventChecksum::default())?;
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_event_case() -> Result<(), Box<dyn Error>> {
    let build_start = Instant::now();
    let mut runner = build_event_runner()?;
    let build_elapsed = build_start.elapsed();

    for _ in 0..WARM_UP_FRAMES {
        black_box(advance(&mut runner)?);
    }

    let measure_start = Instant::now();
    for _ in 0..MEASURED_FRAMES {
        black_box(advance(&mut runner)?);
    }
    let measured = measure_start.elapsed();
    black_box(
        runner
            .resource::<EventChecksum>()
            .ok_or_else(|| io::Error::other("event checksum resource disappeared"))?
            .0,
    );

    let nanoseconds_per_frame = measured.as_nanos() as f64 / MEASURED_FRAMES as f64;
    let nanoseconds_per_event = nanoseconds_per_frame / EVENTS_PER_TICK as f64;
    println!(
        "typed_events events_per_tick={EVENTS_PER_TICK} build_ms={:>9.3} frame_us={:>9.3} ns_per_event={nanoseconds_per_event:>9.3}",
        build_elapsed.as_secs_f64() * 1_000.0,
        nanoseconds_per_frame / 1_000.0,
    );
    Ok(())
}

fn increment_app_counter(mut counter: AppResMut<AppCounter>) {
    counter.0 = counter.0.wrapping_add(1);
}

fn increment_world_counter(mut counter: ResMut<WorldCounter>) {
    counter.0 = counter.0.wrapping_add(1);
}

fn build_resource_access_runner(
    application_owned: bool,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    if application_owned {
        application.register_app_resource(AppCounter(0))?;
        application.add_system(Stage::FixedUpdate, increment_app_counter);
    } else {
        application.add_system(Stage::FixedUpdate, increment_world_counter);
    }
    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let world = application.register_world("resource-access", move |world| {
        world.spawn(camera)?;
        if !application_owned {
            world.insert_resource(WorldCounter(0))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(world)?)
}

fn run_resource_access_case(application_owned: bool) -> Result<(), Box<dyn Error>> {
    let mut runner = build_resource_access_runner(application_owned)?;
    for _ in 0..WARM_UP_FRAMES {
        black_box(advance(&mut runner)?);
    }

    let measure_start = Instant::now();
    for _ in 0..RESOURCE_ACCESS_FRAMES {
        black_box(advance(&mut runner)?);
    }
    let measured = measure_start.elapsed();
    let checksum = if application_owned {
        runner.app_resource::<AppCounter>().map(|counter| counter.0)
    } else {
        runner.resource::<WorldCounter>().map(|counter| counter.0)
    }
    .ok_or_else(|| io::Error::other("resource access benchmark counter disappeared"))?;
    black_box(checksum);

    let owner = if application_owned { "app" } else { "world" };
    let nanoseconds_per_frame = measured.as_nanos() as f64 / RESOURCE_ACCESS_FRAMES as f64;
    println!(
        "resource_access owner={owner:>5} frames={RESOURCE_ACCESS_FRAMES} frame_us={:>9.3}",
        nanoseconds_per_frame / 1_000.0,
    );
    Ok(())
}

fn count_circle_overlaps(
    source_geometry: Query<(&Transform2d, &CircleCollider2d), With<OverlapSource>>,
    candidates: CircleOverlapEntities<With<OverlapCandidate>>,
    mut state: ResMut<OverlapBenchState>,
) -> Result<(), QueryEntityError> {
    let mut hits = 0_u64;
    let (transform, collider) = source_geometry.get(state.source)?;
    for _ in 0..state.scans_per_frame {
        hits = hits.wrapping_add(
            candidates
                .iter_overlapping(state.source, transform, collider)?
                .count() as u64,
        );
    }
    state.checksum = state.checksum.wrapping_add(hits);
    Ok(())
}

fn build_circle_overlap_runner(
    layout: CircleOverlapLayout,
    candidate_count: usize,
    scans_per_frame: usize,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    let entity_limit = candidate_count
        .checked_add(2)
        .ok_or_else(|| io::Error::other("overlap benchmark entity count overflowed"))?;
    config.set_entity_limit(entity_limit)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(OverlapSource, OverlapCandidate)>()?;
    application.add_fallible_system(Stage::FrameUpdate, count_circle_overlaps);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let source_collider = CircleCollider2d::new(1.0)?;
    let candidate_collider = CircleCollider2d::new(0.5)?;
    let world = application.register_world("circle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((OverlapSource, Transform2d::default(), source_collider))?;
        for index in 0..candidate_count {
            let transform = Transform2d::new(layout.position(index))
                .expect("overlap benchmark positions should be finite");
            world.spawn((OverlapCandidate, transform, candidate_collider))?;
        }
        world.insert_resource(OverlapBenchState {
            source,
            scans_per_frame,
            checksum: 0,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(world)?;
    if runner.components::<OverlapCandidate>().count() != candidate_count
        || runner.components::<CircleCollider2d>().count() != candidate_count + 1
    {
        return Err(io::Error::other("circle-overlap fixture lost candidate geometry").into());
    }
    Ok(runner)
}

fn count_rectangle_overlaps(
    source_geometry: Query<(&Transform2d, &RectangleCollider2d), With<OverlapSource>>,
    candidates: RectangleOverlapEntities<With<OverlapCandidate>>,
    mut state: ResMut<OverlapBenchState>,
) -> Result<(), QueryEntityError> {
    let mut hits = 0_u64;
    let (transform, collider) = source_geometry.get(state.source)?;
    for _ in 0..state.scans_per_frame {
        hits = hits.wrapping_add(
            candidates
                .iter_overlapping(state.source, transform, collider)?
                .count() as u64,
        );
    }
    state.checksum = state.checksum.wrapping_add(hits);
    Ok(())
}

fn build_rectangle_overlap_runner(
    layout: RectangleOverlapLayout,
    candidate_count: usize,
    scans_per_frame: usize,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    let entity_limit = candidate_count
        .checked_add(2)
        .ok_or_else(|| io::Error::other("rectangle-overlap entity count overflowed"))?;
    config.set_entity_limit(entity_limit)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(OverlapSource, OverlapCandidate)>()?;
    application.add_fallible_system(Stage::FrameUpdate, count_rectangle_overlaps);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let source_collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let candidate_collider = RectangleCollider2d::new(Vec2::ONE)?;
    let world = application.register_world("rectangle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((OverlapSource, Transform2d::default(), source_collider))?;
        for index in 0..candidate_count {
            let transform = Transform2d::new(layout.position(index))
                .expect("rectangle-overlap benchmark positions should be finite");
            world.spawn((OverlapCandidate, transform, candidate_collider))?;
        }
        world.insert_resource(OverlapBenchState {
            source,
            scans_per_frame,
            checksum: 0,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(world)?;
    if runner.components::<OverlapCandidate>().count() != candidate_count
        || runner.components::<RectangleCollider2d>().count() != candidate_count + 1
    {
        return Err(io::Error::other("rectangle-overlap fixture lost candidate geometry").into());
    }
    Ok(runner)
}

fn count_circle_against_rectangles(
    source_geometry: Query<(&Transform2d, &CircleCollider2d), With<OverlapSource>>,
    candidates: RectangleOverlapEntities<With<OverlapCandidate>>,
    mut state: ResMut<OverlapBenchState>,
) -> Result<(), QueryEntityError> {
    let mut hits = 0_u64;
    let (transform, collider) = source_geometry.get(state.source)?;
    for _ in 0..state.scans_per_frame {
        hits = hits.wrapping_add(
            candidates
                .iter_overlapping_with_circle(state.source, transform, collider)?
                .count() as u64,
        );
    }
    state.checksum = state.checksum.wrapping_add(hits);
    Ok(())
}

fn build_circle_rectangle_overlap_runner(
    layout: MixedOverlapLayout,
    candidate_count: usize,
    scans_per_frame: usize,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    let entity_limit = candidate_count
        .checked_add(2)
        .ok_or_else(|| io::Error::other("mixed-overlap entity count overflowed"))?;
    config.set_entity_limit(entity_limit)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(OverlapSource, OverlapCandidate)>()?;
    application.add_fallible_frame_system(count_circle_against_rectangles);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let source_collider = CircleCollider2d::new(1.25)?;
    let candidate_collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let world = application.register_world("circle-rectangle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((OverlapSource, source_collider))?;
        for index in 0..candidate_count {
            let transform = Transform2d::new(layout.position(index))
                .expect("mixed-overlap benchmark positions should be finite");
            world.spawn((OverlapCandidate, transform, candidate_collider))?;
        }
        world.insert_resource(OverlapBenchState {
            source,
            scans_per_frame,
            checksum: 0,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(world)?;
    if runner.components::<OverlapCandidate>().count() != candidate_count
        || runner.components::<CircleCollider2d>().count() != 1
        || runner.components::<RectangleCollider2d>().count() != candidate_count
    {
        return Err(io::Error::other("mixed-overlap fixture lost candidate geometry").into());
    }
    Ok(runner)
}

fn count_rectangle_against_circles(
    source_geometry: Query<(&Transform2d, &RectangleCollider2d), With<OverlapSource>>,
    candidates: CircleOverlapEntities<With<OverlapCandidate>>,
    mut state: ResMut<OverlapBenchState>,
) -> Result<(), QueryEntityError> {
    let mut hits = 0_u64;
    let (transform, collider) = source_geometry.get(state.source)?;
    for _ in 0..state.scans_per_frame {
        hits = hits.wrapping_add(
            candidates
                .iter_overlapping_with_rectangle(state.source, transform, collider)?
                .count() as u64,
        );
    }
    state.checksum = state.checksum.wrapping_add(hits);
    Ok(())
}

fn build_rectangle_circle_overlap_runner(
    layout: MixedOverlapLayout,
    candidate_count: usize,
    scans_per_frame: usize,
) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>> {
    let mut config = AppConfig::default();
    let entity_limit = candidate_count
        .checked_add(2)
        .ok_or_else(|| io::Error::other("reciprocal mixed-overlap entity count overflowed"))?;
    config.set_entity_limit(entity_limit)?;
    let mut application = Application::<BenchAction>::new(config)?;
    application.approve_components::<(OverlapSource, OverlapCandidate)>()?;
    application.add_fallible_frame_system(count_rectangle_against_circles);

    let camera = ActiveCamera2d::new(Camera2d::new(Vec2::ZERO, 32.0)?);
    let source_collider = RectangleCollider2d::new(Vec2::splat(2.0))?;
    let candidate_collider = CircleCollider2d::new(1.25)?;
    let world = application.register_world("rectangle-circle-overlap", move |world| {
        world.spawn(camera)?;
        let source = world.spawn((OverlapSource, source_collider))?;
        for index in 0..candidate_count {
            let transform = Transform2d::new(layout.position(index))
                .expect("reciprocal mixed-overlap positions should be finite");
            world.spawn((OverlapCandidate, transform, candidate_collider))?;
        }
        world.insert_resource(OverlapBenchState {
            source,
            scans_per_frame,
            checksum: 0,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(world)?;
    if runner.components::<OverlapCandidate>().count() != candidate_count
        || runner.components::<RectangleCollider2d>().count() != 1
        || runner.components::<CircleCollider2d>().count() != candidate_count
    {
        return Err(io::Error::other("reciprocal mixed-overlap fixture lost geometry").into());
    }
    Ok(runner)
}

fn advance_frame_only(runner: &mut HeadlessRunner<BenchAction>) -> Result<(), Box<dyn Error>> {
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;
    let outcome = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport));
    let FrameOutcome::Advanced(report) = outcome else {
        return Err(io::Error::other("overlap benchmark frame was rejected").into());
    };
    if let Some(failure) = report.failure() {
        return Err(io::Error::other(format!("overlap benchmark frame failed: {failure}")).into());
    }
    if report.fixed_ticks_attempted() != 0 {
        return Err(io::Error::other("zero-time overlap frame ran a fixed tick").into());
    }
    black_box(report);
    Ok(())
}

fn measure_frame_only(
    runner: &mut HeadlessRunner<BenchAction>,
) -> Result<Duration, Box<dyn Error>> {
    let start = Instant::now();
    advance_frame_only(runner)?;
    Ok(start.elapsed())
}

fn run_overlap_case<Build>(
    labels: (&str, &str),
    hits_per_scan: usize,
    candidate_count: usize,
    scans_per_frame: usize,
    warm_up_frames: usize,
    measured_frames: usize,
    build_runner: Build,
) -> Result<(), Box<dyn Error>>
where
    Build: Fn(usize, usize) -> Result<HeadlessRunner<BenchAction>, Box<dyn Error>>,
{
    let (shape, layout) = labels;
    let build_start = Instant::now();
    let mut baseline = build_runner(candidate_count, 0)?;
    let mut measured_runner = build_runner(candidate_count, scans_per_frame)?;
    let build_elapsed = build_start.elapsed();

    for _ in 0..warm_up_frames {
        advance_frame_only(&mut baseline)?;
        advance_frame_only(&mut measured_runner)?;
    }

    let mut baseline_elapsed = Duration::ZERO;
    let mut measured_elapsed = Duration::ZERO;
    for frame in 0..measured_frames {
        if frame % 2 == 0 {
            baseline_elapsed += measure_frame_only(&mut baseline)?;
            measured_elapsed += measure_frame_only(&mut measured_runner)?;
        } else {
            measured_elapsed += measure_frame_only(&mut measured_runner)?;
            baseline_elapsed += measure_frame_only(&mut baseline)?;
        }
    }
    let checksum = measured_runner
        .resource::<OverlapBenchState>()
        .ok_or_else(|| io::Error::other("overlap benchmark state disappeared"))?
        .checksum;
    let expected_checksum = (warm_up_frames + measured_frames)
        .checked_mul(scans_per_frame)
        .and_then(|value| value.checked_mul(hits_per_scan))
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(|| io::Error::other("overlap benchmark checksum overflowed"))?;
    if checksum != expected_checksum {
        return Err(io::Error::other(format!(
            "overlap checksum {checksum} did not match {expected_checksum}"
        ))
        .into());
    }
    black_box(checksum);

    let checks_per_frame = scans_per_frame * candidate_count;
    let baseline_nanoseconds = baseline_elapsed.as_nanos() as f64 / measured_frames as f64;
    let measured_nanoseconds = measured_elapsed.as_nanos() as f64 / measured_frames as f64;
    let approximate_nanoseconds_per_visited_candidate =
        (measured_nanoseconds - baseline_nanoseconds) / checks_per_frame as f64;
    println!(
        "{shape}_overlap={layout:>18} queries={scans_per_frame:>3} candidates={candidate_count:>5} checks_per_frame={checks_per_frame:>7} build_two_ms={:>9.3} baseline_us={:>9.3} measured_us={:>9.3} approx_ns_per_visited_candidate={approximate_nanoseconds_per_visited_candidate:>9.3} checksum={checksum}",
        build_elapsed.as_secs_f64() * 1_000.0,
        baseline_nanoseconds / 1_000.0,
        measured_nanoseconds / 1_000.0,
    );
    Ok(())
}

fn run_circle_overlap_suite() -> Result<(), Box<dyn Error>> {
    for candidate_count in [100, 1_000, 10_000] {
        let layout = CircleOverlapLayout::SparseX;
        run_overlap_case(
            ("circle", layout.label()),
            layout.hits_per_scan(candidate_count),
            candidate_count,
            1,
            WARM_UP_FRAMES,
            OVERLAP_TARGET_CHECKS / candidate_count,
            |count, scans| build_circle_overlap_runner(layout, count, scans),
        )?;
    }
    println!("the next circle cases expose repeated linear O(queries * candidates) work");
    for layout in [
        CircleOverlapLayout::Dense,
        CircleOverlapLayout::BoundaryTangent,
        CircleOverlapLayout::SparseX,
        CircleOverlapLayout::SparseY,
        CircleOverlapLayout::DiagonalNearMiss,
        CircleOverlapLayout::Mixed,
    ] {
        run_overlap_case(
            ("circle", layout.label()),
            layout.hits_per_scan(10_000),
            10_000,
            100,
            2,
            10,
            |count, scans| build_circle_overlap_runner(layout, count, scans),
        )?;
    }
    Ok(())
}

fn run_rectangle_overlap_allocation_case() -> Result<(), Box<dyn Error>> {
    let candidate_count = 10_000;
    let mut runner =
        build_rectangle_overlap_runner(RectangleOverlapLayout::Dense, candidate_count, 1)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_frame_only(&mut runner)?;
    }
    let checksum_before = runner
        .resource::<OverlapBenchState>()
        .ok_or_else(|| io::Error::other("rectangle-overlap benchmark state disappeared"))?
        .checksum;

    begin_allocation_count();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            advance_frame_only(&mut runner)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured_result?;

    let checksum_delta = runner
        .resource::<OverlapBenchState>()
        .ok_or_else(|| io::Error::other("rectangle-overlap benchmark state disappeared"))?
        .checksum
        .wrapping_sub(checksum_before);
    let expected = (ALLOCATION_CHECK_FRAMES * candidate_count) as u64;
    println!(
        "stable_rectangle_overlap frames={ALLOCATION_CHECK_FRAMES} candidates={candidate_count} allocation_calls={allocations} checksum_delta={checksum_delta}"
    );
    if checksum_delta != expected {
        return Err(io::Error::other(format!(
            "rectangle-overlap checksum {checksum_delta} did not match {expected}"
        ))
        .into());
    }
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed rectangle-overlap frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn run_rectangle_overlap_suite() -> Result<(), Box<dyn Error>> {
    for candidate_count in [100, 1_000, 10_000] {
        let layout = RectangleOverlapLayout::SparseX;
        run_overlap_case(
            ("rectangle", layout.label()),
            layout.hits_per_scan(candidate_count),
            candidate_count,
            1,
            WARM_UP_FRAMES,
            OVERLAP_TARGET_CHECKS / candidate_count,
            |count, scans| build_rectangle_overlap_runner(layout, count, scans),
        )?;
    }
    println!("the next rectangle cases expose repeated linear O(queries * candidates) work");
    for layout in [
        RectangleOverlapLayout::Dense,
        RectangleOverlapLayout::EdgeTangent,
        RectangleOverlapLayout::CornerTangent,
        RectangleOverlapLayout::SparseX,
        RectangleOverlapLayout::SparseY,
        RectangleOverlapLayout::CornerNearMiss,
        RectangleOverlapLayout::Mixed,
    ] {
        run_overlap_case(
            ("rectangle", layout.label()),
            layout.hits_per_scan(10_000),
            10_000,
            100,
            2,
            10,
            |count, scans| build_rectangle_overlap_runner(layout, count, scans),
        )?;
    }
    run_rectangle_overlap_allocation_case()
}

fn run_mixed_overlap_allocation_case() -> Result<(), Box<dyn Error>> {
    let candidate_count = 10_000;
    let mut runner =
        build_circle_rectangle_overlap_runner(MixedOverlapLayout::Contained, candidate_count, 1)?;
    for _ in 0..WARM_UP_FRAMES {
        advance_frame_only(&mut runner)?;
    }
    let checksum_before = runner
        .resource::<OverlapBenchState>()
        .ok_or_else(|| io::Error::other("mixed-overlap benchmark state disappeared"))?
        .checksum;

    begin_allocation_count();
    let measured_result = (|| -> Result<(), Box<dyn Error>> {
        for _ in 0..ALLOCATION_CHECK_FRAMES {
            advance_frame_only(&mut runner)?;
        }
        Ok(())
    })();
    let allocations = end_allocation_count();
    measured_result?;

    let checksum_delta = runner
        .resource::<OverlapBenchState>()
        .ok_or_else(|| io::Error::other("mixed-overlap benchmark state disappeared"))?
        .checksum
        .wrapping_sub(checksum_before);
    let expected = (ALLOCATION_CHECK_FRAMES * candidate_count) as u64;
    println!(
        "stable_mixed_overlap frames={ALLOCATION_CHECK_FRAMES} candidates={candidate_count} allocation_calls={allocations} checksum_delta={checksum_delta}"
    );
    if checksum_delta != expected {
        return Err(io::Error::other(format!(
            "mixed-overlap checksum {checksum_delta} did not match {expected}"
        ))
        .into());
    }
    if runner.components::<OverlapCandidate>().count() != candidate_count
        || runner.components::<CircleCollider2d>().count() != 1
        || runner.components::<RectangleCollider2d>().count() != candidate_count
    {
        return Err(io::Error::other("mixed-overlap allocation fixture changed shape").into());
    }
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed mixed-overlap frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn run_mixed_overlap_suite() -> Result<(), Box<dyn Error>> {
    for candidate_count in [100, 1_000, 10_000] {
        let layout = MixedOverlapLayout::SparseX;
        run_overlap_case(
            ("circle-rectangle", layout.label()),
            layout.hits_per_scan(candidate_count),
            candidate_count,
            1,
            WARM_UP_FRAMES,
            OVERLAP_TARGET_CHECKS / candidate_count,
            |count, scans| build_circle_rectangle_overlap_runner(layout, count, scans),
        )?;
    }
    println!("the next mixed cases expose repeated linear O(queries * candidates) work");
    for layout in [
        MixedOverlapLayout::Contained,
        MixedOverlapLayout::FaceTangent,
        MixedOverlapLayout::CornerTangent,
        MixedOverlapLayout::SparseX,
        MixedOverlapLayout::SparseY,
        MixedOverlapLayout::CornerNearMiss,
        MixedOverlapLayout::Mixed,
    ] {
        run_overlap_case(
            ("circle-rectangle", layout.label()),
            layout.hits_per_scan(10_000),
            10_000,
            100,
            2,
            10,
            |count, scans| build_circle_rectangle_overlap_runner(layout, count, scans),
        )?;
    }
    let reciprocal = MixedOverlapLayout::Mixed;
    run_overlap_case(
        ("rectangle-circle", reciprocal.label()),
        reciprocal.hits_per_scan(10_000),
        10_000,
        100,
        2,
        10,
        |count, scans| build_rectangle_circle_overlap_runner(reciprocal, count, scans),
    )?;
    run_mixed_overlap_allocation_case()
}

fn run_overlap_suite() -> Result<(), Box<dyn Error>> {
    run_circle_overlap_suite()?;
    run_rectangle_overlap_suite()?;
    run_mixed_overlap_suite()
}

fn run_exit_allocation_case() -> Result<(), Box<dyn Error>> {
    let mut application = Application::<BenchAction>::new(AppConfig::default())?;
    let camera = ActiveCamera2d::centered(32.0)?;
    let world = application.register_world("exit-allocation", move |world| {
        world.spawn(camera)?;
        Ok(())
    })?;
    application.add_fallible_frame_system(
        |mut commands: Commands| -> Result<(), CommandEnqueueError> { commands.request_exit() },
    );
    let mut runner = application.build_headless(world)?;
    let viewport = LogicalViewport::new(1_280.0, 720.0)?;

    for _ in 0..WARM_UP_FRAMES {
        let warm = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport));
        let FrameOutcome::Advanced(warm) = warm else {
            return Err(io::Error::other("exit allocation warm-up was rejected").into());
        };
        if !warm.exit_requested() || warm.failure().is_some() {
            return Err(
                io::Error::other("exit allocation warm-up did not request clean exit").into(),
            );
        }
    }

    begin_allocation_count();
    let measured = runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport));
    let allocations = end_allocation_count();
    let FrameOutcome::Advanced(measured) = measured else {
        return Err(io::Error::other("measured exit frame was rejected").into());
    };
    if !measured.exit_requested()
        || measured.failure().is_some()
        || measured.extracted_generation().is_some()
    {
        return Err(io::Error::other("measured exit frame violated its terminal contract").into());
    }
    println!("warmed_exit_command allocation_calls={allocations}");
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed payload-free exit command unexpectedly allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    // Cargo includes custom benchmark binaries in `test --all-targets`. Only
    // perform timed work when Cargo's bench command supplies this flag.
    if !std::env::args().any(|argument| argument == "--bench") {
        println!("headless runtime benchmark skipped outside `cargo bench`");
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--circle-overlap-only") {
        println!("Sim;Logic circle-overlap benchmark");
        return run_circle_overlap_suite();
    }
    if std::env::args().any(|argument| argument == "--rectangle-overlap-only") {
        println!("Sim;Logic rectangle-overlap benchmark");
        return run_rectangle_overlap_suite();
    }
    if std::env::args().any(|argument| argument == "--mixed-overlap-only") {
        println!("Sim;Logic mixed circle-rectangle overlap benchmark");
        return run_mixed_overlap_suite();
    }
    if std::env::args().any(|argument| argument == "--overlap-only") {
        println!("Sim;Logic same-shape + mixed overlap benchmark");
        return run_overlap_suite();
    }
    if std::env::args().any(|argument| argument == "--commands-only") {
        println!("Sim;Logic command-storage benchmark");
        return run_command_suite();
    }
    if std::env::args().any(|argument| argument == "--insert-only") {
        println!("Sim;Logic deferred component-insert benchmark");
        return run_insert_case();
    }
    if std::env::args().any(|argument| argument == "--remove-only") {
        println!("Sim;Logic deferred component-remove benchmark");
        return run_remove_case();
    }
    if std::env::args().any(|argument| argument == "--enablement-only") {
        println!("Sim;Logic entity-enablement allocation gate");
        return run_enablement_case();
    }
    if std::env::args().any(|argument| argument == "--exit-only") {
        println!("Sim;Logic application-exit allocation gate");
        return run_exit_allocation_case();
    }
    if std::env::args().any(|argument| argument == "--pause-only") {
        println!("Sim;Logic fixed-pause allocation gate");
        return pause::run_allocation_case();
    }
    if std::env::args().any(|argument| argument == "--screen-only") {
        println!("Sim;Logic mixed world/screen allocation gate");
        return screen::run_allocation_case();
    }
    if std::env::args().any(|argument| argument == "--world-build-only") {
        println!("Sim;Logic managed World-build benchmark");
        run_managed_idle_case()?;
        return run_spatial_build_case();
    }
    if std::env::args().any(|argument| argument == "--empty-fixed-only") {
        println!("Sim;Logic empty FixedUpdate benchmark");
        return run_empty_fixed_suite();
    }
    if std::env::args().any(|argument| argument == "--motion-only") {
        println!("Sim;Logic linear-motion allocation gate");
        return run_linear_motion_case();
    }
    if std::env::args().any(|argument| argument == "--acceleration-motion-only") {
        println!("Sim;Logic acceleration-motion allocation gate");
        return run_acceleration_motion_case();
    }
    if std::env::args().any(|argument| argument == "--digital-motion-only") {
        println!("Sim;Logic digital-motion allocation gate");
        return run_digital_motion_case();
    }
    if std::env::args().any(|argument| argument == "--input-only") {
        println!("Sim;Logic input-snapshot allocation gates");
        return run_input_allocation_suite();
    }
    if std::env::args().any(|argument| argument == "--visual-only") {
        println!("Sim;Logic visual-extraction benchmark");
        return run_visual_suite();
    }

    println!(
        "Sim;Logic headless fixed-update + full-extraction benchmark ({MEASURED_FRAMES} measured frames)"
    );
    for circle_count in [0, 100, 1_000, 10_000] {
        run_case(circle_count)?;
    }
    run_visual_suite()?;
    run_fixed_frame_allocation_case("one-empty-tick", 1)?;
    run_fixed_frame_allocation_case("four-empty-ticks", 4)?;
    run_empty_fixed_suite()?;
    run_rectangle_collision_allocation_case()?;
    run_linear_motion_case()?;
    run_acceleration_motion_case()?;
    run_digital_motion_case()?;
    run_input_allocation_suite()?;
    run_managed_idle_case()?;
    run_spatial_build_case()?;
    run_command_suite()?;
    run_exit_allocation_case()?;
    pause::run_allocation_case()?;
    screen::run_allocation_case()?;
    run_event_case()?;
    run_resource_access_case(false)?;
    run_resource_access_case(true)?;
    run_overlap_suite()
}

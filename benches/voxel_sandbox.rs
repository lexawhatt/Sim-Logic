//! Warmed headless FrameUpdate and CPU extraction, without a renderer or GPU.
//!
//! Run: cargo bench --no-default-features --features text --bench voxel_sandbox
//! Allocation counting and elapsed-time measurements use separate 100-frame
//! passes. Zero supplied elapsed time excludes fixed simulation and catch-up.
//! These measurements do not establish desktop rendering cost or frame rate.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use sim_logic::prelude::*;

#[path = "../examples/voxel_sandbox/app.rs"]
mod app;
#[path = "../examples/voxel_sandbox/materials.rs"]
#[allow(dead_code, unused_imports)]
mod materials;
#[path = "../examples/voxel_sandbox/showcase.rs"]
#[allow(dead_code, unused_imports)]
mod showcase;
// Cargo enables cfg(test) for harness-free benches. Model unit-test helpers
// are exercised by tests/voxel_sandbox.rs, not by this benchmark binary.
#[allow(dead_code, unused_imports)]
#[path = "../examples/voxel_sandbox/model/mod.rs"]
mod model;
#[path = "../examples/voxel_sandbox/presentation.rs"]
mod presentation;
#[path = "../examples/voxel_sandbox/projection.rs"]
mod projection;
#[path = "../examples/voxel_sandbox/scene.rs"]
mod scene;
#[path = "../examples/voxel_sandbox/view.rs"]
mod view;

const WARMUP_FRAMES: usize = 20;
const MEASURED_FRAMES: usize = 100;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

// SAFETY: Every allocation operation delegates unchanged to System. The
// atomic counters allocate nothing and do not change pointer ownership.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(pointer, layout, new_size) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
}

fn count_allocation() {
    if COUNT_ALLOCATIONS.load(Ordering::Relaxed) {
        ALLOCATION_CALLS.fetch_add(1, Ordering::Relaxed);
    }
}

fn allocation_calls(work: impl FnOnce() -> LogicResult) -> LogicResult<usize> {
    ALLOCATION_CALLS.store(0, Ordering::SeqCst);
    COUNT_ALLOCATIONS.store(true, Ordering::SeqCst);
    let result = work();
    COUNT_ALLOCATIONS.store(false, Ordering::SeqCst);
    let calls = ALLOCATION_CALLS.load(Ordering::SeqCst);
    result?;
    Ok(calls)
}

fn advance(
    runner: &mut HeadlessRunner<app::Action>,
    viewport: LogicalViewport,
    events: &[InputEvent],
) -> LogicResult<LogicFrameReport> {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(Duration::ZERO, events, viewport))
    else {
        return Err("voxel benchmark frame rejected".into());
    };
    if report.failure().is_some() {
        return Err(format!("voxel benchmark frame failed: {:?}", report.failure()).into());
    }
    assert_eq!(report.fixed_ticks_attempted(), 0);
    black_box(runner.extracted_frame().ok_or("voxel snapshot missing")?);
    Ok(report)
}

fn idle_frames(
    runner: &mut HeadlessRunner<app::Action>,
    viewport: LogicalViewport,
    frames: usize,
) -> LogicResult {
    for _ in 0..frames {
        advance(runner, viewport, &[])?;
    }
    Ok(())
}

struct Baseline {
    generation: WorldGeneration,
    assets: Vec<(LogicEntity, MeshAsset3d)>,
    rebuilt: u64,
    player: model::Player,
    inventory: model::Inventory,
}

fn baseline(runner: &HeadlessRunner<app::Action>) -> LogicResult<Baseline> {
    let local = runner
        .resource::<scene::Local>()
        .ok_or("region state missing")?;
    let session = runner
        .app_resource::<app::Session>()
        .ok_or("session missing")?;
    assert_eq!(local.phase, scene::Phase::Ready);
    assert!(runner.resource::<View3d>().ok_or("view missing")?.enabled());
    let snapshot = runner.extracted_frame().ok_or("voxel snapshot missing")?;
    assert!(!snapshot.resolved_meshes().is_empty());
    Ok(Baseline {
        generation: runner.world_generation(),
        assets: snapshot
            .resolved_meshes()
            .iter()
            .map(|mesh| (mesh.source(), mesh.visual().asset().clone()))
            .collect(),
        rebuilt: local.rebuilt_chunks,
        player: session.game.player,
        inventory: session.game.inventory.clone(),
    })
}

fn verify_idle(runner: &HeadlessRunner<app::Action>, initial: &Baseline) -> LogicResult {
    let local = runner
        .resource::<scene::Local>()
        .ok_or("region state missing")?;
    let session = runner
        .app_resource::<app::Session>()
        .ok_or("session missing")?;
    let snapshot = runner.extracted_frame().ok_or("voxel snapshot missing")?;
    assert_eq!(local.phase, scene::Phase::Ready);
    assert_eq!(runner.world_generation(), initial.generation);
    assert_eq!(
        local.rebuilt_chunks, initial.rebuilt,
        "idle rebuilt a chunk"
    );
    assert_eq!(session.game.player, initial.player);
    assert_eq!(session.game.inventory, initial.inventory);
    assert_eq!((session.edits, session.ticks), (0, 0));
    assert_eq!(snapshot.resolved_meshes().len(), initial.assets.len());
    for (mesh, (entity, asset)) in snapshot.resolved_meshes().iter().zip(&initial.assets) {
        assert_eq!(mesh.source(), *entity);
        assert!(
            mesh.visual().asset().shares_storage(asset),
            "idle replaced mesh source storage"
        );
        assert!(
            runner
                .component::<MeshVisual3d>(*entity)?
                .asset()
                .shares_storage(asset)
        );
    }
    Ok(())
}

fn measure_region(
    runner: &mut HeadlessRunner<app::Action>,
    viewport: LogicalViewport,
) -> LogicResult {
    // This also completes staged loading and warms both extraction buffers.
    idle_frames(runner, viewport, WARMUP_FRAMES)?;
    let initial = baseline(runner)?;
    let calls = allocation_calls(|| idle_frames(runner, viewport, MEASURED_FRAMES))?;
    verify_idle(runner, &initial)?;

    let started = Instant::now();
    idle_frames(runner, viewport, MEASURED_FRAMES)?;
    let elapsed = started.elapsed();
    verify_idle(runner, &initial)?;

    let snapshot = runner.extracted_frame().ok_or("voxel snapshot missing")?;
    let mut vertices = 0;
    let mut triangles = 0;
    let mut source_bytes = 0;
    for mesh in snapshot.resolved_meshes() {
        let asset = mesh.visual().asset();
        vertices += asset.mesh().vertices().len();
        triangles += asset.mesh().triangle_count();
        // Engine reports retained CPU topology capacity here; this excludes
        // fonts, ECS state, snapshot metadata and all desktop/GPU resources.
        source_bytes += asset.source_bytes();
    }
    let session = runner
        .app_resource::<app::Session>()
        .ok_or("session missing")?;
    println!(
        "voxel_headless_cpu region={} frames_per_pass={MEASURED_FRAMES} allocation_calls={calls} mean_frame_us={:.2} meshes={} source_vertices={vertices} source_triangles={triangles} source_bytes={source_bytes} solid_blocks={} labels={} idle_chunk_rebuilds=0 shared_mesh_assets=true",
        session.game.active.name(),
        elapsed.as_secs_f64() * 1_000_000.0 / MEASURED_FRAMES as f64,
        snapshot.resolved_meshes().len(),
        session.game.active_region().solid_count(),
        snapshot.resolved_screen_texts().len(),
    );
    Ok(())
}

fn main() -> LogicResult {
    if !std::env::args().any(|argument| argument == "--bench") {
        println!("voxel benchmark skipped outside `cargo bench`");
        return Ok(());
    }
    println!(
        "voxel_headless_cpu warmup_frames={WARMUP_FRAMES} allocation_and_timing_passes=separate elapsed_input=zero renderer=none gpu=none"
    );
    // Saving is never requested; constructing this path performs no file I/O.
    let (application, initial) =
        app::build_application(TimeConfig::default(), "voxel-benchmark-unused.save".into())?;
    let mut runner = application.build_headless(initial)?;
    let viewport = LogicalViewport::new(1100.0, 720.0)?;
    measure_region(&mut runner, viewport)?;

    // Exercise the other region through the actual application route. Loading
    // and replacement are outside both measured passes, as is initial meshing.
    let events = [
        InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Pressed),
        InputEvent::key(PhysicalKeyCode::KeyN, ButtonState::Released),
    ];
    let traveled = advance(&mut runner, viewport, &events)?;
    assert!(matches!(
        traveled.transition(),
        FrameTransition::Committed { .. }
    ));
    measure_region(&mut runner, viewport)
}

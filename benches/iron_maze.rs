use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    io,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Instant,
};

use sim_logic::prelude::*;

#[path = "../examples/iron_maze/mod.rs"]
mod game_example;

use game_example::{FIXED_STEP, GameAction, game::GameState, render::MAX_QUADS};

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
}

fn advance(
    runner: &mut HeadlessRunner<GameAction>,
    viewport: LogicalViewport,
    events: &[InputEvent],
) -> LogicResult {
    let FrameOutcome::Advanced(report) =
        runner.advance_frame(FrameRequest::new(FIXED_STEP * 2, events, viewport))
    else {
        return Err(io::Error::other("Iron Maze benchmark frame rejected").into());
    };
    if report.failure().is_some()
        || report.fixed_ticks_attempted() != 2
        || report.exit_requested()
        || report.spawned() != 0
        || report.despawned() != 0
        || !matches!(report.transition(), FrameTransition::None)
        || report.extracted_generation() != Some(runner.world_generation())
    {
        return Err(io::Error::other("Iron Maze benchmark frame violated its contract").into());
    }
    let frame = runner
        .extracted_frame()
        .ok_or_else(|| io::Error::other("no extracted game view"))?;
    if frame.resolved_screen_rectangles().len() != MAX_QUADS {
        return Err(io::Error::other("Iron Maze changed the fixed screen pool size").into());
    }
    black_box(frame);
    Ok(())
}

fn main() -> LogicResult {
    let started = Instant::now();
    let (application, world) = game_example::build_application()?;
    let mut runner = application.build_headless(world)?;
    let build_ms = started.elapsed().as_secs_f64() * 1000.0;
    let viewport = LogicalViewport::new(1280.0, 720.0)?;
    let events = [
        InputEvent::key(PhysicalKeyCode::ArrowRight, ButtonState::Pressed),
        InputEvent::mouse_button(MouseButton::Left, ButtonState::Pressed),
    ];
    advance(&mut runner, viewport, &events)?;
    for _ in 0..31 {
        advance(&mut runner, viewport, &[])?;
    }
    let before = runner.resource::<GameState>().ok_or("game state")?.clone();
    const FRAMES: u64 = 120;
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let started = Instant::now();
    let measured = (|| -> LogicResult {
        for _ in 0..FRAMES {
            advance(&mut runner, viewport, &[])?;
        }
        Ok(())
    })();
    let elapsed = started.elapsed();
    COUNTING.store(false, Ordering::Relaxed);
    measured?;
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let after = runner.resource::<GameState>().ok_or("game state")?;
    if after.ticks - before.ticks != FRAMES * 2
        || after.player.ammo >= before.player.ammo
        || after.player.angle == before.player.angle
        || after.phase != game_example::game::Phase::Playing
    {
        return Err(io::Error::other(
            "Iron Maze benchmark did not simulate the intended live controls",
        )
        .into());
    }
    println!(
        "iron_maze_cpu frames={FRAMES} fixed_ticks={} screen_pool={MAX_QUADS} build_ms={build_ms:.3} mean_frame_us={:.3} allocation_calls={allocations}",
        FRAMES * 2,
        elapsed.as_secs_f64() * 1_000_000.0 / FRAMES as f64
    );
    if allocations != 0 {
        return Err(io::Error::other(format!(
            "warmed Iron Maze CPU frames allocated {allocations} times"
        ))
        .into());
    }
    Ok(())
}

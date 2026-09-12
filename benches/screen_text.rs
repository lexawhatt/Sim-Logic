//! Warmed CPU runtime/extraction only; no renderer, rasterization, or GPU timing.
//!
//! Run: cargo bench --no-default-features --features text --bench screen_text
//! Equal initial strings share prepared storage. Changed strings alternate two
//! static 32-byte lines: there is no caller formatting allocation to hide inside
//! the result. Allocation counting and elapsed-time passes run separately.
//! Append `-- --shaping-probe` to isolate Engine shaping versus Logic setters
//! without timing or ECS work.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

use sim_logic::prelude::*;

const FONT: &[u8] = include_bytes!("../tests/assets/text/DejaVuSans.ttf");
const LINES: [&str; 2] = [
    "0123456789ABCDEF0123456789ABCDEF",
    "FEDCBA9876543210FEDCBA9876543210",
];
// Odd warmup plus even measured passes leaves changed labels on LINES[1],
// so result validation catches an accidentally skipped update system.
const WARMUP_FRAMES: usize = 21;
const MEASURED_FRAMES: usize = 120;

struct CountingAllocator;

static COUNT_ALLOCATIONS: AtomicBool = AtomicBool::new(false);
static ALLOCATION_CALLS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static GLOBAL_ALLOCATOR: CountingAllocator = CountingAllocator;

// SAFETY: Every operation delegates unchanged to System. Atomic counters do
// not allocate, and counting never alters pointer layout or allocation lifetime.
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

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Unchanged,
    MovedTinted,
    ChangedText,
}

#[derive(Resource)]
struct Updates {
    mode: Mode,
    second: bool,
}

fn update_labels(
    mut updates: ResMut<Updates>,
    mut labels: Query<&mut ScreenTextVisual>,
) -> Result<(), TextError> {
    updates.second = !updates.second;
    let phase = usize::from(updates.second);
    for (index, mut label) in labels.iter_mut().enumerate() {
        match updates.mode {
            Mode::Unchanged => {}
            Mode::MovedTinted => {
                label.set_position(position(index, phase))?;
                label.set_tint(Color::rgba(
                    0.5,
                    0.75,
                    1.0,
                    if phase == 0 { 0.5 } else { 1.0 },
                ))?;
            }
            Mode::ChangedText => label.set_text(LINES[phase])?,
        }
    }
    Ok(())
}

fn position(index: usize, phase: usize) -> LogicalScreenPosition {
    LogicalScreenPosition::new(
        (index % 20) as f32 * 48.0 + phase as f32,
        (index / 20) as f32 * 20.0 + 30.0,
    )
}

fn build(count: usize, mode: Mode) -> LogicResult<HeadlessRunner<Action>> {
    let mut config = AppConfig::default();
    config.set_text_limits(TextLimits::new(1, 8 * 1024 * 1024));
    config.set_render_limits(
        RenderLimits::default()
            .with_max_screen_texts(count)
            .with_max_screen_text_bytes(count * 32)
            .with_max_screen_text_glyphs(count * 32),
    );
    let mut app = Application::<Action>::new(config)?;
    let font = app.register_font(FONT.to_vec(), TextSettings::new(18.0)?)?;
    let prepared = ScreenTextVisual::new(font, LINES[0], position(0, 0))?;
    assert_eq!(prepared.glyph_count(), 32);
    app.add_fallible_frame_system(update_labels);
    let camera = ActiveCamera2d::centered(1.0)?;
    let initial = app.register_world("screen-text-benchmark", move |world| {
        world.spawn(camera)?;
        world.insert_resource(Updates {
            mode,
            second: false,
        })?;
        for index in 0..count {
            let mut visual = prepared.clone();
            visual
                .set_position(position(index, 0))
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            world.spawn(visual)?;
        }
        Ok(())
    })?;
    Ok(app.build_headless(initial)?)
}

fn advance_frames(runner: &mut HeadlessRunner<Action>, frames: usize) -> LogicResult {
    let viewport = LogicalViewport::new(1280.0, 1080.0)?;
    for _ in 0..frames {
        // Zero elapsed excludes fixed catch-up work. FrameUpdate and the normal
        // World-plus-screen extraction/publication still run on every frame.
        let FrameOutcome::Advanced(report) =
            runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
        else {
            return Err("screen text benchmark frame rejected".into());
        };
        if report.failure().is_some() {
            return Err(format!("screen text benchmark failed: {:?}", report.failure()).into());
        }
        black_box(
            runner
                .extracted_frame()
                .ok_or("screen text snapshot missing")?,
        );
    }
    Ok(())
}

fn verify(runner: &HeadlessRunner<Action>, count: usize) -> LogicResult {
    let snapshot = runner
        .extracted_frame()
        .ok_or("screen text snapshot missing")?;
    assert_eq!(snapshot.resolved_screen_texts().len(), count);
    let updates = runner
        .resource::<Updates>()
        .ok_or("benchmark phase missing")?;
    let expected = if updates.mode == Mode::ChangedText && updates.second {
        LINES[1]
    } else {
        LINES[0]
    };
    for label in snapshot.resolved_screen_texts() {
        assert_eq!(label.text(), expected);
        assert_eq!(label.metrics().glyph_count(), 32);
        if updates.mode == Mode::MovedTinted {
            let phase = usize::from(updates.second);
            assert_eq!(
                label.position().to_vec2().x().rem_euclid(48.0),
                phase as f32
            );
            assert_eq!(
                label.tint(),
                Color::rgba(0.5, 0.75, 1.0, if phase == 0 { 0.5 } else { 1.0 }),
            );
        }
    }
    Ok(())
}

fn measure(count: usize, mode: Mode) -> LogicResult {
    let mut runner = build(count, mode)?;
    advance_frames(&mut runner, WARMUP_FRAMES)?;

    let calls = allocation_calls(|| advance_frames(&mut runner, MEASURED_FRAMES))?;
    verify(&runner, count)?;

    let started = Instant::now();
    advance_frames(&mut runner, MEASURED_FRAMES)?;
    let elapsed = started.elapsed();
    verify(&runner, count)?;

    println!(
        "screen_text_cpu labels={count} mode={mode:?} frames={MEASURED_FRAMES} allocation_calls={calls} mean_frame_us={:.2}",
        elapsed.as_secs_f64() * 1_000_000.0 / MEASURED_FRAMES as f64,
    );
    if mode != Mode::ChangedText {
        assert_eq!(
            calls, 0,
            "unchanged/moved warmed CPU text unexpectedly allocated"
        );
    }
    Ok(())
}

fn shaping_probe() -> LogicResult {
    const ITERATIONS: usize = 1000;
    let settings = TextSettings::new(18.0)?;
    let style = settings.style(1.0)?;
    let budget = settings.layout_budget();
    let face = sim_engine::FontFace::from_bytes(FONT.to_vec(), settings.font_budget())?;
    let mut app = Application::<Action>::new(AppConfig::default())?;
    let font = app.register_font(FONT.to_vec(), settings)?;
    let mut visual = ScreenTextVisual::new(font, LINES[0], position(0, 0))?;
    for index in 0..WARMUP_FRAMES {
        black_box(face.shape_line(LINES[index % 2], &style, &budget)?);
        visual.set_text(LINES[index % 2])?;
    }
    let engine_calls = allocation_calls(|| {
        for index in 0..ITERATIONS {
            black_box(face.shape_line(LINES[index % 2], &style, &budget)?);
        }
        Ok(())
    })?;
    // First measured setter must change text rather than take its equal-text path.
    visual.set_text(LINES[1])?;
    let logic_calls = allocation_calls(|| {
        for index in 0..ITERATIONS {
            visual.set_text(LINES[index % 2])?;
            black_box(&visual);
        }
        Ok(())
    })?;
    assert_eq!(visual.text(), LINES[1]);
    println!(
        "text_shaping_probe iterations={ITERATIONS} line_bytes=32 font=DejaVuSans engine_shape_allocation_calls={engine_calls} logic_set_text_allocation_calls={logic_calls} ecs=none renderer=none timing=not_measured"
    );
    Ok(())
}

fn main() -> LogicResult {
    // Like the other custom benchmarks, do not time work when Cargo includes
    // this binary in a normal `test --all-targets` invocation.
    if !std::env::args().any(|argument| argument == "--bench") {
        println!("screen text benchmark skipped outside `cargo bench`");
        return Ok(());
    }
    if std::env::args().any(|argument| argument == "--shaping-probe") {
        return shaping_probe();
    }
    println!(
        "screen_text_cpu font=DejaVuSans line_bytes=32 warmup_frames={WARMUP_FRAMES} allocation_and_timing_passes=separate elapsed_input=zero renderer=none"
    );
    for count in [0, 1, 100, 1000] {
        for mode in [Mode::Unchanged, Mode::MovedTinted, Mode::ChangedText] {
            measure(count, mode)?;
        }
    }
    Ok(())
}

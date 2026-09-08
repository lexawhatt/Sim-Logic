use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    io,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

// Compile the real application, processor and queue. No benchmark-specific
// synthesis, layout or extraction implementation substitutes for them.
#[allow(
    dead_code,
    unused_imports,
    reason = "The harness-free benchmark uses only part of this example module and does not run its tests."
)]
#[path = "../examples/piano_roll/mod.rs"]
mod piano;

use music::{Command, MAX_VOICES, MIN_PITCH, Processor, SAMPLE_RATE, Sequence};
use piano::{control, music};
use sim_logic::prelude::*;

const WARM_UP_FRAMES: usize = 4096;
const MEASURED_FRAMES: usize = SAMPLE_RATE as usize * 4;
const UPDATE_INTERVAL: usize = 4096;
const APP_WARM_UP_FRAMES: usize = 32;
const APP_MEASURED_FRAMES: usize = 100;
const APP_FRAME_MILLISECONDS: u64 = 16;
const APP_SOURCE_FRAMES: usize = SAMPLE_RATE as usize * APP_FRAME_MILLISECONDS as usize / 1000;

struct CountingAllocator;
static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

// SAFETY: Allocation and deallocation delegate unchanged pointers and layouts
// to System. The counter only observes calls and does not inspect their memory.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        count_allocation();
        unsafe { System.realloc(pointer, layout, size) }
    }
}

fn count_allocation() {
    if COUNTING.load(Ordering::Relaxed) {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    }
}

struct Measurement;

impl Drop for Measurement {
    fn drop(&mut self) {
        COUNTING.store(false, Ordering::Relaxed);
    }
}

fn measure(label: &str, mut next: impl FnMut(usize) -> LogicResult<f32>) -> LogicResult {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let guard = Measurement;
    let started = Instant::now();
    let mut magnitude = 0.0_f64;
    for frame in 0..MEASURED_FRAMES {
        let sample = black_box(next(frame)?);
        if !sample.is_finite() || !(-1.0..=1.0).contains(&sample) {
            return Err(io::Error::other("piano benchmark emitted invalid PCM").into());
        }
        magnitude += f64::from(sample.abs());
    }
    let elapsed = started.elapsed();
    drop(guard);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    println!(
        "piano_cpu case={label} frames={MEASURED_FRAMES} rate={SAMPLE_RATE} voice_cap={MAX_VOICES} ns_per_frame={:.1} audio_seconds_per_wall_second={:.1} allocation_calls={allocations} magnitude={magnitude:.3}",
        elapsed.as_secs_f64() * 1_000_000_000.0 / MEASURED_FRAMES as f64,
        MEASURED_FRAMES as f64 / f64::from(SAMPLE_RATE) / elapsed.as_secs_f64(),
    );
    if magnitude <= 1.0 {
        return Err(io::Error::other("piano benchmark did not exercise audible synthesis").into());
    }
    if allocations != 0 {
        return Err(
            io::Error::other(format!("warmed {label} allocated {allocations} times")).into(),
        );
    }
    Ok(())
}

fn processor_case() -> LogicResult {
    let mut processor = Processor::with_sample_rate(SAMPLE_RATE)?;
    processor.apply(Command::ReplaceSequence(Sequence::demo()))?;
    processor.apply(Command::Play)?;
    for offset in 0..MAX_VOICES {
        processor.apply(Command::NoteOn {
            pitch: MIN_PITCH + offset as u8,
            velocity: 96,
        })?;
    }
    for _ in 0..WARM_UP_FRAMES {
        black_box(processor.next_sample());
    }
    let before = processor.snapshot().frames_generated;
    measure("processor", |_| Ok(processor.next_sample()))?;
    let snapshot = processor.snapshot();
    if snapshot.frames_generated - before != MEASURED_FRAMES as u64
        || !snapshot.playing
        || snapshot.active_keys.count_ones() > MAX_VOICES as u32
    {
        return Err(
            io::Error::other("processor benchmark violated its frame/voice contract").into(),
        );
    }
    Ok(())
}

fn queued_source_case() -> LogicResult {
    let (mut controller, mut stream) = control::channel(SAMPLE_RATE)?;
    let mut settings = control::Settings::default();
    if !controller.submit(settings, control::Action::Play) {
        return Err(io::Error::other("piano benchmark setup rejected Play").into());
    }
    controller.set_held_keys((1 << MAX_VOICES) - 1);
    for _ in 0..WARM_UP_FRAMES {
        black_box(stream.next());
    }
    let before = controller.snapshot().frames_generated;
    measure("queued-source", |frame| {
        if frame.is_multiple_of(UPDATE_INTERVAL) {
            let update = frame / UPDATE_INTERVAL;
            settings.tempo = 90 + (update % 19) as u16 * 5;
            settings.volume = if update.is_multiple_of(2) { 0.35 } else { 0.55 };
            if !controller.submit(settings, control::Action::Update) {
                return Err(io::Error::other("bounded control update was rejected").into());
            }
            controller.preview(MIN_PITCH + (update % 12) as u8);
            controller.set_held_keys(if update.is_multiple_of(2) {
                0x5555
            } else {
                0xAAAA
            });
        }
        stream
            .next()
            .ok_or_else(|| io::Error::other("persistent piano source ended").into())
    })?;
    let snapshot = controller.snapshot();
    if snapshot.frames_generated - before != MEASURED_FRAMES as u64
        || !snapshot.playing
        || snapshot.active_keys.count_ones() > MAX_VOICES as u32
        || controller.rejected != 0
        || controller.is_faulted()
    {
        return Err(
            io::Error::other("queued source benchmark violated its control contract").into(),
        );
    }
    controller.stop();
    for _ in 0..128 {
        black_box(stream.next());
    }
    if controller.snapshot().playing || stream.next() != Some(0.0) {
        return Err(io::Error::other("benchmark source did not stop").into());
    }
    Ok(())
}

fn application_frame(
    runner: &mut HeadlessRunner<piano::Action>,
    stream: &mut control::Stream,
    events: &[InputEvent],
    viewport: LogicalViewport,
) -> LogicResult<f64> {
    let report = match runner.advance_frame(FrameRequest::new(
        Duration::from_millis(APP_FRAME_MILLISECONDS),
        events,
        viewport,
    )) {
        FrameOutcome::Advanced(report) => report,
        FrameOutcome::Rejected(error) => return Err(error.into()),
    };
    if report.failure().is_some()
        || report.exit_requested()
        || report.spawned() != 0
        || report.despawned() != 0
        || !matches!(report.transition(), FrameTransition::None)
        || report.extracted_generation() != Some(runner.world_generation())
    {
        return Err(
            io::Error::other(format!("piano frame did not complete cleanly: {report:?}")).into(),
        );
    }
    // The persistent source is kept outside the application without opening a
    // device or installing the example's frame-driven silent-mode fallback.
    let mut magnitude = 0.0;
    for _ in 0..APP_SOURCE_FRAMES {
        let sample = black_box(
            stream
                .next()
                .ok_or_else(|| io::Error::other("persistent application source ended"))?,
        );
        if !sample.is_finite() || !(-1.0..=1.0).contains(&sample) {
            return Err(io::Error::other("application source emitted invalid PCM").into());
        }
        magnitude += f64::from(sample.abs());
    }
    Ok(magnitude)
}

fn application_counts(runner: &HeadlessRunner<piano::Action>) -> LogicResult<[usize; 4]> {
    let frame = runner
        .extracted_frame()
        .ok_or_else(|| io::Error::other("piano application did not publish extraction"))?;
    if frame.world_generation() != runner.world_generation()
        || !frame.resolved_circles().is_empty()
        || !frame.resolved_rectangles().is_empty()
        || !frame.resolved_lines().is_empty()
    {
        return Err(io::Error::other("unexpected piano world snapshot").into());
    }
    Ok([
        frame.resolved_screen_rectangles().len(),
        frame.resolved_screen_images().len(),
        frame.resolved_cuboids().len(),
        usize::from(frame.three_d().is_some()),
    ])
}

fn application_case(view: piano::View) -> LogicResult {
    let (controller, mut stream) = control::channel(SAMPLE_RATE)?;
    let (application, initial) = piano::build_application(controller, view)?;
    let mut runner = application.build_headless(initial)?;
    let viewport = LogicalViewport::new(1440.0, 900.0)?;
    for frame in 0..APP_WARM_UP_FRAMES {
        let events = match frame {
            0 => &[InputEvent::key(
                PhysicalKeyCode::Space,
                ButtonState::Pressed,
            )][..],
            1 => &[InputEvent::key(
                PhysicalKeyCode::Space,
                ButtonState::Released,
            )][..],
            _ => &[],
        };
        application_frame(&mut runner, &mut stream, events, viewport)?;
    }
    let counts = application_counts(&runner)?;
    let expected_three_d = view == piano::View::ThreeD;
    if counts[0] == 0
        || counts[1] == 0
        || (counts[2] != 0) != expected_three_d
        || (counts[3] != 0) != expected_three_d
    {
        return Err(io::Error::other("piano fixture did not exercise the requested view").into());
    }
    let before = runner
        .app_resource::<control::Control>()
        .ok_or_else(|| io::Error::other("piano control missing"))?
        .snapshot()
        .frames_generated;
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let guard = Measurement;
    let started = Instant::now();
    let mut magnitude = 0.0;
    for _ in 0..APP_MEASURED_FRAMES {
        magnitude += application_frame(&mut runner, &mut stream, &[], viewport)?;
        if application_counts(&runner)? != counts {
            return Err(io::Error::other("piano extraction counts changed during playback").into());
        }
    }
    let elapsed = started.elapsed();
    drop(guard);
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    let controller = runner
        .app_resource::<control::Control>()
        .ok_or_else(|| io::Error::other("piano control missing after measurement"))?;
    let snapshot = controller.snapshot();
    let source_frames = snapshot.frames_generated - before;
    let session = runner
        .app_resource::<piano::Session>()
        .ok_or_else(|| io::Error::other("piano session missing after measurement"))?;
    if source_frames != (APP_MEASURED_FRAMES * APP_SOURCE_FRAMES) as u64
        || !snapshot.playing
        || snapshot.active_keys.count_ones() > MAX_VOICES as u32
        || controller.rejected != 0
        || controller.is_faulted()
        || controller.is_offline()
        || !session.playing
        || session.view != view
        || magnitude <= 1.0
    {
        return Err(io::Error::other("piano application violated its source/view contract").into());
    }
    let label = if expected_three_d { "app-3d" } else { "app-2d" };
    println!(
        "piano_cpu case={label} frames={APP_MEASURED_FRAMES} delta_ms={APP_FRAME_MILLISECONDS} source_frames={source_frames} screen_rectangles={} screen_images={} cuboids={} us_per_frame={:.1} allocation_calls={allocations} magnitude={magnitude:.3}",
        counts[0],
        counts[1],
        counts[2],
        elapsed.as_secs_f64() * 1_000_000.0 / APP_MEASURED_FRAMES as f64,
    );
    if allocations != 0 {
        return Err(
            io::Error::other(format!("warmed {label} allocated {allocations} times")).into(),
        );
    }
    Ok(())
}

fn main() -> LogicResult {
    processor_case()?;
    queued_source_case()?;
    application_case(piano::View::TwoD)?;
    application_case(piano::View::ThreeD)
}

use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    mem::size_of,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
    time::Duration,
};

use sim_engine::{SceneCommand, SceneError};
use sim_logic::prelude::*;

const RECTANGLES: i32 = 64;

#[derive(Clone, Copy, Default)]
struct AllocationProbe {
    active: bool,
    retained_delta: isize,
    failure_size: usize,
    remaining_matches: usize,
    failures: usize,
}

thread_local! {
    // Constant, destructor-free TLS avoids allocation inside the allocator.
    // The test harness and any other threads always use the normal allocator.
    static PROBE: Cell<AllocationProbe> = const { Cell::new(AllocationProbe {
        active: false,
        retained_delta: 0,
        failure_size: 0,
        remaining_matches: 0,
        failures: 0,
    }) };
}

struct ProbeScope;

impl Drop for ProbeScope {
    fn drop(&mut self) {
        PROBE.with(|probe| probe.set(AllocationProbe::default()));
    }
}

fn probe_allocations<T>(
    failure_size: usize,
    occurrence: usize,
    operation: impl FnOnce() -> T,
) -> (T, AllocationProbe) {
    PROBE.with(|probe| {
        assert!(!probe.get().active, "allocation probes must not nest");
        probe.set(AllocationProbe {
            active: true,
            failure_size,
            remaining_matches: occurrence,
            ..AllocationProbe::default()
        });
    });
    let scope = ProbeScope;
    let result = operation();
    let measured = PROBE.with(Cell::get);
    drop(scope);
    (result, measured)
}

fn should_fail(size: usize) -> bool {
    PROBE
        .try_with(|probe| {
            let mut state = probe.get();
            if !state.active || size != state.failure_size || state.remaining_matches == 0 {
                return false;
            }
            state.remaining_matches -= 1;
            let fail = state.remaining_matches == 0;
            state.failures += usize::from(fail);
            probe.set(state);
            fail
        })
        .unwrap_or(false)
}

fn record_bytes(delta: isize) {
    let _ = PROBE.try_with(|probe| {
        let mut state = probe.get();
        if state.active {
            state.retained_delta += delta;
            probe.set(state);
        }
    });
}

struct FaultInjectingAllocator;

#[global_allocator]
static ALLOCATOR: FaultInjectingAllocator = FaultInjectingAllocator;

// SAFETY: Every successful allocation and all deallocations use System with
// their original Layout. Injection only returns the null failure permitted by
// GlobalAlloc. Tracking neither allocates nor dereferences the supplied pointer.
unsafe impl GlobalAlloc for FaultInjectingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            return ptr::null_mut();
        }
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_bytes(layout.size() as isize);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            return ptr::null_mut();
        }
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_bytes(layout.size() as isize);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record_bytes(-(layout.size() as isize));
        unsafe { System.dealloc(pointer, layout) };
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if should_fail(new_size) {
            return ptr::null_mut();
        }
        let result = unsafe { System.realloc(pointer, layout, new_size) };
        if !result.is_null() {
            record_bytes(new_size as isize - layout.size() as isize);
        }
        result
    }
}

#[derive(Component)]
struct ImageIndex(i32);

fn fixture(layout: Arc<AtomicI32>) -> Result<HeadlessRunner<u8>, Box<dyn std::error::Error>> {
    let mut config = AppConfig::default();
    config.set_render_limits(RenderLimits::default().with_max_screen_images(64));
    let mut application = Application::<u8>::new(config)?;
    application.approve_component::<ImageIndex>()?;
    application.add_fallible_frame_system(
        move |mut images: Query<(&ImageIndex, &mut ScreenImageVisual)>| -> LogicResult {
            let group = layout.load(Ordering::Relaxed);
            for (index, mut image) in &mut images {
                let depth = if group < 0 || index.0 < group {
                    index.0 * 2 + 1
                } else if index.0 == group {
                    (RECTANGLES - 2) * 2 - 1
                } else {
                    RECTANGLES * 2 + index.0
                };
                image.set_draw_order_depth(depth as f32)?;
            }
            Ok(())
        },
    );
    let asset = application.register_image_rgba8(1, 1, &[255; 4])?;
    let camera = ActiveCamera2d::centered(1.0)?;
    let factory = application.register_world("failed-image-partitions", move |world| {
        world.spawn(camera)?;
        for index in 0..RECTANGLES {
            let position = LogicalScreenPosition::new(index as f32, 0.0);
            let size = LogicalScreenVector::new(1.0, 1.0);
            let mut rectangle = ScreenRectangleVisual::new(position, size, Color::WHITE)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            rectangle
                .set_draw_order_depth((index * 2) as f32)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            world.spawn(rectangle)?;
            let mut image = ScreenImageVisual::new(asset, position, size)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            image
                .set_draw_order_depth((index * 2 + 1) as f32)
                .map_err(|error| WorldBuildError::user(error.to_string()))?;
            world.spawn((ImageIndex(index), image))?;
        }
        Ok(())
    })?;
    Ok(application.build_headless(factory)?)
}

fn advance(runner: &mut HeadlessRunner<u8>, viewport: LogicalViewport) -> FrameOutcome {
    runner.advance_frame(FrameRequest::new(Duration::ZERO, &[], viewport))
}

#[test]
fn failed_repartitions_release_partial_runs_and_retry_without_partial_publication() -> LogicResult {
    // Engine 0.2.0 starts a budgeted Scene's command allocation at two slots.
    // Validate the injection itself through a fallible reservation first.
    let failure_bytes = 2 * size_of::<SceneCommand>();
    let (reservation, probe) = probe_allocations(failure_bytes, 1, || {
        Vec::<SceneCommand>::new().try_reserve_exact(2)
    });
    assert!(reservation.is_err());
    assert_eq!(probe.failures, 1);

    let layout = Arc::new(AtomicI32::new(-1));
    let mut runner = fixture(Arc::clone(&layout))?;
    let viewport = LogicalViewport::new(640.0, 360.0)?;
    // Warm both extraction buffers, the ECS query, and the merged draw vector.
    for _ in 0..4 {
        let FrameOutcome::Advanced(report) = advance(&mut runner, viewport) else {
            panic!("warmup frame rejected");
        };
        assert!(report.failure().is_none());
    }
    let snapshot = runner.extracted_frame().ok_or("missing warm snapshot")?;
    let generation = snapshot.world_generation();
    let camera = snapshot.camera();
    let background = snapshot.background();
    let rectangles = snapshot.resolved_screen_rectangles().to_vec();
    let images = snapshot.resolved_screen_images().to_vec();
    let draws = snapshot.screen_draws().to_vec();
    assert_eq!(draws.len(), 128);
    let mut retained_delta = 0;
    let linear_allowance = (4 * RECTANGLES as usize * size_of::<SceneCommand>()) as isize;

    // Start with singleton runs, then move a large group through decreasing
    // run indices. Without cleanup, a failed repartition can leave its large
    // prefix beside historical large groups in untouched trailing run slots.
    // The second two-slot allocation fails fallibly; once cleanup empties the
    // spare run vector, a retry may fail earlier while rebuilding its prefix.
    for group in (0..30).rev().map(|index| index * 2) {
        layout.store(group, Ordering::Relaxed);
        let (outcome, probe) =
            probe_allocations(failure_bytes, 2, || advance(&mut runner, viewport));
        assert_eq!(probe.failures, 1, "group {group} did not inject its fault");
        let FrameOutcome::Advanced(report) = outcome else {
            panic!("group {group} rejected before extraction");
        };
        assert!(matches!(report.failure(), Some(FrameFailure::Extraction(
            ExtractionError::ScreenScene(SceneError::AllocationFailed { requested_bytes })
        )) if *requested_bytes == failure_bytes));
        let published = runner.extracted_frame().ok_or("lost published snapshot")?;
        assert_eq!(published.world_generation(), generation);
        assert_eq!(published.camera(), camera);
        assert_eq!(published.background(), background);
        assert_eq!(published.resolved_screen_rectangles(), rectangles);
        assert_eq!(published.resolved_screen_images(), images);
        assert_eq!(published.screen_draws(), draws);
        for (run, rectangle) in rectangles.iter().enumerate() {
            assert_eq!(
                published.screen_rectangle_run_records(run),
                Some(std::slice::from_ref(rectangle))
            );
        }
        retained_delta += probe.retained_delta;
        assert!(
            retained_delta < linear_allowance,
            "group {group}: rejected partitions retained {retained_delta} additional bytes"
        );
    }

    // The latest requested partition must publish after the allocator recovers,
    // in both recycled buffers, without a World generation change.
    for _ in 0..4 {
        let FrameOutcome::Advanced(report) = advance(&mut runner, viewport) else {
            panic!("recovery frame rejected");
        };
        assert!(report.failure().is_none());
        let published = runner
            .extracted_frame()
            .ok_or("missing recovered snapshot")?;
        assert_eq!(published.world_generation(), generation);
        assert_eq!(published.resolved_screen_rectangles(), rectangles);
        assert_eq!(published.resolved_screen_images().len(), 64);
        assert_eq!(
            published.screen_rectangle_run_records(0),
            Some(&rectangles[..62])
        );
        assert_eq!(
            published.screen_rectangle_run_records(1),
            Some(&rectangles[62..])
        );
        assert!(published.screen_rectangle_run_records(2).is_none());
        let draws = published.screen_draws();
        assert_eq!(draws.len(), 66);
        assert_eq!(draws[0], ScreenDraw::Rectangles { run: 0 });
        assert_eq!(draws[1], ScreenDraw::Image { index: 0 });
        assert_eq!(draws[2], ScreenDraw::Rectangles { run: 1 });
        for (index, draw) in draws[3..].iter().enumerate() {
            assert_eq!(*draw, ScreenDraw::Image { index: index + 1 });
        }
    }
    Ok(())
}

#[test]
fn successful_repartitions_replace_run_storage_and_preserve_complete_order() -> LogicResult {
    let layout = Arc::new(AtomicI32::new(-1));
    let mut runner = fixture(Arc::clone(&layout))?;
    let viewport = LogicalViewport::new(640.0, 360.0)?;
    for _ in 0..4 {
        let FrameOutcome::Advanced(report) = advance(&mut runner, viewport) else {
            panic!("warmup frame rejected");
        };
        assert!(report.failure().is_none());
    }
    let snapshot = runner.extracted_frame().ok_or("missing warm snapshot")?;
    let generation = snapshot.world_generation();
    let camera = snapshot.camera();
    let background = snapshot.background();
    let rectangles = snapshot.resolved_screen_rectangles().to_vec();
    let images = snapshot.resolved_screen_images().to_vec();
    let mut retained_delta = 0;
    let linear_allowance = (4 * RECTANGLES as usize * size_of::<SceneCommand>()) as isize;

    // Moving the large group toward later run slots exposes historical-peak
    // retention when earlier slots shrink to singletons. Reverse the sequence
    // as well to exercise dropping trailing runs. Every split is published
    // twice, so both extraction buffers undergo each changing partition.
    for step in 0..60 {
        let group = if step < 30 { step * 2 } else { (59 - step) * 2 };
        layout.store(group, Ordering::Relaxed);
        let group = group as usize;
        for _ in 0..2 {
            // Repartitioning may allocate. Only retained growth is constrained;
            // this is deliberately not a zero-allocation assertion.
            let (outcome, probe) = probe_allocations(0, 0, || advance(&mut runner, viewport));
            assert_eq!(probe.failures, 0);
            let FrameOutcome::Advanced(report) = outcome else {
                panic!("group {group} rejected before extraction");
            };
            assert!(report.failure().is_none(), "group {group}: {report:?}");
            retained_delta += probe.retained_delta;
            assert!(
                retained_delta < linear_allowance,
                "group {group}: successful partitions retained {retained_delta} additional bytes"
            );

            let published = runner
                .extracted_frame()
                .ok_or("missing repartitioned snapshot")?;
            assert_eq!(published.world_generation(), generation);
            assert_eq!(published.camera(), camera);
            assert_eq!(published.background(), background);
            assert_eq!(published.resolved_screen_rectangles(), rectangles);
            assert_eq!(published.resolved_screen_images().len(), images.len());
            for (index, (current, original)) in published
                .resolved_screen_images()
                .iter()
                .zip(&images)
                .enumerate()
            {
                assert_eq!(
                    (
                        current.source(),
                        current.image(),
                        current.position(),
                        current.size(),
                        current.tint(),
                        current.source_region(),
                        current.filter(),
                        current.layer()
                    ),
                    (
                        original.source(),
                        original.image(),
                        original.position(),
                        original.size(),
                        original.tint(),
                        original.source_region(),
                        original.filter(),
                        original.layer()
                    )
                );
                let depth = if index < group {
                    index * 2 + 1
                } else if index == group {
                    123
                } else {
                    128 + index
                };
                assert_eq!(current.draw_order_depth(), depth as f32);
            }

            let mut draws = published.screen_draws().iter().copied();
            assert_eq!(published.screen_draws().len(), group + 66);
            for (run, rectangle) in rectangles[..group].iter().enumerate() {
                assert_eq!(draws.next(), Some(ScreenDraw::Rectangles { run }));
                assert_eq!(
                    published.screen_rectangle_run_records(run),
                    Some(std::slice::from_ref(rectangle))
                );
                assert_eq!(draws.next(), Some(ScreenDraw::Image { index: run }));
            }
            assert_eq!(draws.next(), Some(ScreenDraw::Rectangles { run: group }));
            assert_eq!(
                published.screen_rectangle_run_records(group),
                Some(&rectangles[group..62])
            );
            assert_eq!(draws.next(), Some(ScreenDraw::Image { index: group }));
            assert_eq!(
                draws.next(),
                Some(ScreenDraw::Rectangles { run: group + 1 })
            );
            assert_eq!(
                published.screen_rectangle_run_records(group + 1),
                Some(&rectangles[62..])
            );
            for index in group + 1..images.len() {
                assert_eq!(draws.next(), Some(ScreenDraw::Image { index }));
            }
            assert!(draws.next().is_none());
            assert!(published.screen_rectangle_run_records(group + 2).is_none());
        }
    }
    Ok(())
}

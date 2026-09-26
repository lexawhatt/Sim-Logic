//! Resource integration checks, not pixel or frame-rate claims.
use super::*;
use crate::prelude::*;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::wayland::EventLoopBuilderExtWayland,
    window::{Window, WindowId},
};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Action {}
#[derive(Resource, Default)]
struct Step(u8);

fn runner() -> Result<HeadlessRunner<Action>, Box<dyn Error>> {
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_fallible_frame_system(
        |mut step: ResMut<Step>,
         mut rectangles: Query<&mut ScreenRectangleVisual>|
         -> LogicResult {
            for mut rectangle in &mut rectangles {
                match step.0 {
                    0 => rectangle.set_color(Color::BLACK)?,
                    1 => rectangle.set_clip(ScreenClip::Empty),
                    2 => rectangle.set_clip(ScreenClip::Unclipped),
                    3 => rectangle.set_corner_radius(3.0)?,
                    _ => (),
                }
            }
            step.0 += 1;
            Ok(())
        },
    );
    let camera = ActiveCamera2d::centered(1.0)?;
    let rectangle = ScreenRectangleVisual::new(
        LogicalScreenPosition::new(5.0, 5.0),
        LogicalScreenVector::new(12.0, 15.0),
        Color::WHITE,
    )?;
    let world = app.register_world("cache-regression", move |world| {
        world.spawn(camera)?;
        world.spawn(rectangle)?;
        world.insert_resource(Step::default())?;
        Ok(())
    })?;
    Ok(app.build_headless(world)?)
}

fn probe(renderer: &mut WgpuRenderer) -> LogicResult {
    let mut runner = runner()?;
    let mut cache = DesktopGeometry::default();
    let compact = DesktopScreenMode::Compact;
    cache.prepare(renderer, runner.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.compact_runs, 1);
    assert!(cache.updates.uploaded_bytes > 0);
    cache.prepare(renderer, runner.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.reused_runs, 1);
    assert_eq!(cache.updates.uploaded_bytes, 0);
    for step in 0..4 {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::ZERO,
            &[],
            LogicalViewport::new(64.0, 64.0)?,
        )) else {
            return Err("frame rejected".into());
        };
        assert!(report.failure().is_none());
        cache.prepare(renderer, runner.extracted_frame().unwrap(), compact)?;
        match step {
            0 => assert_eq!(cache.updates.updated_runs, 1),
            1 => {
                assert!(matches!(
                    cache.entries[0].parts[0].resource,
                    super::Resource::Empty
                ));
                assert_eq!(cache.updates.uploaded_bytes, 0);
            }
            2 => assert_eq!(cache.updates.compact_runs, 1),
            3 => assert_eq!(cache.updates.prepared_runs, 1),
            _ => unreachable!(),
        }
    }
    cache.prepare(renderer, runner.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.reused_runs, 1);
    assert_eq!(cache.updates.uploaded_bytes, 0);
    // Stock desktop recovery drops caches and reconstructs from the complete
    // CPU snapshot on the replacement device, never submits stale GPU handles.
    pollster::block_on(renderer.recover_device_and_surface())?;
    cache.clear();
    cache.prepare(renderer, runner.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.prepared_runs, 1);
    let other = self::runner()?;
    cache.prepare(renderer, other.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.compact_runs, 1);
    assert_eq!(cache.updates.reused_runs, 0);
    cache.prepare(
        renderer,
        other.extracted_frame().unwrap(),
        DesktopScreenMode::Streaming,
    )?;
    assert!(!cache.has_runs());
    // A decorative rounded rectangle must not force adjacent simple lines
    // onto the ordinary path. One edit updates only its compact part.
    let mut app = Application::<Action>::new(AppConfig::default())?;
    app.add_fallible_frame_system(|mut lines: Query<&mut ScreenLineVisual>| -> LogicResult {
        if let Some(mut line) = lines.iter_mut().next() {
            line.set_stroke(Stroke::new(2.0, Color::BLACK))?;
        }
        Ok(())
    });
    let camera = ActiveCamera2d::centered(1.0)?;
    let line = ScreenLineVisual::new(
        LogicalScreenPosition::new(2.0, 2.0),
        LogicalScreenPosition::new(20.0, 10.0),
        1.0,
        Color::WHITE,
    )?;
    let panel = ScreenRectangleVisual::rounded(
        LogicalScreenPosition::new(5.0, 5.0),
        LogicalScreenVector::new(12.0, 15.0),
        Color::WHITE,
        3.0,
    )?;
    let initial = app.register_world("mixed", move |world| {
        world.spawn(camera)?;
        world.spawn(line)?;
        world.spawn(panel)?;
        world.spawn(line)?;
        Ok(())
    })?;
    let mut mixed = app.build_headless(initial)?;
    cache.prepare(renderer, mixed.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.compact_runs, 2);
    assert_eq!(cache.updates.prepared_runs, 1);
    assert!(
        cache.entries[0]
            .parts
            .iter()
            .flat_map(|part| part.records.iter())
            .copied()
            .eq(mixed
                .extracted_frame()
                .unwrap()
                .screen_primitive_run_records(0)
                .unwrap())
    );
    let FrameOutcome::Advanced(report) = mixed.advance_frame(FrameRequest::new(
        Duration::ZERO,
        &[],
        LogicalViewport::new(64.0, 64.0)?,
    )) else {
        return Err("mixed frame rejected".into());
    };
    assert!(report.failure().is_none());
    cache.prepare(renderer, mixed.extracted_frame().unwrap(), compact)?;
    assert_eq!(cache.updates.updated_runs, 1);
    assert_eq!(cache.updates.reused_runs, 2);
    println!(
        "retained screen: unchanged, update, hide/show, fallback, generation and replacement-device checks passed"
    );
    Ok(())
}

#[derive(Default)]
struct Fixture {
    result: Option<LogicResult>,
}
impl ApplicationHandler for Fixture {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.result.is_some() {
            return;
        }
        self.result = Some((|| {
            let window = Arc::new(
                event_loop.create_window(
                    Window::default_attributes()
                        .with_inner_size(PhysicalSize::new(64, 64))
                        .with_visible(false),
                )?,
            );
            let mut renderer = pollster::block_on(WgpuRenderer::new(window, 64, 64))?;
            probe(&mut renderer)
        })());
        event_loop.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "requires isolated Wayland display and Vulkan device"]
fn retained_screen_gpu_resources() -> LogicResult {
    let mut builder = EventLoop::builder();
    builder.with_wayland().with_any_thread(true);
    let mut fixture = Fixture::default();
    builder.build()?.run_app(&mut fixture)?;
    fixture.result.ok_or("renderer did not initialize")?
}

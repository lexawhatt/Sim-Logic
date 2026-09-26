//! Native resource checks; not a pixel-equivalence or FPS oracle.
use super::*;
use crate::desktop::images;
use sim_engine::{FrameBudget, RenderStatus};
use std::{sync::Arc, time::Duration};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    platform::wayland::EventLoopBuilderExtWayland,
    window::{Window, WindowId},
};

fn probe(renderer: &mut WgpuRenderer) -> LogicResult {
    let mut runner = runner()?;
    let mut images = images::DesktopImages::new();
    let present = |renderer: &mut WgpuRenderer,
                   images: &mut images::DesktopImages,
                   runner: &HeadlessRunner<Action>|
     -> LogicResult {
        let report = images::present(
            renderer,
            runner.extracted_frame().unwrap(),
            runner.image_assets(),
            images,
            FrameBudget::default(),
            None,
        )
        .map_err(|error| format!("{error:?}"))?;
        assert_eq!(report.status(), RenderStatus::Drawn);
        Ok(())
    };
    present(renderer, &mut images, &runner)?;
    assert_eq!(images.texts.updates.created_batches, 1);
    assert_eq!(images.texts.updates.placements, 3);
    assert_eq!(images.texts.updates.glyphs, 2); // spaces have advance, not ink
    assert_eq!(images.texts.updates.draw_calls, 1);
    present(renderer, &mut images, &runner)?;
    assert_eq!(images.texts.updates.reused_batches, 1);
    assert_eq!(images.texts.updates.uploaded_bytes, 0);
    let key_pointer = images.texts.batches[0].records.as_ptr();
    for step in 0..9 {
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::ZERO,
            &[],
            LogicalViewport::new(64.0, 64.0)?,
        )) else {
            return Err("frame rejected".into());
        };
        assert!(report.failure().is_none());
        present(renderer, &mut images, &runner)?;
        let updates = images.texts.updates;
        match step {
            0 | 1 | 3 => {
                assert_eq!(updates.updated_batches, 1);
                assert!(updates.uploaded_bytes > 0);
                assert_eq!(images.texts.batches[0].records.as_ptr(), key_pointer);
            }
            2 => {
                assert_eq!(updates.updated_batches, 1);
                assert_eq!(updates.uploaded_bytes, 0); // only span clip changes
                assert_eq!(updates.draw_calls, 2);
            }
            4 => assert_eq!(updates.placements, 2),
            5 => assert_eq!(updates.placements, 3),
            6 => assert_eq!(updates.created_batches, 1), // grows past count budget
            7 => assert_eq!(images.texts.batches.len(), 2),
            8 => {
                assert!(images.texts.batches.is_empty());
                assert!(images.texts.cache.runs.is_empty());
            }
            _ => unreachable!(),
        }
        if step == 3 {
            renderer.set_scale_factor(1.25)?;
            present(renderer, &mut images, &runner)?;
            assert_eq!(images.texts.updates.created_batches, 1);
            present(renderer, &mut images, &runner)?;
            assert_eq!(images.texts.updates.reused_batches, 1);
            renderer.set_scale_factor(1.0)?;
            present(renderer, &mut images, &runner)?;
        }
    }
    let mut fresh = super::runner()?;
    images.clear(); // a new Application cannot reuse another font registry
    present(renderer, &mut images, &fresh)?;
    let before_generation = fresh.world_generation();
    let FrameOutcome::Advanced(report) = fresh.advance_frame(FrameRequest::new(
        Duration::from_millis(40),
        &[InputEvent::key(
            PhysicalKeyCode::Enter,
            ButtonState::Pressed,
        )],
        LogicalViewport::new(64.0, 64.0)?,
    )) else {
        return Err("replacement frame rejected".into());
    };
    assert!(report.failure().is_none());
    assert_ne!(fresh.world_generation(), before_generation);
    present(renderer, &mut images, &fresh)?;
    assert_eq!(images.texts.updates.created_batches, 1);
    assert_eq!(images.texts.updates.reused_batches, 0);
    assert_eq!(images.texts.updates.placements, 1);
    pollster::block_on(renderer.recover_device_and_surface())?;
    images.clear();
    present(renderer, &mut images, &fresh)?;
    assert_eq!(images.texts.updates.created_batches, 1);
    images.text_batching = false;
    present(renderer, &mut images, &fresh)?;
    assert!(images.texts.batches.is_empty());
    println!(
        "glyph groups: reuse, position/tint/text/clip, growth, hide/show, font, DPI, World replacement, device recovery and direct-run checks passed"
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
            let window = Arc::new(event_loop.create_window(
                Window::default_attributes().with_inner_size(PhysicalSize::new(64, 64)),
            )?);
            let mut renderer = pollster::block_on(WgpuRenderer::new(window, 64, 64))?;
            probe(&mut renderer)
        })());
        event_loop.exit();
    }
    fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
}

#[test]
#[ignore = "requires isolated Wayland display and Vulkan device"]
fn retained_glyph_batch_gpu_resources() -> LogicResult {
    let mut builder = EventLoop::builder();
    builder.with_wayland().with_any_thread(true);
    let mut fixture = Fixture::default();
    builder.build()?.run_app(&mut fixture)?;
    fixture.result.ok_or("renderer did not initialize")?
}

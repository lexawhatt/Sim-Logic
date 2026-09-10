//! CPU timing only: no allocator counter, GPU, or frame-rate guarantee.

use sim_logic::prelude::*;
use std::{
    hint::black_box,
    time::{Duration, Instant},
};

#[path = "../examples/territory_wars/mod.rs"]
// Cargo enables cfg(test) for harness-free benches; imported unit-test helpers
// are intentionally not executed here. They run in tests/territory_wars.rs.
#[allow(dead_code, unused_imports)]
mod territory_wars;

fn measure(debug: bool) -> LogicResult {
    let (app, world) = territory_wars::app::build_demo(territory_wars::DEFAULT_SEED, debug)?;
    let mut runner = app.build_headless(world)?;
    let viewport = LogicalViewport::new(1440.0, 900.0)?;
    let mut maximum = (0, 0);
    let mut measured = Duration::ZERO;
    for frame in 0..420 {
        let keys = [
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Pressed),
            InputEvent::key(PhysicalKeyCode::Space, ButtonState::Released),
        ];
        let events = if frame % 90 == 0 { &keys[..] } else { &[] };
        let started = Instant::now();
        let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
            Duration::from_micros(16_667),
            events,
            viewport,
        )) else {
            return Err("benchmark frame rejected".into());
        };
        let elapsed = started.elapsed();
        if report.failure().is_some() {
            return Err(format!("benchmark frame failed: {:?}", report.failure()).into());
        }
        if frame >= 60 {
            measured += elapsed;
        }
        let frame = runner
            .extracted_frame()
            .ok_or("benchmark snapshot missing")?;
        maximum.0 = maximum.0.max(frame.resolved_screen_rectangles().len());
        maximum.1 = maximum.1.max(frame.resolved_screen_images().len());
        black_box(frame);
    }
    let game = &runner
        .app_resource::<territory_wars::Session>()
        .ok_or("game state missing")?
        .game;
    assert!(game.elapsed_ticks() > 240);
    println!(
        "frontier_cpu debug={debug} frames=360 mean_frame_us={:.2} max_rectangles={} max_image_items={} simulated_ticks={}",
        measured.as_secs_f64() * 1_000_000.0 / 360.0,
        maximum.0,
        maximum.1,
        game.elapsed_ticks()
    );
    Ok(())
}

fn main() -> LogicResult {
    // Exercise the ordinary build route as well as the prepared battle.
    let (app, world) = territory_wars::build_application(territory_wars::DEFAULT_SEED)?;
    drop(app.build_headless(world)?);
    measure(false)?;
    measure(true)
}

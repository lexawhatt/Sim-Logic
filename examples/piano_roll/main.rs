mod app;
mod control;
mod labels;
mod layout;
mod model3d;
mod music;
mod view;

use sim_logic::{
    audio::{AudioOutput, AudioOutputConfig},
    prelude::*,
};
use std::time::{Duration, Instant};

fn main() -> LogicResult {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    let silent = arguments.iter().any(|value| value == "--silent");
    let three_d = arguments.iter().any(|value| value == "--3d");
    let audio_smoke = arguments.iter().any(|value| value == "--audio-smoke");
    if arguments
        .iter()
        .any(|value| !matches!(value.as_str(), "--silent" | "--3d" | "--audio-smoke"))
    {
        return Err("usage: piano_roll [--silent] [--3d] [--audio-smoke]".into());
    }
    let output = if silent {
        None
    } else {
        match AudioOutput::open(AudioOutputConfig::default().with_device_buffer_fallback(true)) {
            Ok(output) => Some(output),
            Err(error) => {
                eprintln!("Audio unavailable, continuing in explicit silent mode: {error}");
                None
            }
        }
    };
    let rate = output.as_ref().map_or(music::SAMPLE_RATE, |output| {
        output.info().sample_rate().get()
    });
    let (mut controls, stream) = control::channel(rate)?;
    let playback = match output {
        Some(output) => {
            println!(
                "Audio output: {:?}. Playhead reports source time, not speaker latency.",
                output.info()
            );
            let playback = output.play_mono(stream);
            controls.set_output_status(playback.status());
            Some(playback)
        }
        None => {
            controls.use_offline(stream);
            None
        }
    };
    if audio_smoke {
        let Some(playback) = &playback else {
            return Err("audio smoke requires a working device".into());
        };
        controls.submit(
            control::Settings {
                volume: 0.15,
                ..control::Settings::default()
            },
            control::Action::Play,
        );
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(2) && !playback.status().is_faulted() {
            std::thread::sleep(Duration::from_millis(10));
        }
        controls.stop();
        std::thread::sleep(Duration::from_millis(100));
        let status = playback.status();
        println!(
            "audio_smoke generated_samples={} invalid_samples={} faulted={} stopped={}",
            status.generated_sample_count(),
            status.invalid_sample_count(),
            status.is_faulted(),
            !controls.snapshot().playing
        );
        if status.is_faulted()
            || status.generated_sample_count() == 0
            || status.invalid_sample_count() != 0
            || controls.snapshot().playing
        {
            return Err("audio smoke failed".into());
        }
        return Ok(());
    }
    println!("Piano roll: left click draws/selects, drag changes length, right click erases.");
    println!(
        "Tuning: A4 = {} Hz, twelve-tone equal temperament.",
        music::pitch_frequency(69)?
    );
    println!(
        "Space: play/pause. Enter: 2D/3D. A/S/D/W: C/D/E/F. Arrows: length/tempo. Escape: exit."
    );
    let (application, world) = app::build_application(
        controls,
        if three_d {
            app::View::ThreeD
        } else {
            app::View::TwoD
        },
    )?;
    let result = application.run_desktop(
        world,
        DesktopConfig::new("Sim;Logic - Piano Roll", 1440.0, 900.0)?,
    );
    // This independent guard is deliberately kept alive across the entire host
    // loop. Window failure/close ends output; renderer recovery never rebuilds it.
    drop(playback);
    result?;
    Ok(())
}

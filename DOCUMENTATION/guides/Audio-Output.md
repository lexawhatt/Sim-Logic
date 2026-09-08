# Optional streaming audio output

[Documentation](../README.md) / Guides

The `audio` feature opens one output device and attaches one application-owned
mono PCM source. PCM here means a sequence of amplitude samples, not an encoded
audio file. Rodio 0.22.2 supplies device output and channel/sample conversion;
only its playback feature is enabled by this dependency.

Enable audio independently of the desktop renderer:

```toml
[dependencies]
sim-logic = { path = "../Sim-Logic", default-features = false, features = ["audio"] }
```

Keep the default features enabled as well when using the desktop application
host. On Linux, building the output backend requires ALSA development files.
Without the `audio` feature, the headless library does not require Rodio or
an audio device.

The [piano example](../../examples/piano_roll/main.rs) uses this output with an
editable synthetic score. Run `cargo run --release --features audio --example
piano_roll`; it starts silent until Play or a key audition. Its `--silent`
argument selects device-free operation explicitly.

## Open once and keep the guard alive

`AudioOutput::open` returns either a device guard or an `AudioOutputError`.
It does not turn an opening failure into an apparently successful silent
device. An application may catch the error and show its own silent mode.

Read the opened sample rate before constructing a generator, then consume
the output with `play_mono`. The generator must produce mono `f32` samples at
that exact rate. For example, this standalone program opens a persistent
silent source until Enter is pressed:

```rust,no_run
use sim_logic::audio::{AudioOutput, AudioOutputConfig};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = AudioOutput::open(AudioOutputConfig::default())?;
    let info = output.info();
    println!("Source rate: {} Hz; output channels: {}",
        info.sample_rate(), info.channels());

    // A real generator would use info.sample_rate().get() for its sample clock.
    let playback = output.play_mono(std::iter::repeat(0.0_f32));
    let status = playback.status();
    println!("Silent output is running. Press Enter to close it.");
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;

    drop(playback);
    assert!(status.is_closed());
    Ok(())
}
```

`play_mono` consumes its handle, so it cannot become an unbounded queue of
sources. `AudioPlayback` keeps the device alive. Dropping it closes output;
cloning `AudioOutputStatus` does not extend the device lifetime. There is no
detach operation. A finite iterator ends permanently on its first `None`.
An interactive generator that can resume after Stop or Pause must continue
yielding `Some(0.0)` while silent.

The source runs at the opened output rate without resampling. Rodio duplicates
mono into stereo; output channels beyond the first two receive silence.
The adapter replaces non-finite samples with zero and clamps finite samples
outside `-1.0..=1.0`. It counts both cases as invalid input samples.

## Buffer selection is not measured latency

`AudioOutputConfig::default()` requests 512 sample frames and disables
fallback. `AudioOutputConfig::new(frames)` accepts 1 through 16,384 frames;
the device can still reject a request inside that range. A frame contains
one sample per channel, so a stereo frame is not counted twice.

Opt in to a second attempt with
`config.with_device_buffer_fallback(true)`. This retries the same device and
format using its default buffer size, not another output device. The default
buffer is not bounded by the original request.

Inspect `AudioOutputInfo` for the configured sample rate and channels,
`requested_buffer_frames()`, `configured_buffer_frames()` and
`used_device_buffer_fallback()`. The configured buffer getter returns `None`
for device-default sizing. Even a fixed value describes a backend request,
not a guaranteed callback length or measured speaker latency. Operating-system
and hardware buffering can add delay.

## Diagnostics and failure policy

`AudioOutputStatus` is a cloneable set of independent atomic observations:

- `is_faulted()` and `stream_error_count()` report asynchronous backend errors,
  including underruns. The error callback updates counters without logging.
- `generated_sample_count()` counts source frames, including yielded silence
  and sanitized samples. It is neither musical transport position nor audible
  playback position.
- `invalid_sample_count()` reports replaced/clamped input samples.
- `is_finished()` means the source returned `None`, not that all buffered
  samples reached the speakers.
- `is_closed()` means the owning output guard was dropped. Already submitted
  samples cannot be recalled instantaneously.

Counters saturate rather than wrap. Reading several getters does not produce
one transactional snapshot. Stream errors remain recorded; there is no
automatic reconnection, source replay, or universal silent-on-error policy.
The application decides what to do when it polls diagnostics. The piano
example checks them during its application updates, mutes its source and
shows an error when output fails. That policy is frame-polled, not a promise
of immediate audio-thread shutdown.

## Ownership and real-time limits

Keep the output guard outside World factories, Startup, and renderer ownership.
Playing or changing source controls is an immediate external effect. A later
failed ECS stage, command barrier, or render extraction cannot undo samples
already generated. This adapter does not implement staged audio requests,
World-scoped voices, pause groups, or exactly-once physical playback.

The source must return promptly and bound its own work. Avoid locks, allocation,
file/device access and panics in its sample-generation path. Setup and backend
attachment can allocate; the adapter does not promise recovery from every
backend allocation failure or hard real-time scheduling. Synthesis, voices,
note timing and editing policy belong to the application. Source time should
not advance from GPU frames or be reset during renderer recovery.

The [device-free source tests](../../src/audio/output_tests.rs) cover sanitization,
termination, channel expansion, counters and fallback policy. The piano's
[control tests](../../examples/piano_roll/control_tests.rs) additionally cover
bounded queues, Stop epochs, accepted settings and key release. Neither set
opens an audio device or measures physical latency.

Run `cargo bench --no-default-features --bench piano_roll` for the bounded
[offline piano benchmark](../../benches/piano_roll.rs). It checks zero allocator
calls after warm-up in the actual processor, the queued source with control
updates, and the application's headless 2D/3D playback frames. Each application
case measures 100 frames at 16 ms after 32 warm-up frames, including UI layout,
CPU extraction and 768 manually generated source frames per application frame
at 48 kHz. This manual pulling is an offline fixture, not the live audio clock.
Reported application timing includes synthesis. These gates exclude arbitrary
editing/partition changes, device callbacks, GPU work and physical latency;
they are not hard real-time guarantees or timing thresholds.

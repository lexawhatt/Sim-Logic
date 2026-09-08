# Piano roll

A small, editable synthesizer built with Sim;Logic. Draw notes in a 2D score,
play them in a loop, or switch to a geometric 3D upright piano. Both views use
the same score, transport, live keys, and audio source.

```bash
cargo run --release --features audio --example piano_roll
```

The `audio` feature is optional and is not part of the library's default
features. On Linux it requires ALSA development libraries when building;
the system's normal audio output is used at runtime. Start at a comfortable
volume. There are no downloaded samples, model files, or font dependencies.

## Controls

| Control | Action |
| --- | --- |
| Left click in the score | Add or select a note and audition its pitch. |
| Drag a note horizontally | Change its ending step. |
| Right click a note | Remove it. |
| Play / Space | Start or pause the loop. |
| Stop | Silence voices and rewind; keep the score. |
| Demo / Clear | Restore the original phrase or clear the score. |
| Tempo −/+ or Down/Up | Change tempo by five beats per minute. |
| Length −/+ or Left/Right | Resize the selected note; otherwise set the next note's length. |
| Volume −/+ | Change master volume. Zero mutes without pausing. |
| 2D / 3D buttons or Enter | Switch presentation without restarting music. |
| Bottom keyboard | Click or hold a key to play it in either view. |
| A / S / D / W | Play C3 / D3 / E3 / F3. Multiple held keys form a chord. |
| Escape | Stop output and close the application. |

Pause and Stop also silence live keys. Keys already held at that moment must
be released and pressed again. A short tap between display frames still gets
a short audition. Several such taps before one audio control boundary choose
the latest pitch; this is not a recorded performance editor.

The editor has 36 chromatic keys, C3 through B5 (MIDI 48–83), 32 sixteenth-note
steps, and space for 64 notes. A note cannot extend past the loop boundary.
Tempo ranges from 60 to 180 BPM. Up to 16 voices sound simultaneously, including
live keys and release tails; the oldest voice is replaced when all slots are
occupied. Drawing chooses the first existing note covering that pitch/step.

The layout scales to the window, keeping its proportions. A tiny window is
valid but not a useful editing size. The 3D piano has 53 depth-tested cuboids,
including 36 animated keys. It is an original simple model, not a photorealistic
instrument, and its keys are played using the shared screen keyboard or keys.

## Sound and timing

Pitch uses twelve-tone equal temperament:

```text
frequency_hz = 440 × 2^((midi_note − 69) / 12)
```

Four decaying harmonics, a short attack and a release envelope make the tone
piano-like. This is deliberately synthetic: it does not model a piano's
strings, soundboard, pedals, room acoustics, or recorded hammer noise.

The audio device pulls samples from one persistent source. Sequencer time is
counted in those samples at the device's actual rate, including 44.1 and 48 kHz.
The score advances independently of drawing, and the playhead is a read-only
observation of that clock. Fractional step progress is preserved between loops
and tempo changes. It is source time, not a measurement of when a speaker
actually produces sound; operating-system and device buffers add latency.

The editor sends copyable settings through a 32-entry queue. Controls are read
at 64-sample boundaries. A full queue rejects an edit visibly and keeps the
previous score; it does not silently accept a note that audio never sees.
Stop and held-key releases do not need a queue slot. Score changes preserve
transport position and live keys, but restart currently sounding sequenced
voices to match the new score. Changing the view sends no music command.

The synthesizer and its control consumer allocate no memory during steady
sample generation. This is not a hard real-time guarantee for the OS or audio
backend. See [audio output](Audio-Output.md) for the narrower library contract.

## Silent mode and failures

```bash
cargo run --release --features audio --example piano_roll -- --silent
cargo run --release --features audio --example piano_roll -- --3d
```

`--silent` explicitly avoids opening an audio device. If opening audio fails,
the example reports why and continues with a `SILENT` banner. This fallback
advances a bounded amount of offline synthesis per frame so editing and both
views remain usable. It does not provide the live device's independent clock.

The example requests 512 device-buffer frames and explicitly permits a retry
using the same device's default buffer. The selected configuration is printed.
It does not silently switch devices or promise a particular audible latency.
An asynchronous backend error produces `AUDIO ERROR`; the next application
update stops/mutes the source and rejects further audio changes. Window close
or a fatal host error drops the output guard. There is no automatic device
reconnection, and renderer recovery does not reopen the audio stream.

## Source and tests

- [music.rs](../examples/piano_roll/music.rs): validated notes, fixed score,
  sample-clock transport, bounded voices, and synthesis.
- [control.rs](../examples/piano_roll/control.rs): bounded cross-thread controls,
  independent Stop/release signals, and display snapshots.
- [app.rs](../examples/piano_roll/app.rs): editing policy, bindings and setup.
- [view.rs](../examples/piano_roll/view.rs) and
  [layout.rs](../examples/piano_roll/layout.rs): reused visual pools, scaled
  controls and event-time pointer hit testing.
- [model3d.rs](../examples/piano_roll/model3d.rs): the instrument and key animation.
- [labels.rs](../examples/piano_roll/labels.rs): 66 immutable bitmap labels,
  registered once from the example's original 5×7 glyphs.
- [main.rs](../examples/piano_roll/main.rs): device lifetime and window entry.

Music, editor input, and both extracted views are tested without opening a
device or window:

```bash
cargo test --no-default-features --test piano_roll
cargo bench --no-default-features --bench piano_roll
```

For a short, low-volume test of the actual device and source callback:

```bash
cargo run --release --features audio --example piano_roll -- --audio-smoke
```

This plays two seconds of the demo, stops it, and checks sample generation,
sanitization counts and backend status. It does not measure physical speaker
latency or replace listening to the instrument yourself.

This example is not a complete music workstation. Saving, MIDI devices/files,
recording, velocity editing, undo, zoom/scroll, sustain pedals, and arbitrary
3D model import are not implemented. Its domain code stays in the example;
reusable device output and the [cuboid rendering bridge](ThreeD.md) are library
features. No general-purpose text or UI toolkit is implied by the fixed controls.

//! The piano example's bounded score, sample-time transport, and synthetic voice.
//!
//! This module has no renderer, clock, audio device, queue, or framework dependency.
//! Commands take effect before the next generated sample. A host may deliver them
//! through its bounded audio queue; changing the visual representation is not a
//! musical command. The output is mono at the configured sample rate, independent
//! of display updates; the offline default is 48 kHz.

use std::{error::Error, f64::consts::TAU, fmt};

/// Default source sample rate, also used by the offline tests.
pub const SAMPLE_RATE: u32 = 48_000;
/// Lowest editable MIDI pitch, C3.
pub const MIN_PITCH: u8 = 48;
/// Highest editable MIDI pitch, B5.
pub const MAX_PITCH: u8 = 83;
/// Number of pitches in the piano roll.
pub const KEY_COUNT: usize = (MAX_PITCH - MIN_PITCH + 1) as usize;
/// Length of the repeating score in sixteenth-note steps.
pub const STEP_COUNT: u8 = 32;
/// Maximum number of independently editable notes.
pub const MAX_NOTES: usize = 64;
/// Maximum simultaneous sounding voices, including release tails and live keys.
pub const MAX_VOICES: usize = 16;

const PARTIAL_COUNT: usize = 4;
const PARTIAL_WEIGHTS: [f64; PARTIAL_COUNT] = [1.0, 0.28, 0.10, 0.035];
const PARTIAL_NORMALIZATION: f64 = 1.0 / 1.415;

/// A rejected score edit or processor command; rejection leaves state unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MusicError {
    /// The pitch lies outside the example's keyboard.
    InvalidPitch(u8),
    /// A note starts outside the 32-step loop.
    InvalidStart(u8),
    /// A note is empty or extends beyond the end of the loop.
    InvalidDuration { start: u8, duration: u8 },
    /// Velocity must be between 1 and 127 inclusive.
    InvalidVelocity(u8),
    /// Tempo must be between 60 and 180 beats per minute inclusive.
    InvalidTempo(u16),
    /// Volume must be finite and between zero and one inclusive.
    InvalidVolume,
    /// The source rate must be between 8,000 and 192,000 samples per second.
    InvalidSampleRate(u32),
    /// All 64 score slots are occupied.
    FullSequence,
    /// The requested slot is outside the fixed score storage.
    InvalidNoteIndex(usize),
    /// The requested slot does not contain a note.
    MissingNote(usize),
}

impl fmt::Display for MusicError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPitch(pitch) => write!(formatter, "pitch {pitch} is outside 48..=83"),
            Self::InvalidStart(start) => write!(formatter, "start step {start} is outside 0..32"),
            Self::InvalidDuration { start, duration } => write!(
                formatter,
                "duration {duration} at step {start} must be positive and end by step 32"
            ),
            Self::InvalidVelocity(velocity) => {
                write!(formatter, "velocity {velocity} is outside 1..=127")
            }
            Self::InvalidTempo(tempo) => write!(formatter, "tempo {tempo} is outside 60..=180"),
            Self::InvalidVolume => formatter.write_str("volume must be finite and in 0..=1"),
            Self::InvalidSampleRate(rate) => {
                write!(formatter, "sample rate {rate} is outside 8000..=192000")
            }
            Self::FullSequence => formatter.write_str("all 64 note slots are occupied"),
            Self::InvalidNoteIndex(index) => {
                write!(formatter, "note slot {index} is outside 0..64")
            }
            Self::MissingNote(index) => write!(formatter, "note slot {index} is empty"),
        }
    }
}

impl Error for MusicError {}

/// A validated note. Notes may overlap, but do not wrap around the loop boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    pitch: u8,
    start: u8,
    duration: u8,
    velocity: u8,
}

impl Note {
    /// Creates a note in MIDI 48..=83, with a positive length and velocity 1..=127.
    pub fn new(pitch: u8, start: u8, duration: u8, velocity: u8) -> Result<Self, MusicError> {
        validate_pitch(pitch)?;
        validate_velocity(velocity)?;
        if start >= STEP_COUNT {
            return Err(MusicError::InvalidStart(start));
        }
        validate_duration(start, duration)?;
        Ok(Self {
            pitch,
            start,
            duration,
            velocity,
        })
    }

    /// MIDI note number; A4 is 69.
    pub const fn pitch(self) -> u8 {
        self.pitch
    }

    /// Starting sixteenth-note step, from zero through 31.
    pub const fn start(self) -> u8 {
        self.start
    }

    /// Positive duration measured in sixteenth-note steps.
    pub const fn duration(self) -> u8 {
        self.duration
    }

    /// MIDI-style attack strength, from 1 through 127.
    pub const fn velocity(self) -> u8 {
        self.velocity
    }

    fn contains(self, step: u8) -> bool {
        self.start <= step && step < self.start + self.duration
    }
}

/// Copyable score storage. Slots remain stable until explicitly removed/reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sequence {
    notes: [Option<Note>; MAX_NOTES],
}

impl Default for Sequence {
    fn default() -> Self {
        Self::new()
    }
}

impl Sequence {
    /// Creates an empty score without allocating.
    pub const fn new() -> Self {
        Self {
            notes: [None; MAX_NOTES],
        }
    }

    /// A small original C-major phrase for trying the transport and editor.
    pub fn demo() -> Self {
        let mut sequence = Self::new();
        for (index, pitch) in [60, 64, 67, 72, 71, 67, 64, 62].into_iter().enumerate() {
            sequence.notes[index] = Some(Note {
                pitch,
                start: index as u8 * 4,
                duration: 3,
                velocity: 100,
            });
        }
        for (index, pitch) in [48, 53, 55, 48].into_iter().enumerate() {
            sequence.notes[8 + index] = Some(Note {
                pitch,
                start: index as u8 * 8,
                duration: 7,
                velocity: 86,
            });
        }
        sequence
    }

    /// Fixed note slots for allocation-free iteration and UI selection.
    pub const fn notes(&self) -> &[Option<Note>; MAX_NOTES] {
        &self.notes
    }

    /// Adds a validated note to the first vacant slot, returning its index.
    pub fn add(&mut self, note: Note) -> Result<usize, MusicError> {
        let index = self
            .notes
            .iter()
            .position(Option::is_none)
            .ok_or(MusicError::FullSequence)?;
        self.notes[index] = Some(note);
        Ok(index)
    }

    /// Removes one occupied slot; invalid or empty indices leave the score intact.
    pub fn remove(&mut self, index: usize) -> Result<Note, MusicError> {
        self.notes
            .get_mut(index)
            .ok_or(MusicError::InvalidNoteIndex(index))?
            .take()
            .ok_or(MusicError::MissingNote(index))
    }

    /// Changes a note's length atomically, retaining its pitch, start, and velocity.
    pub fn resize(&mut self, index: usize, duration: u8) -> Result<(), MusicError> {
        let note = self
            .notes
            .get_mut(index)
            .ok_or(MusicError::InvalidNoteIndex(index))?
            .as_mut()
            .ok_or(MusicError::MissingNote(index))?;
        validate_duration(note.start, duration)?;
        note.duration = duration;
        Ok(())
    }
}

/// Bounded commands copied from the editor to the sample-generating processor.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::large_enum_variant,
    reason = "The bounded audio queue copies a fixed score; boxing would allocate on edits."
)]
pub enum Command {
    /// Replaces the score. Playing sequence voices restart at the current step;
    /// live keys continue. No transport rewind occurs.
    ReplaceSequence(Sequence),
    /// Starts or resumes from the current sample position; repeated Play is a no-op.
    Play,
    /// Freezes the position and immediately silences all voices, including live keys.
    Pause,
    /// Silences all voices and rewinds the score, including its fractional step.
    Stop,
    /// Changes tempo without rewinding or changing the current fractional step.
    SetTempo(u16),
    /// Sets the finite master gain in 0..=1; default is 0.75.
    SetVolume(f32),
    /// Strikes a live key independently of the score, retriggering the same live key.
    NoteOn { pitch: u8, velocity: u8 },
    /// Releases only the live key at this pitch; matching sequenced notes continue.
    NoteOff { pitch: u8 },
}

/// Source-time telemetry, not a measurement of device latency or audible position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransportSnapshot {
    /// Whether the sample-time score transport is advancing.
    pub playing: bool,
    /// Fractional sixteenth-note position in the 32-step loop.
    pub step: f32,
    /// Bit zero is MIDI 48; bits describe sounding, unreleased keys, not release tails.
    pub active_keys: u64,
    /// All generated mono frames since construction, including paused/stopped silence.
    pub frames_generated: u64,
    /// Playing frames since the last loop boundary or Stop; a pause preserves this.
    pub transport_sample_position: u64,
}

/// A fixed-capacity piano-like synthesizer and looping score transport.
///
/// It uses four decaying harmonics, a 4 ms attack, and a 140 ms key-release tail.
/// At capacity it replaces the oldest voice, breaking ties by slot. Mixing is
/// softly bounded to -1..=1. Score changes restart sequenced voices, which can
/// produce an audible reattack; pause/stop intentionally silence immediately.
/// Construction precomputes pitch rotations and envelope multipliers. Applying
/// commands and generating samples allocate nothing and perform bounded work.
pub struct Processor {
    sample_rate: u32,
    sequence: Sequence,
    tempo: u16,
    volume: f32,
    playing: bool,
    step: u8,
    phase: u32,
    frames_generated: u64,
    transport_sample_position: u64,
    voices: [Voice; MAX_VOICES],
    patches: [Patch; KEY_COUNT],
    next_voice_serial: u64,
}

impl Default for Processor {
    fn default() -> Self {
        Self::new()
    }
}

impl Processor {
    /// Creates an empty, stopped score at 48 kHz, 120 BPM, and volume 0.75.
    pub fn new() -> Self {
        Self::build(SAMPLE_RATE)
    }

    /// Uses a device/source rate in 8,000..=192,000 Hz. No device is opened.
    /// Invalid rates are rejected before constructing the processor.
    pub fn with_sample_rate(rate: u32) -> Result<Self, MusicError> {
        if !(8_000..=192_000).contains(&rate) {
            return Err(MusicError::InvalidSampleRate(rate));
        }
        Ok(Self::build(rate))
    }

    fn build(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            sequence: Sequence::new(),
            tempo: 120,
            volume: 0.75,
            playing: false,
            step: 0,
            phase: 0,
            frames_generated: 0,
            transport_sample_position: 0,
            voices: [Voice::SILENT; MAX_VOICES],
            patches: std::array::from_fn(|index| Patch::new(MIN_PITCH + index as u8, sample_rate)),
            next_voice_serial: 0,
        }
    }

    /// Applies a command atomically before the next sample, without allocating.
    pub fn apply(&mut self, command: Command) -> Result<(), MusicError> {
        match command {
            Command::ReplaceSequence(sequence) => {
                self.sequence = sequence;
                self.reconcile_sequence();
            }
            Command::Play if !self.playing => {
                self.playing = true;
                self.reconcile_sequence();
            }
            Command::Play => {}
            Command::Pause => {
                self.playing = false;
                self.voices.fill(Voice::SILENT);
            }
            Command::Stop => {
                self.playing = false;
                self.step = 0;
                self.phase = 0;
                self.transport_sample_position = 0;
                self.voices.fill(Voice::SILENT);
            }
            Command::SetTempo(tempo) => {
                if !(60..=180).contains(&tempo) {
                    return Err(MusicError::InvalidTempo(tempo));
                }
                self.tempo = tempo;
            }
            Command::SetVolume(volume) => {
                if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
                    return Err(MusicError::InvalidVolume);
                }
                self.volume = volume;
            }
            Command::NoteOn { pitch, velocity } => {
                validate_pitch(pitch)?;
                validate_velocity(velocity)?;
                for voice in &mut self.voices {
                    if voice.owner == Owner::Live && voice.pitch == pitch {
                        *voice = Voice::SILENT;
                    }
                }
                self.strike(pitch, velocity, Owner::Live);
            }
            Command::NoteOff { pitch } => {
                validate_pitch(pitch)?;
                for voice in &mut self.voices {
                    if voice.owner == Owner::Live && voice.pitch == pitch {
                        voice.release();
                    }
                }
            }
        }
        Ok(())
    }

    /// Generates one finite mono sample in -1..=1 at the configured source rate.
    /// Live keys sound while stopped; only Play advances the looping score.
    pub fn next_sample(&mut self) -> f32 {
        let mut mixed = 0.0;
        for voice in &mut self.voices {
            mixed += voice.next_sample();
        }
        self.frames_generated = self.frames_generated.saturating_add(1);
        if self.playing {
            self.transport_sample_position = self.transport_sample_position.saturating_add(1);
            self.phase += u32::from(self.tempo) * 4;
            if self.phase >= self.sample_rate * 60 {
                self.phase -= self.sample_rate * 60;
                self.step += 1;
                if self.step == STEP_COUNT {
                    self.step = 0;
                    self.transport_sample_position = 0;
                }
                self.advance_sequence();
            }
        }
        let gained = mixed * 0.28;
        (gained / (1.0 + gained.abs()) * f64::from(self.volume)) as f32
    }

    /// Returns a copy of the next source sample's transport/key state.
    pub fn snapshot(&self) -> TransportSnapshot {
        let mut active_keys = 0;
        for voice in &self.voices {
            if voice.owner != Owner::Silent && !voice.released {
                active_keys |= 1_u64 << (voice.pitch - MIN_PITCH);
            }
        }
        TransportSnapshot {
            playing: self.playing,
            step: (f32::from(self.step) + self.phase as f32 / (self.sample_rate * 60) as f32)
                .min(f32::from(STEP_COUNT).next_down()),
            active_keys,
            frames_generated: self.frames_generated,
            transport_sample_position: self.transport_sample_position,
        }
    }

    /// Source frames per second, chosen at construction and never changed mid-note.
    pub const fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// Current tempo in beats per minute.
    pub const fn tempo(&self) -> u16 {
        self.tempo
    }

    /// Current master gain, from zero through one.
    pub const fn volume(&self) -> f32 {
        self.volume
    }

    /// Current immutable score; edits arrive as a complete bounded replacement.
    pub const fn sequence(&self) -> &Sequence {
        &self.sequence
    }

    fn reconcile_sequence(&mut self) {
        for voice in &mut self.voices {
            if matches!(voice.owner, Owner::Sequence(_)) {
                *voice = Voice::SILENT;
            }
        }
        if self.playing {
            for index in 0..MAX_NOTES {
                if let Some(note) = self.sequence.notes[index]
                    && note.contains(self.step)
                {
                    self.strike(note.pitch, note.velocity(), Owner::Sequence(index));
                }
            }
        }
    }

    fn advance_sequence(&mut self) {
        // End old gates before starting this boundary's notes, including at wrap.
        for voice in &mut self.voices {
            if let Owner::Sequence(index) = voice.owner
                && (self.step == 0
                    || self.sequence.notes[index].is_none_or(|note| !note.contains(self.step)))
            {
                voice.release();
            }
        }
        for index in 0..MAX_NOTES {
            if let Some(note) = self.sequence.notes[index]
                && note.start == self.step
            {
                self.strike(note.pitch, note.velocity(), Owner::Sequence(index));
            }
        }
    }

    fn strike(&mut self, pitch: u8, velocity: u8, owner: Owner) {
        let index = self
            .voices
            .iter()
            .position(|voice| voice.owner == Owner::Silent)
            .unwrap_or_else(|| {
                self.voices
                    .iter()
                    .enumerate()
                    .min_by_key(|(index, voice)| (voice.serial, *index))
                    .map_or(0, |(index, _)| index)
            });
        self.voices[index] = Voice::new(
            pitch,
            velocity,
            owner,
            self.next_voice_serial,
            self.patches[usize::from(pitch - MIN_PITCH)],
        );
        self.next_voice_serial = self.next_voice_serial.saturating_add(1);
    }
}

/// Twelve-tone equal temperament in hertz, with MIDI 69 equal to 440 Hz.
/// Only pitches on this example's keyboard are accepted.
pub fn pitch_frequency(pitch: u8) -> Result<f64, MusicError> {
    validate_pitch(pitch)?;
    Ok(440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0))
}

fn validate_pitch(pitch: u8) -> Result<(), MusicError> {
    if !(MIN_PITCH..=MAX_PITCH).contains(&pitch) {
        return Err(MusicError::InvalidPitch(pitch));
    }
    Ok(())
}

fn validate_velocity(velocity: u8) -> Result<(), MusicError> {
    if !(1..=127).contains(&velocity) {
        return Err(MusicError::InvalidVelocity(velocity));
    }
    Ok(())
}

fn validate_duration(start: u8, duration: u8) -> Result<(), MusicError> {
    if duration == 0 || u16::from(start) + u16::from(duration) > u16::from(STEP_COUNT) {
        return Err(MusicError::InvalidDuration { start, duration });
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Patch {
    attack_samples: u32,
    release_samples: u32,
    release_multiplier: f64,
    sine_increment: [f64; PARTIAL_COUNT],
    cosine_increment: [f64; PARTIAL_COUNT],
    decay: [f64; PARTIAL_COUNT],
}

impl Patch {
    fn new(pitch: u8, sample_rate: u32) -> Self {
        let frequency = 440.0 * 2.0_f64.powf((f64::from(pitch) - 69.0) / 12.0);
        let increments = std::array::from_fn::<_, PARTIAL_COUNT, _>(|index| {
            TAU * frequency * (index + 1) as f64 / f64::from(sample_rate)
        });
        let decay_seconds = [2.6, 1.7, 1.0, 0.55];
        let pitch_decay_scale = (440.0 / frequency).sqrt();
        Self {
            attack_samples: sample_rate / 250,
            release_samples: sample_rate * 14 / 100,
            release_multiplier: (0.0001_f64.ln() / f64::from(sample_rate * 14 / 100)).exp(),
            sine_increment: increments.map(f64::sin),
            cosine_increment: increments.map(f64::cos),
            decay: decay_seconds.map(|seconds| {
                (-1.0 / (seconds * pitch_decay_scale * f64::from(sample_rate))).exp()
            }),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Owner {
    Silent,
    Live,
    Sequence(usize),
}

#[derive(Clone, Copy)]
struct Voice {
    owner: Owner,
    pitch: u8,
    serial: u64,
    patch: Patch,
    sine: [f64; PARTIAL_COUNT],
    cosine: [f64; PARTIAL_COUNT],
    partial_gain: [f64; PARTIAL_COUNT],
    age: u32,
    released: bool,
    release_remaining: u32,
    release_gain: f64,
}

impl Voice {
    const SILENT: Self = Self {
        owner: Owner::Silent,
        pitch: MIN_PITCH,
        serial: 0,
        patch: Patch {
            attack_samples: 1,
            release_samples: 1,
            release_multiplier: 0.0,
            sine_increment: [0.0; PARTIAL_COUNT],
            cosine_increment: [1.0; PARTIAL_COUNT],
            decay: [0.0; PARTIAL_COUNT],
        },
        sine: [0.0; PARTIAL_COUNT],
        cosine: [1.0; PARTIAL_COUNT],
        partial_gain: [0.0; PARTIAL_COUNT],
        age: 0,
        released: false,
        release_remaining: 0,
        release_gain: 1.0,
    };

    fn new(pitch: u8, velocity: u8, owner: Owner, serial: u64, patch: Patch) -> Self {
        let velocity_gain = (f64::from(velocity) / 127.0).powi(2);
        Self {
            owner,
            pitch,
            serial,
            patch,
            partial_gain: PARTIAL_WEIGHTS.map(|weight| weight * velocity_gain),
            ..Self::SILENT
        }
    }

    fn release(&mut self) {
        if !self.released && self.owner != Owner::Silent {
            self.released = true;
            self.release_remaining = self.patch.release_samples;
        }
    }

    fn next_sample(&mut self) -> f64 {
        if self.owner == Owner::Silent {
            return 0.0;
        }
        let attack = f64::from(self.age.min(self.patch.attack_samples))
            / f64::from(self.patch.attack_samples);
        let mut sample = 0.0;
        for index in 0..PARTIAL_COUNT {
            sample += self.sine[index] * self.partial_gain[index];
            let old_sine = self.sine[index];
            self.sine[index] = old_sine * self.patch.cosine_increment[index]
                + self.cosine[index] * self.patch.sine_increment[index];
            self.cosine[index] = self.cosine[index] * self.patch.cosine_increment[index]
                - old_sine * self.patch.sine_increment[index];
            self.partial_gain[index] *= self.patch.decay[index];
        }
        sample *= attack * self.release_gain * PARTIAL_NORMALIZATION;
        self.age = self.age.saturating_add(1);
        // Rotation recurrence avoids per-sample trigonometry. Occasional
        // renormalization bounds floating-point drift for long held keys.
        if self.age.is_multiple_of(1024) {
            for index in 0..PARTIAL_COUNT {
                let length = self.sine[index].hypot(self.cosine[index]);
                self.sine[index] /= length;
                self.cosine[index] /= length;
            }
        }
        if self.released {
            self.release_gain *= self.patch.release_multiplier;
            self.release_remaining -= 1;
        }
        if (self.released && self.release_remaining == 0) || self.partial_gain[0] < 1.0e-7 {
            self.owner = Owner::Silent;
        }
        sample
    }
}

#[cfg(test)]
#[path = "music_tests.rs"]
mod tests;

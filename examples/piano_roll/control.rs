//! Bounded app-to-audio controls. Only the producer side uses a mutex.
//!
//! Submission is immediate, not an ECS transaction. A fatal host error drops
//! the output guard. Source time is not a measurement of speaker latency.

use super::music::{
    Command, MAX_PITCH, MIN_PITCH, MusicError, Processor, Sequence, TransportSnapshot,
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

pub const CONTROL_CAPACITY: usize = 32;
const CONTROL_BLOCK: u32 = 64;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub sequence: Sequence,
    pub tempo: u16,
    pub volume: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sequence: Sequence::demo(),
            tempo: 110,
            volume: 0.55,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Action {
    Update,
    Play,
    Pause,
}

#[derive(Clone, Copy)]
struct Packet {
    epoch: u64,
    settings: Settings,
    action: Action,
}

#[derive(Default)]
struct Shared {
    stop_epoch: AtomicU64,
    held_keys: AtomicU64,
    preview: AtomicU64,
    step_bits: AtomicU32,
    active_keys: AtomicU64,
    frames: AtomicU64,
    transport_frames: AtomicU64,
    playing: AtomicBool,
    faulted: AtomicBool,
}

pub struct Control {
    // Producer is Send, not Sync; AppRes requires both. Exclusive AppResMut
    // uses get_mut, so neither UI submission nor the audio thread takes a lock.
    producer: Mutex<Producer<Packet>>,
    shared: Arc<Shared>,
    offline: Option<Mutex<Stream>>,
    offline_remainder: f64,
    preview_sequence: u64,
    pub rejected: u64,
    #[cfg(feature = "audio")]
    output_status: Option<sim_logic::audio::AudioOutputStatus>,
}

pub struct Stream {
    consumer: Consumer<Packet>,
    shared: Arc<Shared>,
    processor: Processor,
    epoch: u64,
    block_remaining: u32,
    last_keys: u64,
    last_preview: u64,
    preview_key: u64,
    preview_remaining: u32,
}

pub fn channel(rate: u32) -> Result<(Control, Stream), MusicError> {
    let mut processor = Processor::with_sample_rate(rate)?;
    let settings = Settings::default();
    processor.apply(Command::ReplaceSequence(settings.sequence))?;
    processor.apply(Command::SetTempo(settings.tempo))?;
    processor.apply(Command::SetVolume(settings.volume))?;
    // rtrb's one bounded setup allocation is infallible. No allocation-failure
    // recovery or hard real-time guarantee is claimed for dependencies.
    let (producer, consumer) = RingBuffer::new(CONTROL_CAPACITY);
    let shared = Arc::new(Shared::default());
    Ok((
        Control {
            producer: Mutex::new(producer),
            shared: Arc::clone(&shared),
            offline: None,
            offline_remainder: 0.0,
            preview_sequence: 0,
            rejected: 0,
            #[cfg(feature = "audio")]
            output_status: None,
        },
        Stream {
            consumer,
            shared,
            processor,
            epoch: 0,
            block_remaining: 0,
            last_keys: 0,
            last_preview: 0,
            preview_key: 0,
            preview_remaining: 0,
        },
    ))
}

impl Control {
    pub fn submit(&mut self, settings: Settings, action: Action) -> bool {
        // Every public editing operation validates these values first; checking
        // here also keeps malformed test/caller data away from the audio thread.
        if !(60..=180).contains(&settings.tempo)
            || !settings.volume.is_finite()
            || !(0.0..=1.0).contains(&settings.volume)
            || self.is_faulted()
        {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        let packet = Packet {
            epoch: self.shared.stop_epoch.load(Ordering::Acquire),
            settings,
            action,
        };
        let accepted = self
            .producer
            .get_mut()
            .is_ok_and(|producer| !producer.is_abandoned() && producer.push(packet).is_ok());
        if !accepted {
            self.rejected = self.rejected.saturating_add(1);
        }
        accepted
    }

    pub fn stop(&mut self) {
        self.shared.held_keys.store(0, Ordering::Release);
        self.shared.preview.store(0, Ordering::Release);
        if self
            .shared
            .stop_epoch
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |epoch| {
                epoch.checked_add(1)
            })
            .is_err()
        {
            self.shared.faulted.store(true, Ordering::Release);
        }
    }

    pub fn set_held_keys(&mut self, keys: u64) {
        self.shared.held_keys.store(
            keys & ((1u64 << (MAX_PITCH - MIN_PITCH + 1)) - 1),
            Ordering::Release,
        );
    }

    /// One short editor audition. Multiple presses before an audio boundary
    /// intentionally choose the latest pitch; held keys use a separate mask.
    pub fn preview(&mut self, pitch: u8) {
        if !(MIN_PITCH..=MAX_PITCH).contains(&pitch) {
            return;
        }
        if let Some(sequence) = self
            .preview_sequence
            .checked_add(1)
            .filter(|value| *value <= u64::MAX >> 8)
        {
            self.preview_sequence = sequence;
            self.shared
                .preview
                .store((sequence << 8) | u64::from(pitch), Ordering::Release);
        }
    }

    pub fn snapshot(&self) -> TransportSnapshot {
        // Independent approximate display values; this is not a canonical
        // cross-thread transaction or a sample-accurate speaker clock.
        TransportSnapshot {
            playing: self.shared.playing.load(Ordering::Relaxed),
            step: f32::from_bits(self.shared.step_bits.load(Ordering::Relaxed)),
            active_keys: self.shared.active_keys.load(Ordering::Relaxed),
            frames_generated: self.shared.frames.load(Ordering::Relaxed),
            transport_sample_position: self.shared.transport_frames.load(Ordering::Relaxed),
        }
    }

    pub fn is_faulted(&self) -> bool {
        #[cfg(feature = "audio")]
        if self
            .output_status
            .as_ref()
            .is_some_and(|status| status.is_faulted() || status.is_closed() || status.is_finished())
        {
            return true;
        }
        self.shared.faulted.load(Ordering::Relaxed)
    }

    #[cfg(feature = "audio")]
    pub fn set_output_status(&mut self, status: sim_logic::audio::AudioOutputStatus) {
        self.output_status = Some(status);
    }

    pub fn poll_fault(&mut self) -> bool {
        if !self.is_faulted() {
            return false;
        }
        if !self.shared.faulted.load(Ordering::Relaxed) {
            self.stop();
            self.shared.faulted.store(true, Ordering::Release);
        }
        true
    }
    pub fn is_offline(&self) -> bool {
        self.offline.is_some()
    }

    pub fn use_offline(&mut self, stream: Stream) {
        self.offline = Some(Mutex::new(stream));
    }

    pub fn advance_offline(&mut self, seconds: f64) {
        let Some(stream) = self
            .offline
            .as_mut()
            .and_then(|source| source.get_mut().ok())
        else {
            return;
        };
        // Silent fallback is explicitly wall-frame driven and bounded. It
        // is not the sample-clock guarantee of a running device source.
        let frames = seconds.clamp(0.0, 0.1) * f64::from(stream.processor.sample_rate())
            + self.offline_remainder;
        self.offline_remainder = frames.fract();
        for _ in 0..frames as u32 {
            let _ = stream.next();
        }
    }
}

impl Stream {
    fn apply(&mut self, command: Command) {
        if self.processor.apply(command).is_err() {
            self.shared.faulted.store(true, Ordering::Release);
        }
    }

    fn refresh_epoch(&mut self) {
        let epoch = self.shared.stop_epoch.load(Ordering::Acquire);
        if epoch != self.epoch {
            self.apply(Command::Stop);
            self.epoch = epoch;
            self.last_keys = 0;
            self.last_preview = 0;
            self.preview_key = 0;
            self.preview_remaining = 0;
        }
    }

    fn packet(&mut self, packet: Packet) {
        // Stop may have arrived after the first boundary read, followed by a
        // new Play packet. Refresh before classifying it, never drop the future.
        self.refresh_epoch();
        // Stop cancels transport actions, not settings already accepted by the
        // editor. In particular, a queued mute must also affect live audition.
        if self.processor.sequence() != &packet.settings.sequence {
            self.apply(Command::ReplaceSequence(packet.settings.sequence));
        }
        if self.processor.tempo() != packet.settings.tempo {
            self.apply(Command::SetTempo(packet.settings.tempo));
        }
        if self.processor.volume() != packet.settings.volume {
            self.apply(Command::SetVolume(packet.settings.volume));
        }
        if packet.epoch != self.epoch {
            return;
        }
        match packet.action {
            Action::Update => {}
            Action::Play => self.apply(Command::Play),
            Action::Pause => {
                self.apply(Command::Pause);
                self.last_keys = 0;
                self.preview_key = 0;
                self.preview_remaining = 0;
            }
        }
    }

    fn controls(&mut self) {
        self.refresh_epoch();
        // Never drain an actively refilled queue without a per-boundary bound.
        for _ in 0..CONTROL_CAPACITY {
            let Ok(packet) = self.consumer.pop() else {
                break;
            };
            self.packet(packet);
        }
        self.refresh_epoch();
        let preview = self.shared.preview.load(Ordering::Acquire);
        if preview != 0 && preview != self.last_preview {
            self.last_preview = preview;
            let pitch = (preview & 255) as u8;
            self.preview_key = 1 << (pitch - MIN_PITCH);
            self.preview_remaining = self.processor.sample_rate() / 5;
            // Repeated auditions at the same pitch retrigger deliberately.
            if self.last_keys & self.preview_key != 0 {
                self.apply(Command::NoteOn {
                    pitch,
                    velocity: 96,
                });
            }
        }
        let keys = self.shared.held_keys.load(Ordering::Acquire) | self.preview_key;
        for pitch in MIN_PITCH..=MAX_PITCH {
            let bit = 1 << (pitch - MIN_PITCH);
            if keys & bit != 0 && self.last_keys & bit == 0 {
                self.apply(Command::NoteOn {
                    pitch,
                    velocity: 96,
                });
            }
            if keys & bit == 0 && self.last_keys & bit != 0 {
                self.apply(Command::NoteOff { pitch });
            }
        }
        self.last_keys = keys;
    }

    fn publish(&self) {
        let snapshot = self.processor.snapshot();
        self.shared
            .step_bits
            .store(snapshot.step.to_bits(), Ordering::Relaxed);
        self.shared
            .active_keys
            .store(snapshot.active_keys, Ordering::Relaxed);
        self.shared
            .frames
            .store(snapshot.frames_generated, Ordering::Relaxed);
        self.shared
            .transport_frames
            .store(snapshot.transport_sample_position, Ordering::Relaxed);
        self.shared
            .playing
            .store(snapshot.playing, Ordering::Relaxed);
    }
}

impl Iterator for Stream {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.block_remaining == 0 {
            self.controls();
            self.block_remaining = CONTROL_BLOCK;
        }
        self.block_remaining -= 1;
        if self.preview_remaining != 0 {
            self.preview_remaining -= 1;
            if self.preview_remaining == 0 {
                self.preview_key = 0;
            }
        }
        let sample = self.processor.next_sample();
        if self.block_remaining == 0 {
            self.publish();
        }
        Some(if self.shared.faulted.load(Ordering::Relaxed) {
            0.0
        } else {
            sample
        })
    }
}

#[cfg(test)]
#[path = "control_tests.rs"]
mod tests;

use std::{
    error::Error,
    fmt,
    iter::FusedIterator,
    num::{NonZeroU16, NonZeroU32},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use rodio::{
    DeviceSinkBuilder, DeviceSinkError, MixerDeviceSink, Source,
    cpal::{
        self, BufferSize, Sample as _,
        traits::{DeviceTrait, HostTrait},
    },
};

const MAX_BUFFER_FRAMES: u32 = 16_384;

/// Setup policy for one default-device output stream.
///
/// Defaults request 512 sample frames with fallback disabled. A frame contains
/// one sample per output channel; 512 frames at 48 kHz represent about 10.7 ms
/// of samples, not a measurement of end-to-end audible latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioOutputConfig {
    buffer_frames: u32,
    device_buffer_fallback: bool,
}

impl AudioOutputConfig {
    /// Requests a buffer of 1 through 16,384 sample frames without opening audio.
    ///
    /// Zero and larger values return a typed error. Devices may reject a valid
    /// request; device-default fallback remains disabled until explicitly set.
    pub const fn new(buffer_frames: u32) -> Result<Self, AudioOutputError> {
        if buffer_frames == 0 || buffer_frames > MAX_BUFFER_FRAMES {
            return Err(AudioOutputError::InvalidBufferFrames {
                requested: buffer_frames,
                maximum: MAX_BUFFER_FRAMES,
            });
        }
        Ok(Self {
            buffer_frames,
            device_buffer_fallback: false,
        })
    }

    /// Returns the requested buffer length in sample frames, not scalar samples.
    pub const fn buffer_frames(self) -> u32 {
        self.buffer_frames
    }

    /// Permits a failed fixed-buffer attempt to retry with the device default.
    ///
    /// This retries only the selected default device, with the same sample rate,
    /// channels, and sample format. It never selects another output device.
    /// The device-default buffer is not bounded by the requested frame count;
    /// [`AudioOutputInfo`] reports when this explicit fallback was used.
    pub const fn with_device_buffer_fallback(mut self, enabled: bool) -> Self {
        self.device_buffer_fallback = enabled;
        self
    }

    /// Returns whether the same-device default-buffer retry is enabled.
    pub const fn device_buffer_fallback(self) -> bool {
        self.device_buffer_fallback
    }
}

impl Default for AudioOutputConfig {
    fn default() -> Self {
        Self {
            buffer_frames: 512,
            device_buffer_fallback: false,
        }
    }
}

/// The successfully opened stream's configuration, not measured device latency.
///
/// Sample rate and channel count come from the opened backend. A fixed buffer
/// size is a backend request, not a guarantee about callback lengths or the
/// additional buffering performed by the operating system and physical device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioOutputInfo {
    sample_rate: NonZeroU32,
    channels: NonZeroU16,
    requested_buffer_frames: u32,
    configured_buffer_frames: Option<u32>,
    used_device_buffer_fallback: bool,
}

impl AudioOutputInfo {
    /// Returns frames per second for the output and its mono source.
    pub const fn sample_rate(self) -> NonZeroU32 {
        self.sample_rate
    }

    /// Returns the output's channel count; the application source remains mono.
    pub const fn channels(self) -> NonZeroU16 {
        self.channels
    }

    /// Returns the original bounded request, even when fallback was used.
    pub const fn requested_buffer_frames(self) -> u32 {
        self.requested_buffer_frames
    }

    /// Returns the configured fixed request, or None for device-default sizing.
    ///
    /// This is not the actual callback length or measured physical latency.
    pub const fn configured_buffer_frames(self) -> Option<u32> {
        self.configured_buffer_frames
    }

    /// Reports a successful retry using the same device's default buffer size.
    pub const fn used_device_buffer_fallback(self) -> bool {
        self.used_device_buffer_fallback
    }
}

#[derive(Debug, Default)]
struct OutputState {
    stream_errors: AtomicU64,
    generated_samples: AtomicU64,
    invalid_samples: AtomicU64,
    finished: AtomicBool,
    closed: AtomicBool,
}

/// Cloneable, read-only diagnostics that do not keep the output device alive.
///
/// Fields are independent atomic observations, not one transactional snapshot.
/// Counters saturate at u64::MAX. Generated samples can be buffered ahead of
/// audible output; they are not a physical playback clock.
#[derive(Debug, Clone)]
pub struct AudioOutputStatus {
    state: Arc<OutputState>,
}

impl AudioOutputStatus {
    fn new() -> Self {
        Self {
            state: Arc::new(OutputState::default()),
        }
    }

    /// Reports whether any backend stream error has occurred, including underrun.
    ///
    /// Errors are sticky diagnostics; this adapter neither reconnects nor replays
    /// the source. The application decides whether to stop or reopen output.
    pub fn is_faulted(&self) -> bool {
        self.stream_error_count() != 0
    }

    /// Returns the number of asynchronous backend error callbacks observed.
    ///
    /// Callbacks only update this counter; they do not print or retain allocating
    /// backend error messages. Setup failures remain detailed typed errors.
    pub fn stream_error_count(&self) -> u64 {
        self.state.stream_errors.load(Ordering::Relaxed)
    }

    /// Returns mono samples pulled from the source, including sanitized samples.
    ///
    /// Each sample represents one source/output frame, independently of output
    /// channel count. Silence yielded while stopped also advances this counter;
    /// an application transport must maintain its own musical position.
    pub fn generated_sample_count(&self) -> u64 {
        self.state.generated_samples.load(Ordering::Relaxed)
    }

    /// Returns samples replaced or clamped because they were invalid PCM.
    ///
    /// Non-finite values become silence; finite values outside -1.0 through 1.0
    /// are clamped. Exact endpoints and signed zero are valid samples.
    pub fn invalid_sample_count(&self) -> u64 {
        self.state.invalid_samples.load(Ordering::Relaxed)
    }

    /// Reports that the source returned None, which permanently ends that source.
    ///
    /// This does not mean buffered samples have reached the speakers. Dropping
    /// playback before source exhaustion closes output without marking finished.
    pub fn is_finished(&self) -> bool {
        self.state.finished.load(Ordering::Acquire)
    }

    /// Reports that the owning output guard has been dropped.
    ///
    /// Status clones cannot prevent closure. Already submitted device samples
    /// cannot be recalled, so closure does not promise instantaneous silence.
    pub fn is_closed(&self) -> bool {
        self.state.closed.load(Ordering::Acquire)
    }

    fn record_stream_error(&self) {
        increment(&self.state.stream_errors);
    }
}

fn increment(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        Some(value.saturating_add(1))
    });
}

#[derive(Debug)]
struct OutputLifetime {
    status: AudioOutputStatus,
}

impl Drop for OutputLifetime {
    fn drop(&mut self) {
        self.status.state.closed.store(true, Ordering::Release);
    }
}

/// An opened output device that may accept exactly one application source.
///
/// Opening immediately starts a silent backend stream. Keep this guard, or the
/// [`AudioPlayback`] returned from it, outside World preparation and renderer
/// ownership. Dropping the guard closes output. No application/window/GPU is
/// required, but the optional `audio` feature and a working device are required.
#[derive(Debug)]
#[must_use = "dropping the output guard closes its audio device"]
pub struct AudioOutput {
    // Field order ensures the backend is dropped before lifetime reports closed.
    device: MixerDeviceSink,
    info: AudioOutputInfo,
    lifetime: OutputLifetime,
}

impl AudioOutput {
    /// Opens the default device with explicit buffer policy and atomic diagnostics.
    ///
    /// Failure returns an error, never an implicitly successful silent backend.
    /// Applications may choose and display their own silent-mode fallback.
    /// Backend setup can allocate; this API does not claim recovery from every
    /// backend allocation failure. No automatic device recovery is installed.
    pub fn open(config: AudioOutputConfig) -> Result<Self, AudioOutputError> {
        let selected_device = cpal::default_host().default_output_device().ok_or(
            AudioOutputError::DeviceConfiguration(DeviceSinkError::NoDevice),
        )?;
        let format = selected_device.default_output_config().map_err(|error| {
            AudioOutputError::DeviceConfiguration(DeviceSinkError::DefaultSinkConfigError(error))
        })?;
        let ((mut device, status), used_fallback) = open_with_policy(config, |buffer_size| {
            // The backend's fallback helper may try other configurations. Make
            // only our documented same-device buffer retry instead.
            // Failed attempts have separate diagnostics, so they cannot mark a
            // successfully opened fallback stream as asynchronously faulted.
            let status = AudioOutputStatus::new();
            let callback_status = status.clone();
            let device = DeviceSinkBuilder::default()
                .with_device(selected_device.clone())
                .with_supported_config(&format)
                .with_buffer_size(buffer_size)
                .with_error_callback(move |_| callback_status.record_stream_error())
                .open_stream()?;
            Ok((device, status))
        })?;
        device.log_on_drop(false);
        let backend = device.config();
        let info = AudioOutputInfo {
            sample_rate: backend.sample_rate(),
            channels: backend.channel_count(),
            requested_buffer_frames: config.buffer_frames,
            configured_buffer_frames: match backend.buffer_size() {
                BufferSize::Fixed(frames) => Some(*frames),
                BufferSize::Default => None,
            },
            used_device_buffer_fallback: used_fallback,
        };
        Ok(Self {
            device,
            info,
            lifetime: OutputLifetime { status },
        })
    }

    /// Returns the opened output format and requested/default buffer policy.
    pub const fn info(&self) -> AudioOutputInfo {
        self.info
    }

    /// Returns independently shareable diagnostics without sharing device ownership.
    pub fn status(&self) -> AudioOutputStatus {
        self.lifetime.status.clone()
    }

    /// Transfers one mono PCM iterator to the output and returns its lifetime guard.
    ///
    /// Construct the source for [`AudioOutputInfo::sample_rate`]; output uses that
    /// same rate without resampling. Rodio duplicates mono into stereo; output
    /// channels beyond the first two receive silence.
    /// Each sample is sanitized to finite -1.0 through 1.0 PCM before conversion.
    /// The source executes on the audio thread: it must return promptly, avoid
    /// blocking, allocation, file/device calls and panics, and bound its own work.
    ///
    /// None ends the source permanently. Interactive stopped/paused sources must
    /// keep yielding zero if they need to resume. Controls and musical timing
    /// belong to the application; no pause, time-scale or World policy is implied.
    /// This consumes the output, so callers cannot enqueue unbounded sources.
    /// Backend attachment may allocate; it is setup, not a real-time operation.
    pub fn play_mono(self, source: impl Iterator<Item = f32> + Send + 'static) -> AudioPlayback {
        let source = MonoSource {
            source,
            sample_rate: self.info.sample_rate,
            status: self.status(),
            ended: false,
        };
        self.device.mixer().add(source);
        AudioPlayback { output: self }
    }
}

/// Keeps the sole attached source's output device alive until dropped.
///
/// There is no detach or second-source API. Application-specific controls belong
/// to the source; dropping this guard closes output even when status is cloned.
#[derive(Debug)]
#[must_use = "dropping playback closes its audio output"]
pub struct AudioPlayback {
    output: AudioOutput,
}

impl AudioPlayback {
    /// Returns the configured output format and buffer selection.
    pub const fn info(&self) -> AudioOutputInfo {
        self.output.info()
    }

    /// Returns shared diagnostics without extending the output lifetime.
    pub fn status(&self) -> AudioOutputStatus {
        self.output.status()
    }
}

/// A rejected output configuration or failed device-opening attempt.
#[derive(Debug)]
#[non_exhaustive]
pub enum AudioOutputError {
    /// The requested buffer is zero or exceeds the adapter's setup bound.
    InvalidBufferFrames {
        /// Rejected number of sample frames.
        requested: u32,
        /// Maximum permitted request, in sample frames.
        maximum: u32,
    },
    /// No default device or usable default format could be obtained.
    DeviceConfiguration(DeviceSinkError),
    /// The requested buffer failed, and any explicitly enabled retry also failed.
    Open {
        /// Original fixed-buffer opening failure.
        requested: DeviceSinkError,
        /// Same-device default-buffer failure, if that retry was enabled.
        fallback: Option<DeviceSinkError>,
    },
}

impl fmt::Display for AudioOutputError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidBufferFrames { requested, maximum } => write!(
                formatter,
                "audio buffer request {requested} must be between 1 and {maximum} frames"
            ),
            Self::DeviceConfiguration(error) => {
                write!(
                    formatter,
                    "default audio device configuration failed: {error}"
                )
            }
            Self::Open {
                requested,
                fallback,
            } => {
                write!(
                    formatter,
                    "requested audio buffer could not open: {requested}"
                )?;
                if let Some(fallback) = fallback {
                    write!(
                        formatter,
                        "; device-default buffer retry also failed: {fallback}"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl Error for AudioOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidBufferFrames { .. } => None,
            Self::DeviceConfiguration(error) => Some(error),
            Self::Open {
                requested,
                fallback,
            } => Some(fallback.as_ref().unwrap_or(requested)),
        }
    }
}

fn open_with_policy<T>(
    config: AudioOutputConfig,
    mut open: impl FnMut(BufferSize) -> Result<T, DeviceSinkError>,
) -> Result<(T, bool), AudioOutputError> {
    match open(BufferSize::Fixed(config.buffer_frames)) {
        Ok(output) => Ok((output, false)),
        Err(requested) => {
            if !config.device_buffer_fallback {
                return Err(AudioOutputError::Open {
                    requested,
                    fallback: None,
                });
            }
            match open(BufferSize::Default) {
                Ok(output) => Ok((output, true)),
                Err(fallback) => Err(AudioOutputError::Open {
                    requested,
                    fallback: Some(fallback),
                }),
            }
        }
    }
}

struct MonoSource<I> {
    source: I,
    sample_rate: NonZeroU32,
    status: AudioOutputStatus,
    ended: bool,
}

impl<I: Iterator<Item = f32>> Iterator for MonoSource<I> {
    type Item = rodio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        let Some(sample) = self.source.next() else {
            self.ended = true;
            self.status.state.finished.store(true, Ordering::Release);
            return None;
        };
        increment(&self.status.state.generated_samples);
        let sanitized = if !sample.is_finite() {
            increment(&self.status.state.invalid_samples);
            0.0
        } else if !(-1.0..=1.0).contains(&sample) {
            increment(&self.status.state.invalid_samples);
            sample.clamp(-1.0, 1.0)
        } else {
            sample
        };
        // Rodio's additive `64bit` feature may be enabled by another dependency.
        // Keep the public mono input f32 while honoring the backend sample type.
        Some(rodio::Sample::from_sample(sanitized))
    }
}

impl<I: Iterator<Item = f32>> FusedIterator for MonoSource<I> {}

impl<I: Iterator<Item = f32>> Source for MonoSource<I> {
    fn current_span_len(&self) -> Option<usize> {
        self.ended.then_some(0)
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::MIN
    }

    fn sample_rate(&self) -> NonZeroU32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;

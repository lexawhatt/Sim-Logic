use std::{cell::Cell, iter, sync::atomic::AtomicU64};

use super::*;

fn source<I: Iterator<Item = f32>>(samples: I, status: &AudioOutputStatus) -> MonoSource<I> {
    MonoSource {
        source: samples,
        sample_rate: NonZeroU32::new(48_000).unwrap(),
        status: status.clone(),
        ended: false,
    }
}

#[test]
fn buffer_request_bounds_and_fallback_are_explicit() {
    let default = AudioOutputConfig::default();
    assert_eq!(default.buffer_frames(), 512);
    assert!(!default.device_buffer_fallback());
    for frames in [1, 256, 512, MAX_BUFFER_FRAMES] {
        let config = AudioOutputConfig::new(frames).unwrap();
        assert_eq!(config.buffer_frames(), frames);
        assert!(!config.device_buffer_fallback());
        let enabled = config.with_device_buffer_fallback(true);
        assert!(enabled.device_buffer_fallback());
        assert_eq!(enabled.buffer_frames(), frames);
        assert_eq!(enabled.with_device_buffer_fallback(false), config);
    }
    for requested in [0, MAX_BUFFER_FRAMES + 1, u32::MAX] {
        assert!(matches!(AudioOutputConfig::new(requested),
            Err(AudioOutputError::InvalidBufferFrames { requested: actual, maximum })
            if actual == requested && maximum == MAX_BUFFER_FRAMES));
    }
}

#[test]
fn successful_fixed_open_never_tries_the_optional_fallback() {
    for enabled in [false, true] {
        let mut requests = Vec::new();
        let result = open_with_policy(
            AudioOutputConfig::default().with_device_buffer_fallback(enabled),
            |buffer| {
                requests.push(buffer);
                Ok(7)
            },
        )
        .unwrap();
        assert_eq!(result, (7, false));
        assert_eq!(requests, [BufferSize::Fixed(512)]);
    }
}

#[test]
fn disabled_fallback_returns_the_first_error_without_another_attempt() {
    let mut requests = Vec::new();
    let result = open_with_policy::<()>(AudioOutputConfig::default(), |buffer| {
        requests.push(buffer);
        Err(DeviceSinkError::NoDevice)
    });
    assert!(matches!(
        result,
        Err(AudioOutputError::Open {
            requested: DeviceSinkError::NoDevice,
            fallback: None,
        })
    ));
    assert_eq!(requests, [BufferSize::Fixed(512)]);
}

#[test]
fn enabled_fallback_reports_success_or_retains_both_errors() {
    for succeeds in [false, true] {
        let mut requests = Vec::new();
        let result = open_with_policy(
            AudioOutputConfig::default().with_device_buffer_fallback(true),
            |buffer| {
                requests.push(buffer);
                match buffer {
                    BufferSize::Fixed(_) => Err(DeviceSinkError::NoDevice),
                    BufferSize::Default if succeeds => Ok(9),
                    BufferSize::Default => Err(DeviceSinkError::UnsupportedSampleFormat),
                }
            },
        );
        assert_eq!(requests, [BufferSize::Fixed(512), BufferSize::Default]);
        if succeeds {
            assert_eq!(result.unwrap(), (9, true));
        } else {
            let error = result.unwrap_err();
            assert!(error.source().is_some());
            assert!(error.to_string().contains("retry also failed"));
            assert!(matches!(
                error,
                AudioOutputError::Open {
                    requested: DeviceSinkError::NoDevice,
                    fallback: Some(DeviceSinkError::UnsupportedSampleFormat),
                }
            ));
        }
    }
}

#[test]
fn nonfinite_pcm_is_silenced_and_out_of_range_pcm_is_clamped() {
    let status = AudioOutputStatus::new();
    let mut samples = source(
        [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            2.0,
            -3.0,
            1.0,
            -1.0,
            -0.0,
            0.25,
        ]
        .into_iter(),
        &status,
    );
    assert_eq!(samples.channels(), NonZeroU16::MIN);
    assert_eq!(samples.sample_rate().get(), 48_000);
    assert_eq!(samples.current_span_len(), None);
    assert_eq!(samples.total_duration(), None);
    let values: Vec<_> = samples.by_ref().collect();
    assert_eq!(values, [0.0, 0.0, 0.0, 1.0, -1.0, 1.0, -1.0, -0.0, 0.25]);
    assert_eq!(
        values[7].to_bits(),
        rodio::Sample::from_sample(-0.0_f32).to_bits()
    );
    assert_eq!(status.generated_sample_count(), 9);
    assert_eq!(status.invalid_sample_count(), 5);
    assert!(status.is_finished());
    assert!(!status.is_closed());
    assert!(!status.is_faulted());
    assert_eq!(samples.current_span_len(), Some(0));
    assert_eq!(samples.next(), None);
    assert_eq!(status.generated_sample_count(), 9);
}

#[test]
fn first_none_fuses_even_a_source_that_would_resume() {
    let status = AudioOutputStatus::new();
    let calls = Cell::new(0);
    let input = iter::from_fn(|| {
        calls.set(calls.get() + 1);
        if calls.get() == 2 { None } else { Some(0.5) }
    });
    let mut samples = source(input, &status);
    assert_eq!(samples.next(), Some(0.5));
    assert_eq!(samples.next(), None);
    for _ in 0..10 {
        assert_eq!(samples.next(), None);
    }
    assert_eq!(calls.get(), 2);
    assert_eq!(status.generated_sample_count(), 1);
    assert!(status.is_finished());
}

#[test]
fn stopped_silence_remains_live_and_can_resume_without_a_second_source() {
    let status = AudioOutputStatus::new();
    let playing = Cell::new(false);
    let input = iter::from_fn(|| Some(if playing.get() { 0.25 } else { 0.0 }));
    let mut samples = source(input, &status);
    for _ in 0..100 {
        assert_eq!(samples.next(), Some(0.0));
    }
    assert!(!status.is_finished());
    playing.set(true);
    assert_eq!(samples.next(), Some(0.25));
    playing.set(false);
    assert_eq!(samples.next(), Some(0.0));
    assert_eq!(status.generated_sample_count(), 102);
    assert_eq!(status.invalid_sample_count(), 0);
}

#[test]
fn mono_attachment_does_not_preconsume_or_count_again_for_output_channels() {
    for channels in [1, 2, 4] {
        let status = AudioOutputStatus::new();
        let samples = source([0.25, 0.5, 0.75].into_iter(), &status);
        let (mixer, mut output) =
            rodio::mixer::mixer(NonZeroU16::new(channels).unwrap(), samples.sample_rate());
        mixer.add(samples);
        assert_eq!(status.generated_sample_count(), 0);
        for (frame, value) in [0.25, 0.5, 0.75].into_iter().enumerate() {
            for channel in 0..channels {
                assert_eq!(output.next(), Some(if channel < 2 { value } else { 0.0 }));
                assert_eq!(status.generated_sample_count(), frame as u64 + 1);
            }
        }
        assert_eq!(output.next(), None);
        assert!(status.is_finished());
        assert_eq!(status.generated_sample_count(), 3);
    }
}

#[test]
fn status_outlives_owner_and_faults_do_not_imply_source_exhaustion() {
    let status = AudioOutputStatus::new();
    let observer = status.clone();
    let lifetime = OutputLifetime { status };
    observer.record_stream_error();
    observer.record_stream_error();
    assert!(observer.is_faulted());
    assert_eq!(observer.stream_error_count(), 2);
    assert!(!observer.is_closed());
    assert!(!observer.is_finished());
    drop(lifetime);
    assert!(observer.is_closed());
    assert!(!observer.is_finished());
    assert_eq!(observer.stream_error_count(), 2);
}

#[test]
fn diagnostics_can_be_observed_across_threads_and_counters_saturate() {
    let status = AudioOutputStatus::new();
    let writer = status.clone();
    std::thread::spawn(move || {
        let mut samples = source(iter::repeat_n(0.25, 100), &writer);
        assert_eq!(samples.by_ref().count(), 100);
        writer.record_stream_error();
    })
    .join()
    .unwrap();
    assert_eq!(status.generated_sample_count(), 100);
    assert_eq!(status.stream_error_count(), 1);
    assert!(status.is_finished());
    let counter = AtomicU64::new(u64::MAX - 1);
    increment(&counter);
    increment(&counter);
    assert_eq!(counter.load(Ordering::Relaxed), u64::MAX);
}

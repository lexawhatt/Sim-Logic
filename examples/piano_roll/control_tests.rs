use super::*;

// Check the desktop-only hookup's public signature without opening a device.
#[cfg(feature = "audio")]
const _: fn(&mut Control, sim_logic::audio::AudioOutputStatus) = Control::set_output_status;

#[test]
fn a_new_key_or_preview_after_stop_is_not_lost() -> Result<(), MusicError> {
    for preview in [false, true] {
        let (mut control, mut stream) = channel(48_000)?;
        control.stop();
        if preview {
            control.preview(MIN_PITCH);
        } else {
            control.set_held_keys(1);
        }
        for _ in 0..128 {
            let _ = stream.next();
        }
        assert_eq!(control.snapshot().active_keys, 1);
    }
    Ok(())
}

#[test]
fn stop_preserves_accepted_mute_and_future_epoch_play() -> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    let settings = Settings {
        volume: 0.0,
        ..Settings::default()
    };
    assert!(control.submit(settings, Action::Update));
    control.stop();
    for _ in 0..128 {
        let _ = stream.next();
    }
    assert_eq!(stream.processor.volume(), 0.0);
    control.set_held_keys(1);
    assert!(stream.by_ref().take(1024).all(|sample| sample == 0.0));
    // Emulate a stop arriving between the initial boundary read and a queue
    // read: the source's cached epoch is deliberately still the old one.
    stream.refresh_epoch();
    control.stop();
    assert!(control.submit(settings, Action::Play));
    let packet = stream.consumer.pop().expect("accepted Play packet");
    assert_ne!(stream.epoch, packet.epoch);
    stream.packet(packet);
    assert!(stream.processor.snapshot().playing);
    Ok(())
}

#[test]
fn full_queue_rejects_edits_but_stop_cancels_queued_playback() -> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    for _ in 0..CONTROL_CAPACITY {
        assert!(control.submit(Settings::default(), Action::Play));
    }
    assert!(!control.submit(Settings::default(), Action::Play));
    control.stop();
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(0.0));
    }
    assert!(!control.snapshot().playing);
    assert_eq!(control.snapshot().step, 0.0);
    assert!(control.submit(Settings::default(), Action::Play));
    assert!(
        stream
            .by_ref()
            .take(1024)
            .any(|sample| sample.abs() > 0.001)
    );
    Ok(())
}

#[test]
fn releasing_a_key_does_not_need_queue_capacity() -> Result<(), MusicError> {
    let (mut control, mut stream) = channel(44_100)?;
    control.set_held_keys(1);
    for _ in 0..128 {
        let _ = stream.next();
    }
    assert_eq!(control.snapshot().active_keys, 1);
    for _ in 0..CONTROL_CAPACITY {
        assert!(control.submit(Settings::default(), Action::Update));
    }
    control.set_held_keys(0);
    for _ in 0..256 {
        let _ = stream.next();
    }
    assert_eq!(control.snapshot().active_keys, 0);
    Ok(())
}

#[test]
fn stopped_source_remains_alive_and_can_restart() -> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(0.0));
    }
    assert!(control.submit(Settings::default(), Action::Play));
    let samples: Vec<_> = stream.by_ref().take(4096).collect();
    assert!(samples.iter().any(|sample| sample.abs() > 0.001));
    control.stop();
    for _ in 0..128 {
        let _ = stream.next();
    }
    assert!(!control.snapshot().playing);
    assert_eq!(control.snapshot().active_keys, 0);
    assert_eq!(stream.next(), Some(0.0));
    Ok(())
}

#[test]
fn full_queue_stop_keeps_latest_accepted_settings_but_not_a_rejected_edit() -> Result<(), MusicError>
{
    let (mut control, mut stream) = channel(48_000)?;
    let mut accepted = Settings::default();
    for index in 0..CONTROL_CAPACITY {
        let mut sequence = Sequence::new();
        sequence.add(super::super::music::Note::new(
            MIN_PITCH + index as u8,
            0,
            2,
            96,
        )?)?;
        accepted = Settings {
            sequence,
            tempo: 60 + index as u16,
            volume: index as f32 / 64.0,
        };
        assert!(control.submit(accepted, Action::Play));
    }
    let rejected = Settings {
        sequence: Sequence::new(),
        tempo: 180,
        volume: 1.0,
    };
    assert!(!control.submit(rejected, Action::Update));
    assert_eq!(control.rejected, 1);
    control.stop();
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(0.0));
    }
    assert_eq!(stream.processor.sequence(), &accepted.sequence);
    assert_eq!(stream.processor.tempo(), accepted.tempo);
    assert_eq!(stream.processor.volume(), accepted.volume);
    assert!(!control.snapshot().playing);
    assert_eq!(control.snapshot().active_keys, 0);
    Ok(())
}

#[test]
fn stop_between_boundary_read_and_each_packet_preserves_only_current_transport_actions()
-> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    let first = Settings {
        tempo: 75,
        ..Settings::default()
    };
    let current = Settings {
        tempo: 150,
        ..Settings::default()
    };
    assert!(control.submit(first, Action::Pause));
    stream.refresh_epoch();
    control.stop();
    assert!(control.submit(current, Action::Play));
    // Deterministic interleaving: the boundary initially observed the old
    // epoch, but the queued new Play was published after the newer Stop.
    let old_packet = stream.consumer.pop().unwrap();
    stream.packet(old_packet);
    assert_eq!(stream.processor.tempo(), first.tempo);
    assert!(!stream.processor.snapshot().playing);
    let current_packet = stream.consumer.pop().unwrap();
    stream.packet(current_packet);
    assert_eq!(stream.processor.tempo(), current.tempo);
    assert!(stream.processor.snapshot().playing);

    assert!(control.submit(first, Action::Play));
    let popped_before_stop = stream.consumer.pop().unwrap();
    control.stop();
    stream.packet(popped_before_stop);
    assert_eq!(stream.processor.tempo(), first.tempo);
    assert!(!stream.processor.snapshot().playing);
    assert_eq!(stream.processor.snapshot().transport_sample_position, 0);
    Ok(())
}

#[test]
fn stop_cancels_old_pending_audition_but_accepts_the_next_same_pitch() -> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    control.preview(MIN_PITCH);
    control.set_held_keys(1);
    control.stop();
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(0.0));
    }
    assert_eq!(control.snapshot().active_keys, 0);
    assert_eq!(stream.preview_key, 0);
    control.preview(MIN_PITCH);
    for _ in 0..128 {
        let _ = stream.next();
    }
    assert_eq!(control.snapshot().active_keys, 1);
    assert!(
        stream
            .by_ref()
            .take(1024)
            .any(|sample| sample.abs() > 0.001)
    );
    Ok(())
}

#[test]
fn preview_releases_at_the_next_control_boundary_without_ending_a_held_key()
-> Result<(), MusicError> {
    for rate in [8_000, 44_100, 48_000, 192_000] {
        let (mut control, mut stream) = channel(rate)?;
        control.set_held_keys(1);
        control.preview(MIN_PITCH);
        for _ in 0..rate / 5 + CONTROL_BLOCK * 2 {
            let _ = stream.next();
        }
        assert_eq!(stream.preview_key, 0);
        assert_eq!(control.snapshot().active_keys, 1);
        for _ in 0..CONTROL_CAPACITY {
            assert!(control.submit(Settings::default(), Action::Update));
        }
        control.set_held_keys(0);
        for _ in 0..CONTROL_BLOCK * 2 {
            let _ = stream.next();
        }
        assert_eq!(control.snapshot().active_keys, 0);
        assert!(!control.is_faulted());
    }
    Ok(())
}

#[test]
fn repeated_preview_uses_one_retrigger_and_latest_pitch_wins_before_the_boundary()
-> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    let mut expected = Processor::with_sample_rate(48_000)?;
    let settings = Settings::default();
    expected.apply(Command::ReplaceSequence(settings.sequence))?;
    expected.apply(Command::SetTempo(settings.tempo))?;
    expected.apply(Command::SetVolume(settings.volume))?;
    control.preview(MIN_PITCH + 1);
    control.preview(MIN_PITCH);
    expected.apply(Command::NoteOn {
        pitch: MIN_PITCH,
        velocity: 96,
    })?;
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(expected.next_sample()));
    }
    assert_eq!(control.snapshot().active_keys, 1);
    control.preview(MIN_PITCH);
    expected.apply(Command::NoteOn {
        pitch: MIN_PITCH,
        velocity: 96,
    })?;
    for _ in 0..128 {
        assert_eq!(stream.next(), Some(expected.next_sample()));
    }
    assert_eq!(control.snapshot().active_keys, 1);
    Ok(())
}

#[test]
fn abandoned_source_rejects_settings_and_invalid_masks_cannot_create_out_of_range_keys()
-> Result<(), MusicError> {
    let (mut control, mut stream) = channel(48_000)?;
    control.set_held_keys(u64::MAX);
    for _ in 0..128 {
        let _ = stream.next();
    }
    let valid = (1u64 << (MAX_PITCH - MIN_PITCH + 1)) - 1;
    assert_eq!(control.shared.held_keys.load(Ordering::Relaxed), valid);
    assert_eq!(control.snapshot().active_keys & !valid, 0);
    control.set_held_keys(0);
    for _ in 0..128 {
        let _ = stream.next();
    }
    assert_eq!(control.snapshot().active_keys, 0);
    drop(stream);
    assert!(!control.submit(Settings::default(), Action::Play));
    assert_eq!(control.rejected, 1);
    Ok(())
}

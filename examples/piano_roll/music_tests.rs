use super::*;

fn note(pitch: u8, start: u8, duration: u8) -> Note {
    Note::new(pitch, start, duration, 110).unwrap()
}

fn score(notes: &[Note]) -> Sequence {
    let mut sequence = Sequence::new();
    for &note in notes {
        sequence.add(note).unwrap();
    }
    sequence
}

fn playing(sequence: Sequence, rate: u32, tempo: u16) -> Processor {
    let mut processor = Processor::with_sample_rate(rate).unwrap();
    processor.apply(Command::ReplaceSequence(sequence)).unwrap();
    processor.apply(Command::SetTempo(tempo)).unwrap();
    processor.apply(Command::Play).unwrap();
    processor
}

fn render(processor: &mut Processor, count: u64) {
    for _ in 0..count {
        let sample = processor.next_sample();
        assert!(sample.is_finite() && (-1.0..=1.0).contains(&sample));
    }
}

fn key(pitch: u8) -> u64 {
    1_u64 << (pitch - MIN_PITCH)
}

fn step_frame(step: u64, rate: u32, tempo: u16) -> u64 {
    (step * u64::from(rate) * 60).div_ceil(u64::from(tempo) * 4)
}

#[test]
fn model_and_commands_are_copyable_and_notes_validate_boundaries() {
    fn assert_copy<T: Copy>() {}
    assert_copy::<Note>();
    assert_copy::<Sequence>();
    assert_copy::<Command>();
    assert_copy::<TransportSnapshot>();
    let lowest = Note::new(48, 0, 32, 1).unwrap();
    let highest = Note::new(83, 31, 1, 127).unwrap();
    assert_eq!(
        (lowest.pitch(), lowest.start(), lowest.duration()),
        (48, 0, 32)
    );
    assert_eq!((highest.pitch(), highest.velocity()), (83, 127));
    assert_eq!(Note::new(47, 0, 1, 1), Err(MusicError::InvalidPitch(47)));
    assert_eq!(Note::new(84, 0, 1, 1), Err(MusicError::InvalidPitch(84)));
    assert_eq!(Note::new(60, 32, 1, 1), Err(MusicError::InvalidStart(32)));
    assert_eq!(Note::new(60, 0, 1, 0), Err(MusicError::InvalidVelocity(0)));
    assert_eq!(
        Note::new(60, 0, 1, 128),
        Err(MusicError::InvalidVelocity(128))
    );
    for (start, duration) in [(0, 0), (31, 2), (1, 255)] {
        assert_eq!(
            Note::new(60, start, duration, 64),
            Err(MusicError::InvalidDuration { start, duration })
        );
    }
}

#[test]
fn score_edits_are_atomic_and_reuse_vacant_stable_slots() {
    let mut sequence = Sequence::new();
    for index in 0..MAX_NOTES {
        assert_eq!(sequence.add(note(60, 0, 2)), Ok(index));
    }
    let full = sequence;
    assert_eq!(sequence.add(note(64, 0, 2)), Err(MusicError::FullSequence));
    assert_eq!(sequence, full);
    assert_eq!(
        sequence.resize(0, 33),
        Err(MusicError::InvalidDuration {
            start: 0,
            duration: 33
        })
    );
    assert_eq!(
        sequence.resize(64, 1),
        Err(MusicError::InvalidNoteIndex(64))
    );
    assert_eq!(sequence.remove(64), Err(MusicError::InvalidNoteIndex(64)));
    assert_eq!(sequence, full);
    assert_eq!(sequence.remove(17), Ok(note(60, 0, 2)));
    assert_eq!(sequence.remove(17), Err(MusicError::MissingNote(17)));
    assert_eq!(sequence.resize(17, 1), Err(MusicError::MissingNote(17)));
    assert_eq!(sequence.add(note(67, 8, 8)), Ok(17));
    sequence.resize(17, 24).unwrap();
    assert_eq!(sequence.notes()[17], Some(note(67, 8, 24)));
    assert_eq!(sequence.notes()[18], Some(note(60, 0, 2)));
}

#[test]
fn defaults_and_demo_score_are_explicit() {
    let processor = Processor::default();
    assert_eq!(processor.sample_rate(), 48_000);
    assert_eq!(processor.tempo(), 120);
    assert_eq!(processor.volume(), 0.75);
    assert_eq!(*processor.sequence(), Sequence::default());
    assert_eq!(
        processor.snapshot(),
        TransportSnapshot {
            playing: false,
            step: 0.0,
            active_keys: 0,
            frames_generated: 0,
            transport_sample_position: 0,
        }
    );
    let demo = Sequence::demo();
    assert_eq!(demo.notes().iter().flatten().count(), 12);
    for note in demo.notes().iter().flatten() {
        assert_eq!(
            Note::new(note.pitch(), note.start(), note.duration(), note.velocity()),
            Ok(*note)
        );
    }
}

#[test]
fn sample_rate_bounds_are_checked_without_opening_a_device() {
    for rate in [8_000, 44_100, 48_000, 192_000] {
        assert_eq!(
            Processor::with_sample_rate(rate).unwrap().sample_rate(),
            rate
        );
    }
    for rate in [0, 7_999, 192_001, u32::MAX] {
        assert!(
            matches!(Processor::with_sample_rate(rate), Err(MusicError::InvalidSampleRate(value)) if value == rate)
        );
    }
}

#[test]
fn tuning_uses_equal_temperament_with_the_octave_in_the_exponent() {
    assert_eq!(pitch_frequency(69), Ok(440.0));
    assert_eq!(pitch_frequency(81), Ok(880.0));
    assert_eq!(pitch_frequency(57), Ok(220.0));
    assert!((pitch_frequency(48).unwrap() - 130.812_782_650_299_3).abs() < 1.0e-10);
    assert!((pitch_frequency(70).unwrap() / 440.0 - 2.0_f64.powf(1.0 / 12.0)).abs() < 1.0e-12);
    assert_eq!(pitch_frequency(47), Err(MusicError::InvalidPitch(47)));
}

#[test]
fn generated_waveform_has_the_requested_pitch_at_both_common_rates() {
    for rate in [44_100, 48_000] {
        for pitch in [57, 69, 81] {
            let mut processor = Processor::with_sample_rate(rate).unwrap();
            processor
                .apply(Command::NoteOn {
                    pitch,
                    velocity: 127,
                })
                .unwrap();
            render(&mut processor, u64::from(rate / 20));
            let mut previous = processor.next_sample();
            let mut first_crossing = None;
            let mut last_crossing = 0.0;
            let mut crossings = 0;
            for frame in 1..rate / 2 {
                let sample = processor.next_sample();
                if previous <= 0.0 && sample > 0.0 {
                    let fraction = f64::from(-previous) / f64::from(sample - previous);
                    let crossing = f64::from(frame - 1) + fraction;
                    first_crossing.get_or_insert(crossing);
                    last_crossing = crossing;
                    crossings += 1;
                }
                previous = sample;
            }
            let measured = f64::from(rate) * f64::from(crossings - 1)
                / (last_crossing - first_crossing.unwrap());
            let expected = pitch_frequency(pitch).unwrap();
            assert!(
                (measured - expected).abs() < 0.05,
                "rate={rate} pitch={pitch} measured={measured}"
            );
        }
    }
}

#[test]
fn note_start_and_end_are_exact_sample_boundaries() {
    for rate in [44_100, 48_000] {
        let mut processor = playing(score(&[note(60, 2, 3), note(64, 5, 1)]), rate, 120);
        let start = step_frame(2, rate, 120);
        for _ in 0..start - 1 {
            assert_eq!(processor.next_sample(), 0.0);
        }
        assert_eq!(processor.snapshot().active_keys, 0);
        processor.next_sample();
        assert_eq!(processor.snapshot().frames_generated, start);
        assert_eq!(processor.snapshot().active_keys, key(60));
        assert_eq!(
            processor
                .voices
                .iter()
                .filter(|voice| voice.owner != Owner::Silent)
                .count(),
            1
        );
        let end = step_frame(5, rate, 120);
        render(&mut processor, end - start - 1);
        assert_eq!(processor.snapshot().active_keys, key(60));
        processor.next_sample();
        assert_eq!(processor.snapshot().active_keys, key(64));
        assert!(
            processor
                .voices
                .iter()
                .any(|voice| voice.pitch == 60 && voice.released)
        );
        assert_eq!(processor.next_voice_serial, 2);
    }
}

#[test]
fn odd_tempo_keeps_integer_remainders_across_steps_and_loops() {
    for rate in [44_100, 48_000] {
        let tempo = 137;
        let mut processor = playing(score(&[note(60, 0, 32)]), rate, tempo);
        let mut rendered = 0;
        let mut loop_start = 0;
        for boundary in 1..=u64::from(STEP_COUNT) * 3 {
            let at_frame = step_frame(boundary, rate, tempo);
            render(&mut processor, at_frame - rendered - 1);
            assert_eq!(
                u64::from(processor.step),
                (boundary - 1) % u64::from(STEP_COUNT)
            );
            processor.next_sample();
            rendered = at_frame;
            if boundary.is_multiple_of(u64::from(STEP_COUNT)) {
                loop_start = at_frame;
            }
            assert_eq!(u64::from(processor.step), boundary % u64::from(STEP_COUNT));
            assert_eq!(
                u64::from(processor.phase),
                (at_frame * u64::from(tempo) * 4) % (u64::from(rate) * 60)
            );
            assert_eq!(
                processor.snapshot().transport_sample_position,
                at_frame - loop_start
            );
            assert_eq!(processor.snapshot().frames_generated, at_frame);
            assert_eq!(
                processor.next_voice_serial,
                1 + boundary / u64::from(STEP_COUNT)
            );
            assert_eq!(processor.snapshot().active_keys, key(60));
        }
    }
}

#[test]
fn pause_freezes_transport_and_play_resumes_without_double_triggering() {
    let mut processor = playing(score(&[note(60, 0, 8)]), SAMPLE_RATE, 120);
    render(&mut processor, 7_777);
    let before = processor.snapshot();
    processor.apply(Command::Pause).unwrap();
    for _ in 0..1_000 {
        assert_eq!(processor.next_sample(), 0.0);
    }
    let paused = processor.snapshot();
    assert!(!paused.playing);
    assert_eq!(paused.active_keys, 0);
    assert_eq!(paused.step, before.step);
    assert_eq!(
        paused.transport_sample_position,
        before.transport_sample_position
    );
    assert_eq!(paused.frames_generated, before.frames_generated + 1_000);
    processor.apply(Command::Play).unwrap();
    assert_eq!(processor.snapshot().active_keys, key(60));
    let serial = processor.next_voice_serial;
    processor.apply(Command::Play).unwrap();
    assert_eq!(processor.next_voice_serial, serial);
    render(&mut processor, 1);
    assert_eq!(
        processor.snapshot().transport_sample_position,
        before.transport_sample_position + 1
    );
}

#[test]
fn stop_is_immediate_silence_and_rewinds_without_erasing_the_score() {
    let sequence = score(&[note(60, 0, 8)]);
    let mut processor = playing(sequence, SAMPLE_RATE, 120);
    processor
        .apply(Command::NoteOn {
            pitch: 69,
            velocity: 127,
        })
        .unwrap();
    render(&mut processor, 8_000);
    processor.apply(Command::Stop).unwrap();
    for _ in 0..10_000 {
        assert_eq!(processor.next_sample(), 0.0);
    }
    assert_eq!(processor.snapshot().step, 0.0);
    assert_eq!(processor.phase, 0);
    assert_eq!(processor.snapshot().active_keys, 0);
    assert_eq!(processor.snapshot().transport_sample_position, 0);
    assert_eq!(processor.snapshot().frames_generated, 18_000);
    assert_eq!(*processor.sequence(), sequence);
    processor.apply(Command::Play).unwrap();
    assert_eq!(processor.snapshot().active_keys, key(60));
    assert!((0..100).any(|_| processor.next_sample() != 0.0));
}

#[test]
fn score_replacement_removes_stale_voices_but_keeps_live_keys_and_position() {
    let mut sequence = score(&[note(60, 0, 8)]);
    let mut processor = playing(sequence, SAMPLE_RATE, 120);
    processor
        .apply(Command::NoteOn {
            pitch: 69,
            velocity: 100,
        })
        .unwrap();
    render(&mut processor, 2_048);
    let before = processor.snapshot();
    let live_serial = processor
        .voices
        .iter()
        .find(|voice| voice.owner == Owner::Live)
        .unwrap()
        .serial;
    sequence.remove(0).unwrap();
    sequence.add(note(64, 0, 8)).unwrap();
    processor.apply(Command::ReplaceSequence(sequence)).unwrap();
    assert_eq!(processor.snapshot().step, before.step);
    assert_eq!(
        processor.snapshot().transport_sample_position,
        before.transport_sample_position
    );
    assert_eq!(processor.snapshot().active_keys, key(64) | key(69));
    assert!(
        !processor
            .voices
            .iter()
            .any(|voice| voice.owner != Owner::Silent && voice.pitch == 60)
    );
    let live = processor
        .voices
        .iter()
        .find(|voice| voice.owner == Owner::Live)
        .unwrap();
    assert_eq!(live.serial, live_serial);
    assert_eq!(live.age, 2_048);
}

#[test]
fn resizing_notes_across_the_playhead_reconciles_their_gate() {
    let mut sequence = score(&[note(60, 0, 8)]);
    let mut processor = playing(sequence, SAMPLE_RATE, 120);
    render(&mut processor, 24_000);
    assert_eq!(processor.snapshot().step, 4.0);
    sequence.resize(0, 2).unwrap();
    processor.apply(Command::ReplaceSequence(sequence)).unwrap();
    assert_eq!(processor.snapshot().active_keys, 0);
    assert_eq!(processor.next_sample(), 0.0);
    sequence.resize(0, 8).unwrap();
    processor.apply(Command::ReplaceSequence(sequence)).unwrap();
    assert_eq!(processor.snapshot().active_keys, key(60));
    assert_eq!(processor.snapshot().transport_sample_position, 24_001);
}

#[test]
fn live_note_off_does_not_release_a_matching_sequenced_note() {
    let mut processor = playing(score(&[note(60, 0, 32)]), SAMPLE_RATE, 120);
    processor
        .apply(Command::NoteOn {
            pitch: 60,
            velocity: 127,
        })
        .unwrap();
    render(&mut processor, 1_000);
    processor.apply(Command::NoteOff { pitch: 60 }).unwrap();
    render(&mut processor, 100);
    processor.apply(Command::NoteOff { pitch: 60 }).unwrap();
    let live = processor
        .voices
        .iter()
        .find(|voice| voice.owner == Owner::Live)
        .unwrap();
    assert!(live.released);
    assert_eq!(live.release_remaining, SAMPLE_RATE * 14 / 100 - 100);
    assert_eq!(processor.snapshot().active_keys, key(60));
    render(&mut processor, u64::from(SAMPLE_RATE * 14 / 100 - 100));
    assert!(
        !processor
            .voices
            .iter()
            .any(|voice| voice.owner == Owner::Live)
    );
    assert_eq!(processor.snapshot().active_keys, key(60));
}

#[test]
fn live_retrigger_and_release_work_while_transport_is_stopped() {
    let mut processor = Processor::new();
    for _ in 0..4 {
        processor
            .apply(Command::NoteOn {
                pitch: 69,
                velocity: 127,
            })
            .unwrap();
        render(&mut processor, 100);
        assert_eq!(
            processor
                .voices
                .iter()
                .filter(|voice| voice.owner == Owner::Live)
                .count(),
            1
        );
        assert_eq!(processor.snapshot().active_keys, key(69));
    }
    assert_eq!(processor.snapshot().transport_sample_position, 0);
    processor.apply(Command::NoteOff { pitch: 69 }).unwrap();
    assert_eq!(processor.snapshot().active_keys, 0);
    render(&mut processor, u64::from(SAMPLE_RATE * 14 / 100));
    assert_eq!(processor.next_sample(), 0.0);
}

#[test]
fn invalid_commands_preserve_the_transport_score_and_voice_waveform() {
    let sequence = score(&[note(60, 0, 8)]);
    let mut actual = playing(sequence, SAMPLE_RATE, 120);
    let mut expected = playing(sequence, SAMPLE_RATE, 120);
    for command in [
        Command::SetTempo(0),
        Command::SetTempo(59),
        Command::SetTempo(181),
        Command::SetVolume(-0.1),
        Command::SetVolume(1.1),
        Command::SetVolume(f32::NAN),
        Command::SetVolume(f32::INFINITY),
        Command::NoteOn {
            pitch: 0,
            velocity: 64,
        },
        Command::NoteOn {
            pitch: 60,
            velocity: 0,
        },
        Command::NoteOff { pitch: 255 },
    ] {
        assert!(actual.apply(command).is_err());
        assert_eq!(actual.snapshot(), expected.snapshot());
        assert_eq!(actual.tempo(), expected.tempo());
        assert_eq!(actual.volume(), expected.volume());
        assert_eq!(actual.sequence(), expected.sequence());
        for _ in 0..500 {
            assert_eq!(
                actual.next_sample().to_bits(),
                expected.next_sample().to_bits()
            );
        }
    }
}

#[test]
fn tempo_changes_preserve_fractional_position_without_retriggering() {
    let mut processor = playing(score(&[note(60, 0, 8)]), SAMPLE_RATE, 120);
    render(&mut processor, 3_000);
    assert_eq!(processor.snapshot().step, 0.5);
    let before = processor.snapshot();
    let serial = processor.next_voice_serial;
    processor.apply(Command::SetTempo(60)).unwrap();
    assert_eq!(processor.snapshot(), before);
    render(&mut processor, 6_000);
    assert_eq!(processor.snapshot().step, 1.0);
    assert_eq!(processor.next_voice_serial, serial);
    processor.apply(Command::SetTempo(180)).unwrap();
    render(&mut processor, 4_000);
    assert_eq!(processor.snapshot().step, 2.0);
}

#[test]
fn voice_capacity_is_fixed_and_oldest_voice_stealing_is_deterministic() {
    let mut sequence = Sequence::new();
    for index in 0..MAX_NOTES {
        sequence
            .add(note(MIN_PITCH + (index % KEY_COUNT) as u8, 0, 32))
            .unwrap();
    }
    let mut actual = playing(sequence, SAMPLE_RATE, 120);
    let mut expected = playing(sequence, SAMPLE_RATE, 120);
    assert_eq!(
        actual
            .voices
            .iter()
            .filter(|voice| voice.owner != Owner::Silent)
            .count(),
        MAX_VOICES
    );
    for index in MAX_NOTES - MAX_VOICES..MAX_NOTES {
        assert!(
            actual
                .voices
                .iter()
                .any(|voice| voice.owner == Owner::Sequence(index))
        );
    }
    actual
        .apply(Command::NoteOn {
            pitch: 83,
            velocity: 127,
        })
        .unwrap();
    expected
        .apply(Command::NoteOn {
            pitch: 83,
            velocity: 127,
        })
        .unwrap();
    assert!(
        !actual
            .voices
            .iter()
            .any(|voice| voice.owner == Owner::Sequence(48))
    );
    assert!(actual.voices.iter().any(|voice| voice.owner == Owner::Live));
    for _ in 0..20_000 {
        let sample = actual.next_sample();
        assert_eq!(sample.to_bits(), expected.next_sample().to_bits());
        assert!(sample.is_finite() && sample.abs() <= 1.0);
    }
}

#[test]
fn reading_the_playhead_at_irregular_intervals_does_not_change_music() {
    let mut observed = playing(Sequence::demo(), SAMPLE_RATE, 137);
    let mut unobserved = playing(Sequence::demo(), SAMPLE_RATE, 137);
    for frame in 0_u32..180_000 {
        if frame.is_multiple_of(17) || frame.is_multiple_of(997) {
            std::hint::black_box(observed.snapshot());
        }
        assert_eq!(
            observed.next_sample().to_bits(),
            unobserved.next_sample().to_bits()
        );
    }
    assert_eq!(observed.snapshot(), unobserved.snapshot());
}

#[test]
fn full_polyphony_is_finite_and_bounded_at_rate_and_gain_extremes() {
    for rate in [8_000, 44_100, 48_000, 192_000] {
        let mut processor = Processor::with_sample_rate(rate).unwrap();
        processor.apply(Command::SetVolume(1.0)).unwrap();
        for pitch in MIN_PITCH..=MAX_PITCH {
            processor
                .apply(Command::NoteOn {
                    pitch,
                    velocity: 127,
                })
                .unwrap();
        }
        let mut peak = 0.0_f32;
        for _ in 0..rate / 2 {
            let sample = processor.next_sample();
            assert!(sample.is_finite() && sample.abs() <= 1.0);
            peak = peak.max(sample.abs());
        }
        assert!(
            peak > 0.1,
            "the bounded output must still produce an audible signal"
        );
        for pitch in MIN_PITCH..=MAX_PITCH {
            processor.apply(Command::NoteOff { pitch }).unwrap();
        }
        render(&mut processor, u64::from(rate * 14 / 100));
        assert_eq!(processor.next_sample(), 0.0);
    }
}

#[test]
fn volume_zero_mutes_without_pausing_or_losing_notes() {
    let mut processor = playing(score(&[note(60, 0, 32)]), SAMPLE_RATE, 120);
    processor.apply(Command::SetVolume(0.0)).unwrap();
    for _ in 0..12_000 {
        assert_eq!(processor.next_sample(), 0.0);
    }
    assert_eq!(processor.snapshot().step, 2.0);
    assert_eq!(processor.snapshot().active_keys, key(60));
    processor.apply(Command::SetVolume(1.0)).unwrap();
    assert!((0..100).any(|_| processor.next_sample().abs() > 0.01));
}

#[test]
fn piano_envelope_attacks_then_decays_and_releases_to_exact_silence() {
    let mut processor = Processor::new();
    processor
        .apply(Command::NoteOn {
            pitch: 69,
            velocity: 127,
        })
        .unwrap();
    assert_eq!(processor.next_sample(), 0.0);
    let mut first_energy = 0.0_f64;
    for _ in 0..4_800 {
        first_energy += f64::from(processor.next_sample()).powi(2);
    }
    render(&mut processor, 96_000);
    let mut later_energy = 0.0_f64;
    for _ in 0..4_800 {
        later_energy += f64::from(processor.next_sample()).powi(2);
    }
    assert!(first_energy > later_energy * 2.0);
    processor.apply(Command::NoteOff { pitch: 69 }).unwrap();
    render(&mut processor, u64::from(SAMPLE_RATE * 14 / 100));
    assert_eq!(processor.next_sample(), 0.0);
}

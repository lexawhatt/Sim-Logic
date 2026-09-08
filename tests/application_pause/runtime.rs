use std::{error::Error, time::Duration};

use sim_logic::prelude::*;

use super::support::*;

#[test]
fn fixed_pause_finishes_one_tick_and_resume_retains_work_without_replaying_paused_input()
-> Result<(), Box<dyn Error>> {
    let mut application = application()?;
    application.add_fallible_fixed_system(|time: FixedTime, mut commands: Commands| {
        if time.tick_index() == 0 {
            commands.set_paused(true)?;
            commands.spawn(Marker)?;
        }
        Ok::<_, CommandEnqueueError>(())
    });
    application.add_fallible_fixed_system(move_visuals);
    application.add_fixed_system(observe_fixed);
    application.add_frame_system(observe_frame);
    application.add_fallible_frame_system(
        |input: FrameInput<TestAction>, time: FrameTime, mut commands: Commands| {
            if input.has_press_occurrence(TestAction::Pause) {
                assert!(time.is_paused());
                commands.set_paused(false)?;
                commands.spawn(Marker)?;
            }
            Ok::<_, CommandEnqueueError>(())
        },
    );
    let initial = register_world(&mut application, "retained-work", 0.0)?;
    let mut runner = application.build_headless(initial)?;

    let paused = advance(&mut runner, FIXED_STEP * 4, &[])?;
    assert!(paused.failure().is_none());
    assert_eq!(paused.timing().ticks_to_attempt(), 4);
    assert_eq!(paused.fixed_ticks_attempted(), 1);
    assert_eq!(paused.spawned(), 1);
    assert_eq!(
        paused.extracted_generation(),
        Some(runner.world_generation())
    );
    assert!(runner.is_paused());
    assert_eq!(probe(&runner)?.fixed_ticks, [0]);
    assert_eq!(probe(&runner)?.frame_pause_states, [true]);
    assert_eq!(extracted_x(&runner)?, 1.0);
    let (_, transform) = runner
        .components::<Transform2d>()
        .next()
        .ok_or("moving transform should remain")?;
    assert_eq!(transform.previous_translation(), transform.translation());
    let (_, camera) = runner
        .components::<ActiveCamera2d>()
        .next()
        .ok_or("moving camera should remain")?;
    assert_eq!(camera.previous_center(), camera.center());

    let resume = advance(
        &mut runner,
        Duration::from_secs(1),
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(resume.failure().is_none());
    assert_eq!(resume.fixed_ticks_attempted(), 0);
    assert_eq!(resume.timing().scaled_delta(), Duration::ZERO);
    assert_eq!(resume.spawned(), 1);
    assert!(!runner.is_paused());
    assert_eq!(extracted_x(&runner)?, 1.0);

    let retained = advance(&mut runner, Duration::ZERO, &[])?;
    assert!(retained.failure().is_none());
    assert_eq!(retained.fixed_ticks_attempted(), 3);
    let observed = probe(&runner)?;
    assert_eq!(observed.fixed_ticks, [0, 1, 2, 3]);
    assert_eq!(observed.frame_pause_states, [true, true, false]);
    assert_eq!(observed.fixed_pause_presses, 0);
    assert_eq!(observed.held_pause_ticks, 3);
    assert_eq!(runner.components::<Marker>().count(), 2);

    let drained = advance(&mut runner, Duration::ZERO, &[])?;
    assert_eq!(drained.fixed_ticks_attempted(), 0);
    Ok(())
}

#[test]
fn same_frame_fixed_pause_and_frame_resume_use_zero_alpha_for_only_that_frame()
-> Result<(), Box<dyn Error>> {
    let mut application = application()?;
    application.add_fallible_fixed_system(|time: FixedTime, mut commands: Commands| {
        if time.tick_index() == 0 {
            commands.set_paused(true)?;
        }
        Ok::<_, CommandEnqueueError>(())
    });
    application.add_fallible_fixed_system(move_visuals);
    application.add_fixed_system(observe_fixed);
    application.add_frame_system(observe_frame);
    application.add_fallible_frame_system(|time: FrameTime, mut commands: Commands| {
        if time.is_paused() {
            commands.set_paused(false)?;
        }
        Ok::<_, CommandEnqueueError>(())
    });
    let initial = register_world(&mut application, "same-frame-resume", 0.0)?;
    let mut runner = application.build_headless(initial)?;

    let resumed = advance(&mut runner, Duration::from_millis(45), &[])?;
    assert!(resumed.failure().is_none());
    assert_eq!(resumed.timing().ticks_to_attempt(), 4);
    assert_eq!(resumed.fixed_ticks_attempted(), 1);
    assert!(!runner.is_paused());
    assert_eq!(probe(&runner)?.frame_pause_states, [true]);
    assert_eq!(extracted_x(&runner)?, 1.0);
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("resume extraction")?
            .camera()
            .center(),
        Vec2::new(1.0, 0.0)
    );

    let next = advance(&mut runner, Duration::ZERO, &[])?;
    assert!(next.failure().is_none());
    assert_eq!(next.fixed_ticks_attempted(), 3);
    assert_eq!(probe(&runner)?.fixed_ticks, [0, 1, 2, 3]);
    assert_eq!(probe(&runner)?.frame_pause_states, [true, false]);
    // The retained half tick must now interpolate normally. A persistent
    // alpha-zero override would incorrectly publish x=3 instead of x=3.5.
    assert_eq!(extracted_x(&runner)?, 3.5);
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("next extraction")?
            .camera()
            .center(),
        Vec2::new(3.5, 0.0)
    );
    Ok(())
}

#[test]
fn host_state_is_symmetric_and_all_frame_systems_observe_the_stage_entry_snapshot()
-> Result<(), Box<dyn Error>> {
    let mut application = application()?;
    application.add_frame_system(observe_frame);
    application.add_fallible_frame_system(|time: FrameTime, mut commands: Commands| {
        commands.set_paused(!time.is_paused())
    });
    application.add_frame_system(observe_frame);
    let initial = register_world(&mut application, "entry-snapshot", 0.0)?;
    let mut runner = application.build_headless(initial)?;

    assert!(!runner.is_paused());
    runner.set_paused(true);
    assert!(runner.is_paused());
    runner.set_paused(false);
    assert!(!runner.is_paused());

    let pause = advance(&mut runner, Duration::ZERO, &[])?;
    assert!(pause.failure().is_none());
    assert!(runner.is_paused());
    let resume = advance(&mut runner, Duration::ZERO, &[])?;
    assert!(resume.failure().is_none());
    assert!(!runner.is_paused());
    assert_eq!(
        probe(&runner)?.frame_pause_states,
        [false, false, true, true]
    );
    Ok(())
}

fn remain_running(mut commands: Commands) -> Result<(), CommandEnqueueError> {
    commands.set_paused(false)?;
    commands.set_paused(true)?;
    commands.set_paused(false)
}

#[test]
fn frame_pause_and_resume_preserve_a_fractional_tick_without_adding_paused_wall_time()
-> Result<(), Box<dyn Error>> {
    let mut application = application()?;
    application.add_fallible_fixed_system(move_visuals);
    application.add_frame_system(observe_frame);
    application.add_fallible_frame_system(|probe: AppRes<Probe>, mut commands: Commands| {
        match probe.frame_pause_states.len() {
            1 => commands.set_paused(true)?,
            2 => commands.set_paused(false)?,
            _ => {}
        }
        Ok::<_, CommandEnqueueError>(())
    });
    let initial = register_world(&mut application, "fractional-retention", 0.0)?;
    let mut runner = application.build_headless(initial)?;

    let paused = advance(&mut runner, Duration::from_millis(15), &[])?;
    assert!(paused.failure().is_none());
    assert_eq!(paused.fixed_ticks_attempted(), 1);
    assert!(runner.is_paused());
    assert_eq!(extracted_x(&runner)?, 1.0);

    let resumed = advance(&mut runner, Duration::from_secs(1), &[])?;
    assert!(resumed.failure().is_none());
    assert_eq!(resumed.fixed_ticks_attempted(), 0);
    assert!(!runner.is_paused());
    assert_eq!(extracted_x(&runner)?, 1.0);

    let below_tick = advance(&mut runner, Duration::from_millis(4), &[])?;
    assert!(below_tick.failure().is_none());
    assert_eq!(below_tick.fixed_ticks_attempted(), 0);
    let completed_tick = advance(&mut runner, Duration::from_millis(1), &[])?;
    assert!(completed_tick.failure().is_none());
    assert_eq!(completed_tick.fixed_ticks_attempted(), 1);
    Ok(())
}

#[test]
fn last_write_wins_in_both_stages_without_transient_snapping_or_losing_pending_edges()
-> Result<(), Box<dyn Error>> {
    let mut application = application()?;
    application.add_fallible_fixed_system(move_visuals);
    application.add_fallible_fixed_system(remain_running);
    application.add_fixed_system(observe_fixed);
    application.add_fallible_frame_system(remain_running);
    let initial = register_world(&mut application, "last-write-wins", 0.0)?;
    let mut runner = application.build_headless(initial)?;

    let first = advance(&mut runner, Duration::from_millis(15), &[])?;
    assert!(first.failure().is_none());
    assert_eq!(extracted_x(&runner)?, 0.5);
    let pending = advance(
        &mut runner,
        Duration::ZERO,
        &[InputEvent::key(
            PhysicalKeyCode::Space,
            ButtonState::Pressed,
        )],
    )?;
    assert!(pending.failure().is_none());
    assert_eq!(pending.fixed_ticks_attempted(), 0);
    assert_eq!(extracted_x(&runner)?, 0.5);
    assert!(!runner.is_paused());

    let catch_up = advance(&mut runner, Duration::from_millis(35), &[])?;
    assert!(catch_up.failure().is_none());
    assert_eq!(catch_up.fixed_ticks_attempted(), 4);
    assert_eq!(probe(&runner)?.fixed_pause_presses, 1);
    assert_eq!(probe(&runner)?.fixed_ticks, [0, 1, 2, 3, 4]);
    Ok(())
}

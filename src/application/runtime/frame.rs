//! BeginFrame-to-EndFrame ordering and fixed/frame stage failure handling.

use std::time::Duration;

use crate::{
    commands::{CommandCommitError, CommittedCommands, DirectIntentIssuer},
    headless::HeadlessRunner,
    input::Action,
    render::FrameViewportState,
    system::Stage,
    time::{FixedTickError, FixedTimeState, FrameTimeState},
};

use super::{
    barriers::{StageExecutionFailure, apply_command_queue, run_stage},
    rendering::{
        capture_and_begin_fixed_interpolation, restore_previous_camera_centers,
        restore_previous_translations, snap_runtime_interpolation, stage_runtime_world,
    },
    reports::{
        BeginFrameRejection, FrameFailure, FrameOutcome, FrameRequest, FrameTransition,
        LogicFrameReport,
    },
    snapshots::{
        clear_fixed_snapshot, clear_frame_snapshot, copy_fixed_snapshot, prepare_frame_snapshots,
    },
};

impl<A: Action> HeadlessRunner<A> {
    /// Advances the same BeginFrame-to-EndFrame lifecycle used by the desktop
    /// adapter, without GPU preparation or presentation.
    pub fn advance_frame(&mut self, request: FrameRequest<'_>) -> FrameOutcome {
        let Some(next_frame) = self.frame_index.checked_add(1) else {
            return FrameOutcome::Rejected(BeginFrameRejection::FrameIdentityExhausted);
        };

        let mut planned_time = self.time.clone();
        let timing = match planned_time.plan_frame(request.elapsed()) {
            Ok(timing) => timing,
            Err(error) => return FrameOutcome::Rejected(BeginFrameRejection::Time(error)),
        };
        if let Err(error) = self.input.collect_frame(
            request.events(),
            self.time.is_paused(),
            self.application,
            self.active.generation,
            self.frame_index,
        ) {
            return FrameOutcome::Rejected(BeginFrameRejection::Input(error));
        }
        self.time = planned_time;

        let mut report = LogicFrameReport {
            frame_index: self.frame_index,
            timing,
            fixed_ticks_attempted: 0,
            spawned: 0,
            despawned: 0,
            exit_requested: false,
            transitions_suppressed_by_exit: 0,
            dropped_for_transition: None,
            transition: FrameTransition::None,
            extracted_generation: None,
            failure: None,
        };
        if !self.update_common_resources() {
            report.failure = Some(FrameFailure::RuntimeInvariant);
        }

        let mut force_zero_extraction_alpha = false;
        let mut fixed_transition = None;
        for tick_number in 0..timing.ticks_to_attempt() {
            if report.failure.is_some() {
                break;
            }
            let fixed_time = match self.time.begin_fixed_tick() {
                Ok(time) => time,
                Err(error) => {
                    report.failure = Some(fixed_tick_failure(error));
                    break;
                }
            };
            report.fixed_ticks_attempted += 1;
            if self.active.fixed.is_empty() {
                self.active.previous_translations.clear();
                self.active.previous_camera_centers.clear();
            } else if !capture_and_begin_fixed_interpolation(
                &mut self.active.world,
                &mut self.active.interpolation_queries,
                &mut self.active.previous_translations,
                &mut self.active.previous_camera_centers,
            ) {
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }
            self.active.world.insert_resource(fixed_time);
            if !copy_fixed_snapshot(&mut self.active.world, &self.input) {
                restore_previous_translations(
                    &mut self.active.world,
                    &self.active.previous_translations,
                );
                restore_previous_camera_centers(
                    &mut self.active.world,
                    &self.active.previous_camera_centers,
                );
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }

            if let Err(error) = run_stage(&mut self.active.fixed, &mut self.active.world) {
                restore_previous_translations(
                    &mut self.active.world,
                    &self.active.previous_translations,
                );
                restore_previous_camera_centers(
                    &mut self.active.world,
                    &self.active.previous_camera_centers,
                );
                let commands_intact = self.consume_failed_stage_tokens();
                self.input.consume_fixed_delivery();
                self.time.stop_remaining_ticks();
                report.failure = Some(if commands_intact {
                    match error {
                        StageExecutionFailure::System(error) => FrameFailure::System {
                            stage: Stage::FixedUpdate,
                            error,
                        },
                        StageExecutionFailure::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                    }
                } else {
                    FrameFailure::RuntimeInvariant
                });
                break;
            }

            let (failed_tokens, committed) = match apply_command_queue(
                &mut self.active.world,
                &mut self.active.approved,
                self.application,
                self.active.generation,
                self.active.managed_entities,
                self.config.entity_limit(),
            ) {
                Some(outcome) => outcome,
                None => {
                    restore_previous_translations(
                        &mut self.active.world,
                        &self.active.previous_translations,
                    );
                    restore_previous_camera_centers(
                        &mut self.active.world,
                        &self.active.previous_camera_centers,
                    );
                    self.input.consume_fixed_delivery();
                    self.time.stop_remaining_ticks();
                    report.failure = Some(FrameFailure::RuntimeInvariant);
                    break;
                }
            };
            let committed = match committed {
                Ok(committed) => committed,
                Err(error) => {
                    restore_previous_translations(
                        &mut self.active.world,
                        &self.active.previous_translations,
                    );
                    restore_previous_camera_centers(
                        &mut self.active.world,
                        &self.active.previous_camera_centers,
                    );
                    self.consume_tokens(&failed_tokens);
                    self.input.consume_fixed_delivery();
                    self.time.stop_remaining_ticks();
                    report.failure = Some(match error {
                        CommandCommitError::Batch(error) => FrameFailure::Commands {
                            stage: Stage::FixedUpdate,
                            error,
                        },
                        CommandCommitError::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                    });
                    break;
                }
            };
            self.active.managed_entities = committed.live_entities;
            add_committed_counts(&mut report, &committed);
            let pause = committed.pause;

            if committed.exit_requested {
                report.exit_requested = true;
                report.transitions_suppressed_by_exit = committed.transitions.len();
                self.input.clear_world_edges();
                snap_runtime_interpolation(&mut self.active);
                self.time.clear_accumulator();
                if !clear_fixed_snapshot::<A>(&mut self.active.world) {
                    report.failure = Some(FrameFailure::RuntimeInvariant);
                }
                break;
            }

            if !committed.transitions.is_empty() {
                fixed_transition = Some((committed.transitions, pause));
                self.time.stop_remaining_ticks();
                break;
            }

            self.input.consume_fixed_delivery();
            force_zero_extraction_alpha |= self.apply_committed_pause(pause);
            if self.time.is_paused() {
                break;
            }

            if tick_number + 1 == timing.ticks_to_attempt()
                && !clear_fixed_snapshot::<A>(&mut self.active.world)
            {
                report.failure = Some(FrameFailure::RuntimeInvariant);
                break;
            }
        }

        if report.failure.is_none() && !report.exit_requested {
            if let Some((requests, pause)) = fixed_transition {
                self.resolve_transition(requests, true, &mut report);
                // Transition validation must happen while input-backed intent
                // tokens are still live. Once arbitration is complete, every
                // edge delivered to this fixed tick is consumed together.
                self.input.consume_fixed_delivery();
                force_zero_extraction_alpha |= self.apply_committed_pause(pause);
            } else {
                self.run_frame_update(
                    request.elapsed(),
                    request.viewport(),
                    &mut report,
                    &mut force_zero_extraction_alpha,
                );
            }
        }

        if report.failure.is_none()
            && !report.exit_requested
            && report.extracted_generation.is_none()
        {
            let alpha = if force_zero_extraction_alpha {
                Some(0.0)
            } else {
                self.time.interpolation_alpha()
            };
            match alpha {
                Some(alpha) => match stage_runtime_world(
                    &mut self.active,
                    alpha as f32,
                    self.config.render(),
                    &mut self.extraction,
                ) {
                    Ok(()) => {
                        report.extracted_generation = self.extraction.publish();
                        if report.extracted_generation.is_none() {
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                        }
                    }
                    Err(error) => report.failure = Some(FrameFailure::Extraction(error)),
                },
                None => report.failure = Some(FrameFailure::RuntimeInvariant),
            }
        }

        self.input.end_frame();
        if let Some(mut issuer) = self.active.world.get_resource_mut::<DirectIntentIssuer>() {
            issuer.disable();
        }
        // `bevy_ecs` is used without `bevy_app`, so its per-frame change and
        // removal trackers are our responsibility. Advancing them here keeps
        // removed temporary resources bounded and gives the next application
        // frame the same change-detection boundary as a Bevy App update.
        self.active.world.clear_trackers();
        self.frame_index = next_frame;
        FrameOutcome::Advanced(report)
    }

    fn update_common_resources(&mut self) -> bool {
        let frame_snapshot_present = clear_frame_snapshot::<A>(&mut self.active.world);
        self.active.world.remove_resource::<FrameTimeState>();
        self.active.world.remove_resource::<FrameViewportState>();
        let Some(mut issuer) = self.active.world.get_resource_mut::<DirectIntentIssuer>() else {
            return false;
        };
        issuer.set_frame(self.frame_index, self.active.generation);
        frame_snapshot_present
    }

    fn run_frame_update(
        &mut self,
        elapsed: Duration,
        viewport: sim_engine::LogicalViewport,
        report: &mut LogicFrameReport,
        force_zero_extraction_alpha: &mut bool,
    ) {
        if !prepare_frame_snapshots(&mut self.active.world, &self.input) {
            report.failure = Some(FrameFailure::RuntimeInvariant);
            return;
        }
        self.active.world.remove_resource::<FixedTimeState>();
        self.active
            .world
            .insert_resource(self.time.frame_time(elapsed));
        self.active
            .world
            .insert_resource(FrameViewportState::new(viewport));
        if let Err(error) = run_stage(&mut self.active.frame, &mut self.active.world) {
            let commands_intact = self.consume_failed_stage_tokens();
            report.failure = Some(if commands_intact {
                match error {
                    StageExecutionFailure::System(error) => FrameFailure::System {
                        stage: Stage::FrameUpdate,
                        error,
                    },
                    StageExecutionFailure::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                }
            } else {
                FrameFailure::RuntimeInvariant
            });
            return;
        }

        let Some((failed_tokens, committed)) = apply_command_queue(
            &mut self.active.world,
            &mut self.active.approved,
            self.application,
            self.active.generation,
            self.active.managed_entities,
            self.config.entity_limit(),
        ) else {
            report.failure = Some(FrameFailure::RuntimeInvariant);
            return;
        };
        let committed = match committed {
            Ok(committed) => committed,
            Err(error) => {
                self.consume_tokens(&failed_tokens);
                report.failure = Some(match error {
                    CommandCommitError::Batch(error) => FrameFailure::Commands {
                        stage: Stage::FrameUpdate,
                        error,
                    },
                    CommandCommitError::RuntimeInvariant => FrameFailure::RuntimeInvariant,
                });
                return;
            }
        };
        self.active.managed_entities = committed.live_entities;
        add_committed_counts(report, &committed);
        let pause = committed.pause;
        if committed.exit_requested {
            report.exit_requested = true;
            report.transitions_suppressed_by_exit = committed.transitions.len();
            self.input.clear_world_edges();
            snap_runtime_interpolation(&mut self.active);
            self.time.clear_accumulator();
            return;
        }
        if !committed.transitions.is_empty() {
            self.resolve_transition(committed.transitions, false, report);
        }
        *force_zero_extraction_alpha |= self.apply_committed_pause(pause);
    }

    fn apply_committed_pause(&mut self, pause: Option<bool>) -> bool {
        let Some(paused) = pause else {
            return false;
        };
        let resuming = self.time.is_paused() && !paused;
        self.set_paused(paused);
        // Resume occurs after fixed execution. Retain any whole ticks for the
        // next frame, but present the snapped state now with a valid alpha.
        resuming && self.time.interpolation_alpha().is_none()
    }
}

fn add_committed_counts(report: &mut LogicFrameReport, committed: &CommittedCommands) {
    report.spawned = report.spawned.saturating_add(committed.spawned);
    report.despawned = report.despawned.saturating_add(committed.despawned);
}

fn fixed_tick_failure(error: FixedTickError) -> FrameFailure {
    match error {
        FixedTickError::TickIndexExhausted
        | FixedTickError::Paused
        | FixedTickError::NoPlannedTick => FrameFailure::RuntimeInvariant,
    }
}

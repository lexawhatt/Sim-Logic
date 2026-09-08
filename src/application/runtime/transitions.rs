//! Intent validation, deterministic transition arbitration, and World installation.

use crate::{
    headless::HeadlessRunner,
    identity::{IntentKind, TransitionIntentToken},
    input::Action,
    transition::{TransitionArbitration, TransitionRequest, arbitrate},
};

use super::{
    barriers::discard_command_queue,
    lifecycle::factory_exists,
    rendering::snap_runtime_interpolation,
    reports::{
        FrameFailure, FrameTransition, LifecycleEvent, LogicFrameReport, TransitionRequestFailure,
    },
};

impl<A: Action> HeadlessRunner<A> {
    pub(super) fn resolve_transition(
        &mut self,
        requests: Vec<TransitionRequest>,
        stopped_fixed: bool,
        report: &mut LogicFrameReport,
    ) {
        let validation = self.validate_transition_requests(&requests, stopped_fixed);
        let tokens: Vec<_> = requests.iter().map(|request| request.intent()).collect();
        self.consume_tokens(&tokens);

        if let Err(failure) = validation {
            report.transition = FrameTransition::Invalid(failure);
            self.finish_rejected_fixed_transition(stopped_fixed, report);
            return;
        }

        match arbitrate(&requests) {
            TransitionArbitration::None => {}
            TransitionArbitration::Rejected(rejection) => {
                report.transition = FrameTransition::Rejected(rejection);
                self.finish_rejected_fixed_transition(stopped_fixed, report);
            }
            TransitionArbitration::Replace { target, warning } => {
                match self.prepare_world(target) {
                    Ok(mut candidate) => {
                        if !self
                            .application_resources
                            .can_transfer(&self.active.world, &candidate.world)
                        {
                            self.extraction.discard_staged();
                            self.time.clear_accumulator();
                            snap_runtime_interpolation(&mut self.active);
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                            return;
                        }
                        let old = self.active.generation;
                        self.record_lifecycle(LifecycleEvent::WorldExit, old, self.active.factory);
                        self.application_resources
                            .transfer_prevalidated(&mut self.active.world, &mut candidate.world);
                        let new = candidate.generation;
                        self.active = candidate;
                        self.record_lifecycle(LifecycleEvent::WorldEnter, new, target);
                        snap_runtime_interpolation(&mut self.active);
                        self.time.clear_accumulator();
                        self.input.clear_world_edges();
                        report.transition = FrameTransition::Committed {
                            old,
                            new,
                            target,
                            warning,
                        };
                        report.extracted_generation = self.extraction.publish();
                        if report.extracted_generation != Some(new) {
                            report.failure = Some(FrameFailure::RuntimeInvariant);
                        }
                    }
                    Err(error) => {
                        self.time.clear_accumulator();
                        snap_runtime_interpolation(&mut self.active);
                        report.transition = FrameTransition::PreparationFailed { target, error };
                    }
                }
            }
        }
    }

    fn finish_rejected_fixed_transition(
        &mut self,
        stopped_fixed: bool,
        report: &mut LogicFrameReport,
    ) {
        if stopped_fixed {
            match self.time.drop_remaining_whole_ticks() {
                Ok(dropped) => report.dropped_for_transition = Some(dropped),
                Err(error) => report.failure = Some(FrameFailure::Time(error)),
            }
        }
    }

    fn validate_transition_requests(
        &self,
        requests: &[TransitionRequest],
        from_fixed_update: bool,
    ) -> Result<(), TransitionRequestFailure> {
        for request in requests {
            if !factory_exists(self.application, &self.factories, request.target()) {
                return Err(TransitionRequestFailure::InvalidFactory);
            }
            let token = request.intent();
            if token.application() != self.application || token.origin() != self.active.generation {
                return Err(TransitionRequestFailure::InvalidIntent);
            }
            match token.kind() {
                IntentKind::Input
                    if from_fixed_update && !self.input.token_is_live_in_fixed(token) =>
                {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Input
                    if !from_fixed_update && !self.input.token_is_live_in_frame(token) =>
                {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Direct if token.issued_frame() != self.frame_index => {
                    return Err(TransitionRequestFailure::InvalidIntent);
                }
                IntentKind::Input | IntentKind::Direct => {}
            }
        }
        Ok(())
    }

    pub(super) fn consume_tokens(&mut self, tokens: &[TransitionIntentToken]) {
        for token in tokens {
            if token.kind() == IntentKind::Input {
                self.input.consume_token_occurrence(*token);
            }
        }
    }

    pub(super) fn consume_failed_stage_tokens(&mut self) -> bool {
        let Some(tokens) = discard_command_queue(&mut self.active.world) else {
            return false;
        };
        self.consume_tokens(&tokens);
        true
    }
}

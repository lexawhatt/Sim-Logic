//! Narrow opt-in admission-error recovery, never a catch-all renderer fallback.
use crate::{
    ExtractionError,
    headless::{FrameFailure, LogicFrameReport},
    identity::WorldGeneration,
    render::{PresentationBudgetRejection, PresentationBudgetResource, PresentationBudgetStage},
};
use sim_engine::{FrameBudgetResource, FrameComposerError, SceneError};

#[cfg(all(test, target_os = "linux"))]
#[path = "budget_recovery_gpu.rs"]
mod gpu;

pub(super) fn logical(
    report: &LogicFrameReport,
    generation: WorldGeneration,
) -> Option<PresentationBudgetRejection> {
    let (stage, error) = match report.failure()? {
        FrameFailure::Extraction(ExtractionError::ScreenScene(error)) => {
            (PresentationBudgetStage::ScreenScene, error)
        }
        FrameFailure::Extraction(ExtractionError::Scene(error)) => {
            (PresentationBudgetStage::WorldScene, error)
        }
        _ => return None,
    };
    let SceneError::BudgetExceeded {
        resource,
        limit,
        requested,
    } = *error
    else {
        return None;
    };
    Some(PresentationBudgetRejection {
        frame_index: report.frame_index(),
        generation,
        stage,
        resource: PresentationBudgetResource::Scene(resource),
        limit,
        requested,
    })
}

pub(super) fn composed(
    error: &FrameComposerError,
    frame_index: u64,
    generation: WorldGeneration,
) -> Option<PresentationBudgetRejection> {
    let FrameComposerError::BudgetExceeded {
        resource,
        limit,
        actual,
    } = *error
    else {
        return None;
    };
    let resource = match resource {
        FrameBudgetResource::Passes => PresentationBudgetResource::Passes,
        FrameBudgetResource::Commands => PresentationBudgetResource::Commands,
        FrameBudgetResource::Vertices => PresentationBudgetResource::Vertices,
        FrameBudgetResource::UploadBytes => PresentationBudgetResource::UploadBytes,
        FrameBudgetResource::TextureBytes => PresentationBudgetResource::TextureBytes,
        FrameBudgetResource::DrawCalls => PresentationBudgetResource::DrawCalls,
    };
    Some(PresentationBudgetRejection {
        frame_index,
        generation,
        stage: PresentationBudgetStage::Composition,
        resource,
        limit,
        requested: actual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{identity::ApplicationId, render::PresentationFeedback};

    #[test]
    fn only_typed_budget_failure_is_recoverable_and_feedback_is_latched() {
        let generation = WorldGeneration::new(ApplicationId::issue().unwrap(), 1);
        assert!(
            composed(
                &FrameComposerError::AllocationFailed {
                    requested_bytes: 123
                },
                7,
                generation
            )
            .is_none()
        );
        let failure = FrameComposerError::BudgetExceeded {
            resource: FrameBudgetResource::Vertices,
            limit: 100,
            actual: 101,
        };
        let rejection = composed(&failure, 7, generation).unwrap();
        assert_eq!(rejection.resource, PresentationBudgetResource::Vertices);
        assert_eq!(
            (rejection.frame_index, rejection.limit, rejection.requested),
            (7, 100, 101)
        );
        let mut feedback = PresentationFeedback::default();
        feedback.reject(rejection);
        assert_eq!(feedback.last_rejection(), Some(rejection));
        feedback.reject(rejection);
        assert_eq!(feedback.rejected_frames(), 2);
        feedback.presented();
        assert!(feedback.last_rejection().is_none());
        assert_eq!(feedback.rejected_frames(), 2);
    }

    #[test]
    fn extraction_feedback_is_explicit_and_reaches_the_next_application_update()
    -> crate::LogicResult {
        use crate::prelude::*;
        use std::time::Duration;
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        enum Action {}
        for opt_in in [false, true] {
            let mut config = AppConfig::default();
            config.set_render_limits(RenderLimits::default().with_screen_scene_budget(
                SceneBudget::new(1, 0, 1000, 100_000, 100_000, 100_000, 1000),
            ));
            let mut app = Application::<Action>::new(config)?;
            if opt_in {
                app.register_app_resource(PresentationFeedback::default())?;
            }
            let rectangle = ScreenRectangleVisual::new(
                LogicalScreenPosition::new(2.0, 2.0),
                LogicalScreenVector::new(3.0, 3.0),
                Color::WHITE,
            )?;
            app.add_fallible_frame_system(move |mut commands: Commands| -> crate::LogicResult {
                commands.spawn(rectangle)?;
                Ok(())
            });
            let camera = ActiveCamera2d::centered(1.0)?;
            let initial = app.register_world("feedback", move |world| {
                world.spawn(camera)?;
                world.spawn(rectangle)?;
                Ok(())
            })?;
            let mut runner = app.build_headless(initial)?;
            let FrameOutcome::Advanced(report) = runner.advance_frame(FrameRequest::new(
                Duration::ZERO,
                &[],
                LogicalViewport::new(64.0, 64.0)?,
            )) else {
                return Err("frame rejected".into());
            };
            let rejected =
                logical(&report, runner.world_generation()).ok_or("missing typed rejection")?;
            assert_eq!(rejected.stage, PresentationBudgetStage::ScreenScene);
            assert_eq!(
                rejected.resource,
                PresentationBudgetResource::Scene(SceneBudgetResource::Commands)
            );
            assert_eq!((rejected.limit, rejected.requested), (1, 2));
            assert_eq!(
                runner
                    .with_presentation_feedback(|feedback| feedback.reject(rejected))
                    .is_some(),
                opt_in
            );
            if opt_in {
                assert_eq!(
                    runner
                        .app_resource::<PresentationFeedback>()
                        .unwrap()
                        .last_rejection(),
                    Some(rejected)
                );
            }
            assert_eq!(
                runner
                    .extracted_frame()
                    .unwrap()
                    .screen_primitives()
                    .count(),
                1
            );
        }
        Ok(())
    }
}

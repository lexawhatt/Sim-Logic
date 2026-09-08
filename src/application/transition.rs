//! No-payload World replacement helpers and deterministic arbitration.

use bevy_ecs::prelude::{Res, Resource};

use crate::{
    commands::{CommandEnqueueError, LogicCommands},
    identity::{TransitionIntentToken, WorldFactoryId},
    input::{Action, FixedInput},
};

/// One World-owned route from a fixed input press to World replacement.
///
/// The descriptor is inert until
/// [`crate::app::Application::add_world_replacement_on_press_system`] is
/// registered. That adapter observes every matching press occurrence in
/// FixedUpdate and forwards the occurrence's runtime-issued causal token to
/// the ordinary deferred World-replacement command path. Physical key binding
/// remains separate application configuration.
///
/// A route belongs only to the World containing this Resource and disappears
/// when that World is replaced. Applications needing conditional navigation,
/// multiple destinations, payloads, or FrameUpdate timing should use an
/// ordinary System instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub struct WorldReplacementOnPress<A: Action> {
    action: A,
    target: WorldFactoryId,
}

impl<A: Action> WorldReplacementOnPress<A> {
    /// Creates one infallible action-to-factory route.
    ///
    /// Factory provenance is checked by the existing transition runtime only
    /// when a matching press requests replacement. Replace the complete Copy
    /// Resource through `ResMut` when an earlier System must change both route
    /// fields atomically.
    pub const fn new(action: A, target: WorldFactoryId) -> Self {
        Self { action, target }
    }

    /// Returns the logical action whose press occurrences request replacement.
    pub const fn action(&self) -> A {
        self.action
    }

    /// Returns the opaque registered World factory selected by this route.
    pub const fn target(&self) -> WorldFactoryId {
        self.target
    }
}

pub(crate) fn replace_world_on_press<A: Action>(
    input: FixedInput<A>,
    route: Option<Res<WorldReplacementOnPress<A>>>,
    mut commands: LogicCommands,
) -> Result<(), CommandEnqueueError> {
    let Some(route) = route else {
        return Ok(());
    };
    for edge in input.pressed(route.action) {
        commands.replace_world(edge.intent(), route.target)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TransitionRequest {
    intent: TransitionIntentToken,
    target: WorldFactoryId,
}

impl TransitionRequest {
    pub(crate) const fn new(intent: TransitionIntentToken, target: WorldFactoryId) -> Self {
        Self { intent, target }
    }

    pub(crate) const fn intent(self) -> TransitionIntentToken {
        self.intent
    }

    pub(crate) const fn target(self) -> WorldFactoryId {
        self.target
    }
}

/// Diagnostic emitted when independent causes converge on the same World.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConvergentTransitionWarning {
    distinct_intents: usize,
}

impl ConvergentTransitionWarning {
    /// Returns the number of distinct causal tokens that requested the target.
    pub const fn distinct_intents(self) -> usize {
        self.distinct_intents
    }
}

/// Reason an otherwise well-formed transition batch did not select a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionRejection {
    /// One causal token was reused for different targets.
    MalformedIntent,
    /// Independent causal tokens selected different targets.
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionArbitration {
    None,
    Replace {
        target: WorldFactoryId,
        warning: Option<ConvergentTransitionWarning>,
    },
    Rejected(TransitionRejection),
}

pub(crate) fn arbitrate(requests: &[TransitionRequest]) -> TransitionArbitration {
    let Some(first) = requests.first().copied() else {
        return TransitionArbitration::None;
    };

    for (index, request) in requests.iter().enumerate() {
        if requests[..index]
            .iter()
            .any(|earlier| earlier.intent == request.intent && earlier.target != request.target)
        {
            return TransitionArbitration::Rejected(TransitionRejection::MalformedIntent);
        }
    }

    if requests
        .iter()
        .any(|request| request.target != first.target)
    {
        return TransitionArbitration::Rejected(TransitionRejection::Conflict);
    }

    let distinct_intents = requests
        .iter()
        .enumerate()
        .filter(|(index, request)| {
            !requests[..*index]
                .iter()
                .any(|earlier| earlier.intent == request.intent)
        })
        .count();

    TransitionArbitration::Replace {
        target: first.target,
        warning: (distinct_intents > 1).then_some(ConvergentTransitionWarning { distinct_intents }),
    }
}

#[cfg(test)]
mod tests {
    use crate::identity::{ApplicationId, WorldGeneration};

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {
        Continue,
    }

    fn factory(application: ApplicationId, slot: u32) -> WorldFactoryId {
        WorldFactoryId::new(application, slot, 1)
    }

    fn intent(application: ApplicationId, occurrence: u64) -> TransitionIntentToken {
        TransitionIntentToken::direct(
            application,
            WorldGeneration::new(application, 1),
            9,
            occurrence,
        )
    }

    #[test]
    fn replacement_on_press_preserves_its_action_and_opaque_target() {
        let application = ApplicationId::from_raw(1);
        let target = factory(application, 3);
        let route = WorldReplacementOnPress::new(TestAction::Continue, target);

        assert_eq!(route.action(), TestAction::Continue);
        assert_eq!(route.target(), target);
        assert_eq!(route, route);
        let copied = route;
        assert_eq!(copied, route);
    }

    #[test]
    fn same_intent_and_target_coalesce() {
        let application = ApplicationId::from_raw(1);
        let target = factory(application, 0);
        let intent = intent(application, 1);
        let requests = [
            TransitionRequest::new(intent, target),
            TransitionRequest::new(intent, target),
        ];

        assert_eq!(
            arbitrate(&requests),
            TransitionArbitration::Replace {
                target,
                warning: None,
            }
        );
    }

    #[test]
    fn same_intent_with_different_targets_is_malformed() {
        let application = ApplicationId::from_raw(1);
        let intent = intent(application, 1);
        let requests = [
            TransitionRequest::new(intent, factory(application, 0)),
            TransitionRequest::new(intent, factory(application, 1)),
        ];

        assert_eq!(
            arbitrate(&requests),
            TransitionArbitration::Rejected(TransitionRejection::MalformedIntent)
        );
    }

    #[test]
    fn different_intents_with_same_target_converge() {
        let application = ApplicationId::from_raw(1);
        let target = factory(application, 0);
        let requests = [
            TransitionRequest::new(intent(application, 1), target),
            TransitionRequest::new(intent(application, 2), target),
        ];

        assert_eq!(
            arbitrate(&requests),
            TransitionArbitration::Replace {
                target,
                warning: Some(ConvergentTransitionWarning {
                    distinct_intents: 2,
                }),
            }
        );
    }

    #[test]
    fn different_intents_with_different_targets_conflict() {
        let application = ApplicationId::from_raw(1);
        let requests = [
            TransitionRequest::new(intent(application, 1), factory(application, 0)),
            TransitionRequest::new(intent(application, 2), factory(application, 1)),
        ];

        assert_eq!(
            arbitrate(&requests),
            TransitionArbitration::Rejected(TransitionRejection::Conflict)
        );
    }
}

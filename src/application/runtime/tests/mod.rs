//! Behavior regression tests for the shared application runtime.
//!
//! Topic modules inherit imports from this test-only parent. Fault injection
//! stays beneath RuntimeWorld so production fields need no test accessors.

use std::{
    any::type_name,
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bevy_ecs::{
    entity_disabling::{DefaultQueryFilters, Disabled},
    prelude::{Component, Res, ResMut, Resource, With},
};
use sim_engine::{Camera2d, Color, LogicalViewport, Vec2};

use crate::{
    app::{AppConfig, Application},
    commands::{CommandEnqueueError, CommandQueue, DirectIntentIssuer, LogicCommands},
    events::{EventReader, EventSendError, EventWriter},
    identity::{
        LogicEntity, LogicEntityRef, ManagedEntity, TransitionIntentToken, WorldGeneration,
    },
    input::{
        ButtonState, FixedInput, FixedInputState, FrameInput, FrameInputState,
        InputCollectionError, InputEvent, PhysicalKeyCode,
    },
    query::{Query, QueryEntityError, Single},
    render::FrameViewport,
    system::Stage,
    time::{FixedTime, FrameTime, TimeConfig},
    transition::TransitionRejection,
    visual::{ActiveCamera2d, CircleVisual, Transform2d},
    world::WorldBuildError,
};

use crate::headless::{
    BeginFrameRejection, CandidateFailure, FrameFailure, FrameOutcome, FrameRequest,
    FrameTransition, LifecycleEvent, RunnerBuildError, TransitionRequestFailure,
};

use super::rendering::capture_and_begin_fixed_interpolation;

mod fixtures;
use fixtures::*;

mod commands;
mod events;
mod exit_pause_input;
mod failures;
mod identity;
mod interpolation;
mod stage_contracts;
mod transitions;

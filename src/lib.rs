//! A code-first application and logic layer for Sim;Engine.
//!
//! Sim;Logic owns the ordinary application loop around an ECS world: staged
//! systems, bounded structural spawn/insert/remove/despawn commands, typed input, fixed-step time,
//! application-owned resources, World replacement, application-requested exit,
//! and extraction into Sim;Engine scenes.

#![warn(missing_docs)]

#[path = "application/app.rs"]
pub mod app;
#[path = "assets/mod.rs"]
pub mod assets;
#[cfg(feature = "audio")]
pub mod audio;
#[path = "logic/collision.rs"]
pub mod collision;
#[path = "ecs/commands.rs"]
pub mod commands;
#[path = "ecs/component.rs"]
mod component;
#[cfg(feature = "desktop")]
#[path = "platform/desktop.rs"]
pub mod desktop;
#[cfg(feature = "easter-eggs")]
#[path = "rendering/easter_eggs/mod.rs"]
pub mod easter_eggs;
#[path = "ecs/events.rs"]
pub mod events;
#[path = "rendering/extraction.rs"]
mod extraction;
#[path = "application/headless.rs"]
pub mod headless;

#[path = "ecs/identity.rs"]
pub mod identity;
#[path = "application/input.rs"]
pub mod input;
#[path = "logic/motion.rs"]
pub mod motion;
#[path = "ecs/query.rs"]
pub mod query;
#[path = "rendering/render.rs"]
pub mod render;
#[path = "application/resources.rs"]
pub mod resources;
#[path = "rendering/screen.rs"]
pub mod screen;
#[path = "ecs/system.rs"]
pub mod system;
#[cfg(feature = "text")]
pub mod text;
#[path = "rendering/three_d/mod.rs"]
pub mod three_d;
#[path = "application/time.rs"]
pub mod time;
#[path = "application/transition.rs"]
pub mod transition;
#[path = "rendering/visual.rs"]
pub mod visual;
#[path = "ecs/world.rs"]
pub mod world;

/// Exact ECS foundation used by Sim;Logic.
///
/// Re-exporting the crate lets downstream `Component` and `Resource` derive
/// macros resolve their generated Bevy paths with only a `sim-logic`
/// dependency.
pub use bevy_ecs;
pub use component::{ComponentApprovalError, ComponentTuple, LifecycleHook};
#[cfg(feature = "easter-eggs")]
pub use easter_eggs::{CrabDrawError, draw_crab};
#[cfg(feature = "text")]
pub use extraction::ResolvedScreenText;
pub use extraction::{
    ExtractedFrame, ExtractionError, ResolvedCircle, ResolvedLine, ResolvedRectangle,
    ResolvedScreenImage, ResolvedScreenRectangle, ScreenDraw,
};

/// Convenience result for application code and fallible Systems that combine
/// unrelated owned error types with `?`.
///
/// Sim;Logic operations continue to return their concrete error types, and
/// fallible Systems may return any `Display + 'static` error without using
/// this alias. The boxed error is not required to be `Send` or `Sync`;
/// applications needing those bounds or a concrete error enum should define
/// their own result type.
pub type LogicResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Commonly used Sim;Logic and ECS types.
pub mod prelude {
    pub use crate::bevy_ecs;
    #[cfg(all(feature = "desktop", feature = "text"))]
    pub use crate::desktop::DesktopTextError;
    #[cfg(feature = "desktop")]
    pub use crate::desktop::FrameCacheBudget;
    pub use bevy_ecs::prelude::{Bundle, Component, Res, ResMut, Resource, With, Without};
    #[cfg(feature = "desktop")]
    pub use sim_engine::RendererPresentMode;
    pub use sim_engine::{
        Camera2d, Camera2dError, Camera3d, Color, LogicalPixels, LogicalScreenPosition,
        LogicalScreenVector, LogicalViewport, Rotation3d, Transform3d, Vec2, Vec3,
        WireframeStyle3d, WorldLength,
    };

    pub use crate::ComponentTuple;
    pub use crate::LogicResult;
    #[cfg(feature = "text")]
    pub use crate::ResolvedScreenText;
    pub use crate::app::{AppConfig, Application};
    pub use crate::assets::{ImageAsset, ImageAssetError, ImageAssetId, ImageAssetLimits};
    pub use crate::collision::{
        CircleCollider2d, CircleColliderError, CircleOverlapEntities, RectangleCollider2d,
        RectangleColliderError, RectangleOverlapEntities,
    };
    pub use crate::commands::{CommandEnqueueError, LogicCommands as Commands};
    #[cfg(feature = "desktop")]
    pub use crate::desktop::{
        DesktopConfig, DesktopExitReason, DesktopImageError, DesktopPointerError, DesktopRunError,
        DesktopRunReport, DesktopThreeDError,
    };
    pub use crate::events::{EventReader, EventSendError, EventWriter, WorldEvent};
    pub use crate::headless::{
        BeginFrameRejection, CandidateFailure, FrameFailure, FrameOutcome, FrameRequest,
        FrameTransition, HeadlessRunner, LifecycleEvent, LifecycleRecord, LogicFrameReport,
        RunnerBuildError, TransitionRequestFailure,
    };
    pub use crate::identity::{
        LogicEntity, LogicEntityRef, TransitionIntentToken, WorldFactoryId, WorldGeneration,
    };
    pub use crate::input::{
        Action, ActionEdge, ButtonState, DigitalAxis2d, DuplicateMouseBinding, FixedInput,
        FrameInput, InputCancellationReason, InputCollectionError, InputControl, InputEvent,
        MouseButton, PhysicalKeyCode, PointerSample, PointerSampleError,
    };
    pub use crate::motion::{
        CameraFollowTarget2d, CameraFollowTarget2dError, DigitalMovement2d, DigitalMovement2dError,
        LinearAcceleration2d, LinearAcceleration2dError, LinearVelocity2d, LinearVelocity2dError,
    };
    pub use crate::query::{Query, QueryEntityError, QuerySingleError, Single};
    pub use crate::render::{FrameLimits, FrameViewport, RenderLimits};
    pub use crate::resources::{AppRes, AppResMut, ApplicationResourceError};
    pub use crate::screen::{
        ImageFilter, ImageRegion, ImageVisualError, ScreenImageVisual, ScreenRectangleVisual,
        ScreenVisualError,
    };
    pub use crate::system::{Stage, SystemRunFailure, SystemSetupError};
    #[cfg(feature = "text")]
    pub use crate::text::{
        ScreenTextVisual, TextAlignment, TextError, TextFont, TextLimits, TextMetrics, TextSettings,
    };
    pub use crate::three_d::{
        CuboidVisual3d, CuboidVisualError, ResolvedCuboid3d, ThreeDExtractionError,
        ThreeDLimitResource, ThreeDRenderLimits, ThreeDSnapshot, View3d, View3dError,
    };
    pub use crate::time::{DroppedFixedTime, FixedFramePlan, FixedTime, FrameTime, TimeConfig};
    pub use crate::transition::WorldReplacementOnPress;
    pub use crate::visual::{
        ActiveCamera2d, CircleVisual, LineVisual, RectangleVisual, Transform2d, VisualValueError,
        WorldBackground,
    };
    pub use crate::world::{WorldBuildError, WorldBuilder};
    #[cfg(feature = "easter-eggs")]
    pub use crate::{CrabDrawError, draw_crab};
    pub use crate::{
        ExtractedFrame, ExtractionError, ResolvedCircle, ResolvedLine, ResolvedRectangle,
        ResolvedScreenImage, ResolvedScreenRectangle, ScreenDraw,
    };
}

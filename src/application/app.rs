//! Application configuration and immutable registry construction.

use std::{error::Error, fmt, sync::Arc};

use bevy_ecs::{component::Component, entity_disabling::Disabled, system::SystemParamFunction};

use crate::{
    collision::{CircleCollider2d, RectangleCollider2d},
    component::{ComponentApprovalError, ComponentRegistry, ComponentTuple},
    events::EventRegistry,
    headless::{HeadlessRunner, RunnerBuildError},
    identity::{ApplicationId, IdentityExhausted, WorldFactoryId},
    input::{
        Action, ActionBindings, DigitalAxis2d, DuplicateKeyBinding, DuplicateMouseBinding,
        MouseButton, PhysicalKeyCode,
    },
    motion::{
        CameraFollowTarget2d, DigitalMovement2d, LinearAcceleration2d, LinearVelocity2d,
        follow_camera_target2d, integrate_digital_movement2d, integrate_linear_acceleration2d,
        integrate_linear_velocity2d,
    },
    render::RenderLimits,
    resources::{ApplicationResourceError, ApplicationResourceRegistry},
    screen::ScreenRectangleVisual,
    system::{
        Stage, StageFactories, SupportedSystemParamTuple, register_system_application_resources,
        register_system_events,
    },
    time::TimeConfig,
    transition::replace_world_on_press,
    visual::{ActiveCamera2d, CircleVisual, LineVisual, RectangleVisual, Transform2d},
    world::{WorldBuildError, WorldBuilder},
};

/// Default maximum number of managed entities in one active World.
pub const DEFAULT_ENTITY_LIMIT: usize = 100_000;

/// Default maximum structural, transition, and application-control commands
/// in one stage.
pub const DEFAULT_COMMAND_LIMIT: usize = 4_096;

/// Default maximum number of events of one type in one stage invocation.
pub const DEFAULT_EVENT_LIMIT: usize = 1_024;

/// Default maximum retained WorldEnter/WorldExit diagnostic records.
pub const DEFAULT_LIFECYCLE_TRACE_LIMIT: usize = 1_024;

/// Default maximum number of distinct Application Resource types.
pub const DEFAULT_APPLICATION_RESOURCE_LIMIT: usize = 256;

/// Complete frozen configuration for one Sim;Logic application.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppConfig {
    time: TimeConfig,
    entity_limit: usize,
    command_limit: usize,
    event_limit: usize,
    input_event_limit: usize,
    lifecycle_trace_limit: usize,
    application_resource_limit: usize,
    render: RenderLimits,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            time: TimeConfig::default(),
            entity_limit: DEFAULT_ENTITY_LIMIT,
            command_limit: DEFAULT_COMMAND_LIMIT,
            event_limit: DEFAULT_EVENT_LIMIT,
            input_event_limit: crate::input::DEFAULT_INPUT_EVENT_LIMIT,
            lifecycle_trace_limit: DEFAULT_LIFECYCLE_TRACE_LIMIT,
            application_resource_limit: DEFAULT_APPLICATION_RESOURCE_LIMIT,
            render: RenderLimits::default(),
        }
    }
}

impl AppConfig {
    /// Replaces the fixed-step time configuration.
    pub fn set_time(&mut self, time: TimeConfig) -> &mut Self {
        self.time = time;
        self
    }

    /// Replaces the maximum managed entity count.
    pub fn set_entity_limit(&mut self, limit: usize) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroEntityLimit);
        }
        self.entity_limit = limit;
        Ok(self)
    }

    /// Replaces the maximum combined command count for one stage.
    pub fn set_command_limit(&mut self, limit: usize) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroCommandLimit);
        }
        self.command_limit = limit;
        Ok(self)
    }

    /// Replaces the maximum event count for each type in one stage invocation.
    pub fn set_event_limit(&mut self, limit: usize) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroEventLimit);
        }
        self.event_limit = limit;
        Ok(self)
    }

    /// Replaces the maximum physical events per frame, generated logical edges
    /// per frame, and retained logical edges for a later FixedUpdate delivery.
    ///
    /// A frame exceeding any bound is rejected before input state changes.
    /// Generated edges include synthetic mouse releases on pointer leave;
    /// their frame bound still applies while fixed updates are paused.
    pub fn set_input_event_limit(&mut self, limit: usize) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroInputEventLimit);
        }
        self.input_event_limit = limit;
        Ok(self)
    }

    /// Replaces the maximum retained WorldEnter/WorldExit diagnostic records.
    ///
    /// Once full, the trace evicts its oldest record before retaining a new
    /// one. This affects diagnostics only, never World lifecycle execution.
    pub fn set_lifecycle_trace_limit(&mut self, limit: usize) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroLifecycleTraceLimit);
        }
        self.lifecycle_trace_limit = limit;
        Ok(self)
    }

    /// Replaces the maximum number of distinct Application Resource types.
    ///
    /// This bounds registry entries, not heap memory retained inside arbitrary
    /// user values.
    pub fn set_application_resource_limit(
        &mut self,
        limit: usize,
    ) -> Result<&mut Self, AppConfigError> {
        if limit == 0 {
            return Err(AppConfigError::ZeroApplicationResourceLimit);
        }
        self.application_resource_limit = limit;
        Ok(self)
    }

    /// Replaces renderer-independent extraction and presentation limits.
    pub fn set_render_limits(&mut self, limits: RenderLimits) -> &mut Self {
        self.render = limits;
        self
    }

    pub(crate) const fn time(self) -> TimeConfig {
        self.time
    }

    pub(crate) const fn entity_limit(self) -> usize {
        self.entity_limit
    }

    pub(crate) const fn command_limit(self) -> usize {
        self.command_limit
    }

    pub(crate) const fn event_limit(self) -> usize {
        self.event_limit
    }

    pub(crate) const fn input_event_limit(self) -> usize {
        self.input_event_limit
    }

    pub(crate) const fn lifecycle_trace_limit(self) -> usize {
        self.lifecycle_trace_limit
    }

    pub(crate) const fn application_resource_limit(self) -> usize {
        self.application_resource_limit
    }

    pub(crate) const fn render(self) -> RenderLimits {
        self.render
    }
}

/// Invalid finite application limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppConfigError {
    /// An active World must permit at least one entity.
    ZeroEntityLimit,
    /// A stage must permit at least one command.
    ZeroCommandLimit,
    /// A stage must permit at least one event of each registered type.
    ZeroEventLimit,
    /// A frame must permit at least one physical input event.
    ZeroInputEventLimit,
    /// A lifecycle trace must retain at least its newest record.
    ZeroLifecycleTraceLimit,
    /// An Application must permit at least one persistent resource type.
    ZeroApplicationResourceLimit,
}

impl fmt::Display for AppConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroEntityLimit => formatter.write_str("entity limit must be positive"),
            Self::ZeroCommandLimit => formatter.write_str("command limit must be positive"),
            Self::ZeroEventLimit => formatter.write_str("event limit must be positive"),
            Self::ZeroInputEventLimit => formatter.write_str("input event limit must be positive"),
            Self::ZeroLifecycleTraceLimit => {
                formatter.write_str("lifecycle trace limit must be positive")
            }
            Self::ZeroApplicationResourceLimit => {
                formatter.write_str("Application Resource limit must be positive")
            }
        }
    }
}

impl Error for AppConfigError {}

/// Failure to create an application runtime identity or its standard registry.
#[derive(Debug)]
pub enum ApplicationCreationError {
    /// The process-wide application identity counter is exhausted.
    Identity(IdentityExhausted),
    /// A built-in component unexpectedly violates the managed component rules.
    StandardComponent(ComponentApprovalError),
}

impl fmt::Display for ApplicationCreationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Identity(error) => error.fmt(formatter),
            Self::StandardComponent(error) => {
                write!(formatter, "standard component approval failed: {error}")
            }
        }
    }
}

impl Error for ApplicationCreationError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Identity(error) => Some(error),
            Self::StandardComponent(error) => Some(error),
        }
    }
}

/// Failure to register a no-payload World factory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorldRegistrationError {
    /// Diagnostic route names must contain a non-whitespace character.
    EmptyName,
    /// Diagnostic route names are bounded to 128 UTF-8 bytes.
    NameTooLong,
    /// Another factory already uses this exact diagnostic route name.
    DuplicateName {
        /// Rejected duplicate name.
        name: String,
    },
    /// The runtime registry cannot represent another factory slot.
    RegistryExhausted,
}

impl fmt::Display for WorldRegistrationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyName => formatter.write_str("World factory name must not be empty"),
            Self::NameTooLong => formatter.write_str("World factory name exceeds 128 UTF-8 bytes"),
            Self::DuplicateName { name } => {
                write!(
                    formatter,
                    "World factory name `{name}` is already registered"
                )
            }
            Self::RegistryExhausted => formatter.write_str("World factory registry is exhausted"),
        }
    }
}

impl Error for WorldRegistrationError {}

pub(crate) type WorldFactory =
    dyn Fn(&mut WorldBuilder) -> Result<(), WorldBuildError> + Send + Sync + 'static;

pub(crate) struct RegisteredWorldFactory {
    pub(crate) id: WorldFactoryId,
    pub(crate) diagnostic_name: String,
    pub(crate) factory: Arc<WorldFactory>,
}

/// Mutable pre-startup builder for one typed Sim;Logic application.
pub struct Application<A: Action> {
    pub(crate) application: ApplicationId,
    pub(crate) config: AppConfig,
    pub(crate) components: ComponentRegistry,
    pub(crate) events: EventRegistry,
    pub(crate) application_resources: ApplicationResourceRegistry,
    pub(crate) bindings: ActionBindings<A>,
    pub(crate) factories: Vec<RegisteredWorldFactory>,
    pub(crate) startup: StageFactories,
    pub(crate) fixed: StageFactories,
    pub(crate) frame: StageFactories,
}

impl<A: Action> Application<A> {
    /// Creates an application builder and approves Sim;Logic's standard
    /// world and screen visual, collision, motion, and disabled components,
    /// including their complete required-component closure.
    pub fn new(config: AppConfig) -> Result<Self, ApplicationCreationError> {
        let application = ApplicationId::issue().map_err(ApplicationCreationError::Identity)?;
        let mut components = ComponentRegistry::default();
        components
            .approve::<Transform2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<Disabled>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<CircleVisual>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<RectangleVisual>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<LineVisual>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<ScreenRectangleVisual>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<ActiveCamera2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<CircleCollider2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<RectangleCollider2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<LinearVelocity2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<LinearAcceleration2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<DigitalMovement2d<A>>()
            .map_err(ApplicationCreationError::StandardComponent)?;
        components
            .approve::<CameraFollowTarget2d>()
            .map_err(ApplicationCreationError::StandardComponent)?;

        Ok(Self {
            application,
            config,
            components,
            events: EventRegistry::default(),
            application_resources: ApplicationResourceRegistry::default(),
            bindings: ActionBindings::new(),
            factories: Vec::new(),
            startup: StageFactories::default(),
            fixed: StageFactories::default(),
            frame: StageFactories::default(),
        })
    }

    /// Approves one hook-free component type for candidate and command bundles.
    pub fn approve_component<T: Component>(&mut self) -> Result<&mut Self, ComponentApprovalError> {
        self.components.approve::<T>()?;
        Ok(self)
    }

    /// Approves a flat tuple of one through fifteen hook-free component types.
    ///
    /// The registry update is atomic: if one tuple member is rejected, types
    /// newly added by this call are removed again. Components that were
    /// already approved before the call remain approved. Required components
    /// must still be listed explicitly, in any order, and are checked when the
    /// application creates a World candidate.
    pub fn approve_components<T: ComponentTuple>(
        &mut self,
    ) -> Result<&mut Self, ComponentApprovalError> {
        self.components.approve_tuple::<T>()?;
        Ok(self)
    }

    /// Binds one portable physical key to a typed action.
    pub fn bind_key(
        &mut self,
        key: PhysicalKeyCode,
        action: A,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bindings.bind(key, action)?;
        Ok(self)
    }

    /// Binds one portable mouse button to a typed action without replacing an
    /// existing binding. Bindings freeze when the application is started.
    ///
    /// Mouse buttons and keys may share an action: it remains held until all
    /// bound physical controls release. Their individual edges remain distinct.
    pub fn bind_mouse_button(
        &mut self,
        button: MouseButton,
        action: A,
    ) -> Result<&mut Self, DuplicateMouseBinding> {
        self.bindings.bind_mouse_button(button, action)?;
        Ok(self)
    }

    /// Atomically binds W/A/S/D to one typed two-dimensional action axis.
    ///
    /// W maps to positive y, A to negative x, S to negative y, and D to
    /// positive x. All four physical keys are checked first in W/A/S/D order.
    /// If any is already bound, that binding is reported and the complete
    /// table remains unchanged. Repeating one logical action in several axis
    /// slots is allowed, matching [`DigitalAxis2d`] sampling semantics.
    pub fn bind_wasd(&mut self, axis: DigitalAxis2d<A>) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bindings.bind_wasd(axis)?;
        Ok(self)
    }

    /// Atomically binds the physical arrow keys to one typed action axis.
    ///
    /// Left maps to negative x, Right to positive x, Down to negative y, and
    /// Up to positive y. All four keys are checked first in that order. If any
    /// is already bound, that binding is reported and the complete table
    /// remains unchanged. The same axis may be bound to both arrows and W/A/S/D.
    pub fn bind_arrows(
        &mut self,
        axis: DigitalAxis2d<A>,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bindings.bind_arrows(axis)?;
        Ok(self)
    }

    /// Atomically binds W/A/S/D and the arrow keys to one typed action axis.
    ///
    /// The mappings match [`Application::bind_wasd`] and
    /// [`Application::bind_arrows`]. All eight keys are checked before any
    /// binding is written, in W, A, S, D, Left, Right, Down, Up order. If any
    /// key is already bound, that binding is reported and the complete table
    /// remains unchanged. Calling this method again reports W as the first
    /// conflict. Use the separate presets when only one key family should be
    /// accepted.
    pub fn bind_wasd_and_arrows(
        &mut self,
        axis: DigitalAxis2d<A>,
    ) -> Result<&mut Self, DuplicateKeyBinding> {
        self.bindings.bind_wasd_and_arrows(axis)?;
        Ok(self)
    }

    /// Registers one typed value owned by the Application rather than a World.
    ///
    /// The value becomes available through [`crate::resources::AppRes`] and
    /// [`crate::resources::AppResMut`] in FixedUpdate and FrameUpdate, and the
    /// same value survives successful World replacement without cloning.
    /// Candidate factories and Startup systems cannot access it.
    ///
    /// Registration is explicit, accepts at most one value of each concrete
    /// type, and is bounded by [`AppConfig::set_application_resource_limit`].
    /// A duplicate returns [`ApplicationResourceError::Duplicate`] and keeps
    /// the first value; registration never means replacement. `T` needs no
    /// Bevy `Resource` derive.
    ///
    /// ```no_run
    /// use sim_logic::prelude::*;
    ///
    /// #[derive(Clone, Copy, PartialEq, Eq, Hash)]
    /// enum Action {}
    ///
    /// #[derive(Default)]
    /// struct SessionScore(u32);
    ///
    /// fn award(mut score: AppResMut<SessionScore>) {
    ///     score.0 = score.0.saturating_add(1);
    /// }
    ///
    /// # fn main() -> LogicResult {
    /// let mut app = Application::<Action>::new(AppConfig::default())?;
    /// app.register_app_resource(SessionScore::default())?;
    /// app.add_system(Stage::FixedUpdate, award);
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_app_resource<T: Send + Sync + 'static>(
        &mut self,
        resource: T,
    ) -> Result<&mut Self, ApplicationResourceError> {
        self.application_resources
            .insert(resource, self.config.application_resource_limit())?;
        Ok(self)
    }

    /// Registers one repeatable synchronous no-payload World factory.
    ///
    /// A factory is a restricted construction recipe, not an effect callback.
    /// It must derive the same candidate from its immutable captures, mutate
    /// only the supplied [`WorldBuilder`], and finish before returning. It must
    /// not perform I/O, start background work, or mutate shared external state.
    /// The runtime can discard an isolated candidate, but it cannot roll back
    /// effects performed outside that candidate.
    pub fn register_world(
        &mut self,
        diagnostic_name: impl Into<String>,
        factory: impl Fn(&mut WorldBuilder) -> Result<(), WorldBuildError> + Send + Sync + 'static,
    ) -> Result<WorldFactoryId, WorldRegistrationError> {
        let diagnostic_name = diagnostic_name.into();
        if diagnostic_name.trim().is_empty() {
            return Err(WorldRegistrationError::EmptyName);
        }
        if diagnostic_name.len() > 128 {
            return Err(WorldRegistrationError::NameTooLong);
        }
        if self
            .factories
            .iter()
            .any(|registered| registered.diagnostic_name == diagnostic_name)
        {
            return Err(WorldRegistrationError::DuplicateName {
                name: diagnostic_name,
            });
        }
        let slot = u32::try_from(self.factories.len())
            .map_err(|_| WorldRegistrationError::RegistryExhausted)?;
        let id = WorldFactoryId::new(self.application, slot, 1);
        self.factories.push(RegisteredWorldFactory {
            id,
            diagnostic_name,
            factory: Arc::new(factory),
        });
        Ok(id)
    }

    /// Registers an infallible System in the FixedUpdate stage.
    ///
    /// This is the short form of `add_system(Stage::FixedUpdate, system)`.
    /// Both forms append to the same ordered stage list.
    pub fn add_fixed_system<FunctionMarker: 'static, S>(&mut self, system: S) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = ()> + Clone + Send + Sync + 'static,
        S::Param: SupportedSystemParamTuple,
    {
        self.add_system(Stage::FixedUpdate, system)
    }

    /// Registers a fallible System in the FixedUpdate stage.
    ///
    /// This is the short form of
    /// `add_fallible_system(Stage::FixedUpdate, system)`. Both forms append to
    /// the same ordered stage list and use the same failure semantics.
    pub fn add_fallible_fixed_system<FunctionMarker: 'static, S, E>(
        &mut self,
        system: S,
    ) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = Result<(), E>>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: SupportedSystemParamTuple,
        E: fmt::Display + 'static,
    {
        self.add_fallible_system(Stage::FixedUpdate, system)
    }

    /// Appends the standard fixed-press World-replacement System.
    ///
    /// In a World containing
    /// [`WorldReplacementOnPress<A>`](crate::transition::WorldReplacementOnPress), every matching
    /// press occurrence queues one ordinary deferred replacement request with
    /// that edge's original causal token. A World without the Resource, or a
    /// fixed tick without a matching press, is a successful no-op. Physical
    /// key bindings remain explicit and the Resource is inert until this
    /// method is called. Presses received while fixed simulation is paused are
    /// frame-only input and are not replayed through this adapter on unpause.
    ///
    /// The adapter runs at this exact point in FixedUpdate registration order.
    /// Later Systems in the same tick still run before the stage barrier. An
    /// accepted replacement then follows the existing arbitration, candidate
    /// preparation, lifecycle, catch-up stopping, input consumption, and
    /// same-frame extraction rules. Every occurrence consumes one command
    /// slot. Each duplicate registration runs at its own position and consumes
    /// another slot. If no intervening System changes the route, the repeated
    /// token and target coalesce idempotently; otherwise ordinary arbitration
    /// decides the differently routed requests.
    ///
    /// This helper provides one unconditional route per World. It does not bind
    /// a key, observe held or release state, request application exit, build a
    /// route table, attach a payload, or add a FrameUpdate counterpart.
    pub fn add_world_replacement_on_press_system(&mut self) -> &mut Self {
        self.add_fallible_fixed_system(replace_world_on_press::<A>)
    }

    /// Appends the standard fixed-step linear-acceleration System.
    ///
    /// Each enabled managed entity with [`LinearAcceleration2d`] changes its
    /// [`LinearVelocity2d`] by `acceleration * fixed_delta` at this exact point
    /// in registration order. The component is inert until this method is
    /// called. Exact zero acceleration is a no-op before arithmetic. Calling
    /// this method twice applies acceleration twice per fixed tick.
    ///
    /// Register this before [`Self::add_linear_velocity2d_system`] for
    /// semi-implicit Euler motion. Reversing the calls deliberately moves with
    /// the old velocity before updating it for the next step. Direct component
    /// writes by earlier Systems are visible; deferred spawn or insert Commands
    /// first accelerate on a later fixed tick. Arithmetic overflow leaves the
    /// failing entity's velocity unchanged; entities processed earlier by the
    /// same System may already have changed under ordinary non-transactional
    /// fallible-System semantics.
    ///
    /// This adapter does not represent force or mass, move a Transform itself,
    /// or apply gravity, drag, collision response, rotation, or world bounds.
    pub fn add_linear_acceleration2d_system(&mut self) -> &mut Self {
        self.add_fallible_fixed_system(integrate_linear_acceleration2d)
    }

    /// Appends the standard fixed-step linear-velocity System.
    ///
    /// Each enabled managed entity with [`LinearVelocity2d`] moves by
    /// `velocity * fixed_delta` at this exact point in registration order.
    /// The component itself is inert until this method is called. Calling the
    /// method twice appends two Systems and therefore integrates twice per
    /// fixed tick, just like registering an ordinary System twice.
    ///
    /// Direct component writes by earlier Systems are visible to this System,
    /// and later Systems observe the integrated translation. Deferred spawn or
    /// insert Commands become visible only after the stage barrier, so newly
    /// moving entities first integrate on a later fixed tick. Arithmetic
    /// overflow follows normal fallible-System semantics and may leave earlier
    /// entities in the same query already moved.
    ///
    /// This adapter does not apply acceleration, forces, collision response,
    /// rotation, drag, or world bounds.
    pub fn add_linear_velocity2d_system(&mut self) -> &mut Self {
        self.add_fallible_fixed_system(integrate_linear_velocity2d)
    }

    /// Appends the standard fixed-step digital-movement System.
    ///
    /// Each enabled managed entity with [`DigitalMovement2d<A>`] samples its
    /// typed axis, normalizes the direction, and moves by `speed * fixed_delta`
    /// at this exact point in registration order. The component is inert until
    /// this method is called, and physical keys must be bound separately.
    /// Calling the method twice appends two Systems and moves twice per tick.
    ///
    /// Direct component writes by earlier Systems are visible here; later
    /// Systems observe the integrated translation. Deferred spawn or insert
    /// Commands become visible only after the stage barrier. If this and the
    /// linear-velocity adapter are both registered, their Transform writes
    /// compose in explicit registration order. Idle or exactly opposed input
    /// is a no-op before speed/time multiplication; active arithmetic overflow
    /// follows ordinary fallible-System semantics.
    ///
    /// This adapter does not apply collision response, acceleration, facing,
    /// analog input, smoothing, or world bounds.
    pub fn add_digital_movement2d_system(&mut self) -> &mut Self {
        self.add_fallible_fixed_system(integrate_digital_movement2d::<A>)
    }

    /// Appends the standard fixed-step camera-follow System.
    ///
    /// With no enabled [`CameraFollowTarget2d`], this System is a successful
    /// no-op so other Worlds in the application need no follow target. With
    /// exactly one target and one enabled [`ActiveCamera2d`], it replaces the
    /// canonical camera center with `target translation + camera offset` at
    /// this exact point in registration order. More than one target fails the
    /// stage before changing a camera. Missing or duplicate active cameras are
    /// left unchanged for the extraction validator and its repair path.
    ///
    /// Register this after target movement for same-tick following, or before
    /// movement to deliberately use the previous target position. Deferred
    /// spawn or insert Commands first affect following on a later fixed tick;
    /// Disabled targets are ignored. Arithmetic overflow leaves the camera
    /// unchanged. Duplicate registration performs another ordinary assignment
    /// and can observe writes placed between the two adapters.
    ///
    /// The adapter does not run during World preparation, create or initially
    /// align a camera, infer teleports, smooth motion, clamp bounds, change zoom
    /// or rotation, or choose among multiple targets.
    pub fn add_camera_follow2d_system(&mut self) -> &mut Self {
        self.add_fallible_fixed_system(follow_camera_target2d)
    }

    /// Registers an infallible System in the FrameUpdate stage.
    ///
    /// This is the short form of `add_system(Stage::FrameUpdate, system)`.
    /// Both forms append to the same ordered stage list.
    pub fn add_frame_system<FunctionMarker: 'static, S>(&mut self, system: S) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = ()> + Clone + Send + Sync + 'static,
        S::Param: SupportedSystemParamTuple,
    {
        self.add_system(Stage::FrameUpdate, system)
    }

    /// Registers a fallible System in the FrameUpdate stage.
    ///
    /// This is the short form of
    /// `add_fallible_system(Stage::FrameUpdate, system)`. Both forms append to
    /// the same ordered stage list and use the same failure semantics.
    pub fn add_fallible_frame_system<FunctionMarker: 'static, S, E>(
        &mut self,
        system: S,
    ) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = Result<(), E>>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: SupportedSystemParamTuple,
        E: fmt::Display + 'static,
    {
        self.add_fallible_system(Stage::FrameUpdate, system)
    }

    /// Registers a recreatable System in explicit execution order.
    ///
    /// The supplied function or closure must be cloneable because every World
    /// generation receives a fresh Bevy System instance.
    /// Captured values shared through interior mutability remain external to a
    /// candidate and therefore are not rolled back when preparation fails.
    ///
    /// Only the managed parameter set documented by
    /// [`SupportedSystemParamTuple`] is accepted. In particular, raw Bevy
    /// `Query` or `Single`, raw `Entity`, Bevy `Commands`, raw World access,
    /// `Local<T>`, and arbitrary custom SystemParams are rejected at compile
    /// time instead of running their initialization code against a candidate.
    pub fn add_system<FunctionMarker: 'static, S>(&mut self, stage: Stage, system: S) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = ()> + Clone + Send + Sync + 'static,
        S::Param: SupportedSystemParamTuple,
    {
        register_system_events::<S::Param>(&mut self.events);
        if stage != Stage::Startup {
            register_system_application_resources::<S::Param>(
                &mut self.application_resources,
                stage,
                std::any::type_name::<S>(),
            );
        }
        match stage {
            Stage::Startup => self.startup.add(system),
            Stage::FixedUpdate => self.fixed.add(system),
            Stage::FrameUpdate => self.frame.add(system),
        }
        self
    }

    /// Registers a recreatable System that can stop its stage with an error.
    ///
    /// Returning `Err` skips the remaining Systems in the current stage and
    /// discards every [`crate::commands::LogicCommands`] operation queued by
    /// that stage. It is not a transaction: direct component and Resource
    /// mutations performed before the error remain in an active World. On a
    /// failed fixed tick, the runtime restores only the previous interpolation
    /// endpoints; current [`Transform2d`] and [`ActiveCamera2d`] values remain
    /// mutated.
    ///
    /// A Startup error instead discards the complete isolated World candidate.
    /// The error is formatted only on the failing path and is exposed through
    /// [`crate::system::SystemRunFailure`]. Panics are outside this typed error
    /// contract.
    ///
    /// The same parameter and recreation restrictions as [`Self::add_system`]
    /// apply.
    pub fn add_fallible_system<FunctionMarker: 'static, S, E>(
        &mut self,
        stage: Stage,
        system: S,
    ) -> &mut Self
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = Result<(), E>>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: SupportedSystemParamTuple,
        E: fmt::Display + 'static,
    {
        register_system_events::<S::Param>(&mut self.events);
        if stage != Stage::Startup {
            register_system_application_resources::<S::Param>(
                &mut self.application_resources,
                stage,
                std::any::type_name::<S>(),
            );
        }
        match stage {
            Stage::Startup => self.startup.add_fallible(system),
            Stage::FixedUpdate => self.fixed.add_fallible(system),
            Stage::FrameUpdate => self.frame.add_fallible(system),
        }
        self
    }

    /// Freezes the registries, prepares `initial`, and creates the shared
    /// headless runtime core.
    pub fn build_headless(
        self,
        initial: WorldFactoryId,
    ) -> Result<HeadlessRunner<A>, RunnerBuildError> {
        HeadlessRunner::from_application(self, initial)
    }

    /// Runs the one-window desktop adapter until the window closes or a System
    /// commits [`crate::commands::LogicCommands::request_exit`].
    ///
    /// Call this on the process main thread. Platform event-loop rules may
    /// permit creating only one event loop for the lifetime of the process.
    #[cfg(feature = "desktop")]
    pub fn run_desktop(
        self,
        initial: WorldFactoryId,
        config: crate::desktop::DesktopConfig,
    ) -> Result<crate::desktop::DesktopRunReport, crate::desktop::DesktopRunError> {
        crate::desktop::run(self, initial, config)
    }

    /// Runs one desktop window with the default title, size, and VSync mode
    /// until the window closes or a System requests exit.
    ///
    /// Call this on the process main thread. Platform event-loop rules may
    /// permit creating only one event loop for the lifetime of the process.
    #[cfg(feature = "desktop")]
    pub fn run(
        self,
        initial: WorldFactoryId,
    ) -> Result<crate::desktop::DesktopRunReport, crate::desktop::DesktopRunError> {
        self.run_desktop(initial, crate::desktop::DesktopConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AppConfig, AppConfigError, Application, DEFAULT_APPLICATION_RESOURCE_LIMIT,
        DEFAULT_EVENT_LIMIT,
    };
    use crate::input::{DigitalAxis2d, PhysicalKeyCode};

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum BindingAction {
        Left,
        Right,
        Down,
        Up,
        Other,
    }

    #[test]
    fn event_limit_is_positive_and_rejected_updates_are_atomic() {
        let mut config = AppConfig::default();
        assert_eq!(config.event_limit(), DEFAULT_EVENT_LIMIT);
        assert!(matches!(
            config.set_event_limit(0),
            Err(AppConfigError::ZeroEventLimit)
        ));
        assert_eq!(config.event_limit(), DEFAULT_EVENT_LIMIT);
        assert!(config.set_event_limit(7).is_ok());
        assert_eq!(config.event_limit(), 7);
    }

    #[test]
    fn application_resource_limit_is_positive_and_rejected_updates_are_atomic() {
        let mut config = AppConfig::default();
        assert_eq!(
            config.application_resource_limit(),
            DEFAULT_APPLICATION_RESOURCE_LIMIT
        );
        assert!(matches!(
            config.set_application_resource_limit(0),
            Err(AppConfigError::ZeroApplicationResourceLimit)
        ));
        assert_eq!(
            config.application_resource_limit(),
            DEFAULT_APPLICATION_RESOURCE_LIMIT
        );
        assert!(config.set_application_resource_limit(7).is_ok());
        assert_eq!(config.application_resource_limit(), 7);
    }

    #[test]
    fn public_combined_binding_does_not_install_wasd_before_an_arrow_conflict() {
        let axis = DigitalAxis2d::new(
            BindingAction::Left,
            BindingAction::Right,
            BindingAction::Down,
            BindingAction::Up,
        );
        let mut application = Application::new(AppConfig::default()).unwrap();
        assert!(
            application
                .bind_key(PhysicalKeyCode::ArrowLeft, BindingAction::Other)
                .is_ok()
        );
        assert!(
            application
                .bind_key(PhysicalKeyCode::Escape, BindingAction::Other)
                .is_ok()
        );

        let combined_error = match application.bind_wasd_and_arrows(axis) {
            Ok(_) => panic!("occupied arrow should reject the combined preset"),
            Err(error) => error,
        };
        assert_eq!(combined_error.key(), PhysicalKeyCode::ArrowLeft);
        assert!(
            application
                .bind_key(PhysicalKeyCode::KeyW, BindingAction::Up)
                .is_ok()
        );
        let escape_error = match application.bind_key(PhysicalKeyCode::Escape, BindingAction::Up) {
            Ok(_) => panic!("the unrelated Escape binding should remain installed"),
            Err(error) => error,
        };
        assert_eq!(escape_error.key(), PhysicalKeyCode::Escape);
    }
}

//! Sequential, per-World System construction and execution.

use std::{error::Error, fmt, marker::PhantomData, sync::Arc};

use bevy_ecs::{
    component::{Component, Mutable},
    query::{QueryData, QueryFilter},
    resource::Resource,
    system::{BoxedSystem, IntoSystem, IsFunctionSystem, Res, ResMut, SystemParamFunction},
    world::World,
};

use crate::{
    collision::{CircleOverlapEntities, RectangleOverlapEntities},
    commands::LogicCommands,
    events::{EventReader, EventRegistry, EventWriter, WorldEvent, event_stage_failure},
    identity::LogicEntityRef,
    input::{Action, FixedInput, FrameInput},
    query::{Query, Single},
    render::FrameViewport,
    resources::{AppRes, AppResMut, ApplicationResourceRegistry},
    time::{FixedTime, FrameTime},
    visual::{ActiveCamera2d, Transform2d},
};

mod supported {
    use crate::{events::EventRegistry, resources::ApplicationResourceRegistry};

    pub trait QueryData {}
    #[allow(private_interfaces)]
    pub trait Param {
        fn register_events(_registry: &mut EventRegistry) {}
        fn register_application_resources(
            _registry: &mut ApplicationResourceRegistry,
            _stage: super::Stage,
            _system: &'static str,
        ) {
        }
        fn writes_events() -> bool {
            false
        }
        fn uses_application_resources() -> bool {
            false
        }
    }
    #[allow(private_interfaces)]
    pub trait Tuple {
        fn register_events(registry: &mut EventRegistry);
        fn register_application_resources(
            registry: &mut ApplicationResourceRegistry,
            stage: super::Stage,
            system: &'static str,
        );
        fn writes_events() -> bool;
        fn uses_application_resources() -> bool;
    }
}

/// Compile-time marker for the System-parameter tuples accepted by the managed
/// first-slice scheduler.
///
/// Sim;Logic implements this sealed trait for generation-checked
/// [`crate::query::Query`], [`crate::query::Single`],
/// [`crate::collision::CircleOverlapEntities`],
/// [`crate::collision::RectangleOverlapEntities`], `Res`, `ResMut`,
/// [`crate::resources::AppRes`], [`crate::resources::AppResMut`], its own
/// read-only frame parameters, and [`LogicCommands`]. Raw Bevy `Query` or
/// `Single`, raw `Entity`, and parameters with
/// initialization-time World access, including Bevy `Local<T: FromWorld>`, are
/// intentionally excluded. Typed event parameters register their channels
/// automatically when a System is added. Application Resource parameters
/// register a required type and are forbidden in Startup.
pub trait SupportedSystemParamTuple: supported::Tuple {}

impl<T: supported::Tuple> SupportedSystemParamTuple for T {}

impl supported::QueryData for () {}
impl<T: Component> supported::QueryData for &T {}
impl<T: Component> supported::QueryData for &mut T {}
impl supported::QueryData for LogicEntityRef {}
impl<D> supported::QueryData for Option<D> where D: QueryData + supported::QueryData {}

macro_rules! impl_supported_query_tuple {
    ($($parameter:ident),+) => {
        impl<$($parameter),+> supported::QueryData for ($($parameter,)+)
        where
            $($parameter: QueryData + supported::QueryData,)+
        {}
    };
}

impl_supported_query_tuple!(P0);
impl_supported_query_tuple!(P0, P1);
impl_supported_query_tuple!(P0, P1, P2);
impl_supported_query_tuple!(P0, P1, P2, P3);
impl_supported_query_tuple!(P0, P1, P2, P3, P4);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12);
impl_supported_query_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13);
impl_supported_query_tuple!(
    P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14
);
impl_supported_query_tuple!(
    P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15
);

impl<D, F> supported::Param for Query<'_, '_, D, F>
where
    D: QueryData + supported::QueryData,
    F: QueryFilter,
{
}
impl<D, F> supported::Param for Single<'_, '_, D, F>
where
    D: bevy_ecs::query::IterQueryData + supported::QueryData,
    F: QueryFilter,
{
}
impl<R: Resource> supported::Param for Res<'_, R> {}
impl<R: Resource<Mutability = Mutable>> supported::Param for ResMut<'_, R> {}
impl<R: Resource> supported::Param for Option<Res<'_, R>> {}
impl<R: Resource<Mutability = Mutable>> supported::Param for Option<ResMut<'_, R>> {}
#[allow(
    private_interfaces,
    reason = "this implementation registers a private resource requirement through a sealed hook"
)]
impl<T: Send + Sync + 'static> supported::Param for AppRes<'_, T> {
    fn register_application_resources(
        registry: &mut ApplicationResourceRegistry,
        stage: Stage,
        system: &'static str,
    ) {
        registry.require::<T>(stage, system);
    }

    fn uses_application_resources() -> bool {
        true
    }
}
#[allow(
    private_interfaces,
    reason = "this implementation registers a private resource requirement through a sealed hook"
)]
impl<T: Send + Sync + 'static> supported::Param for AppResMut<'_, T> {
    fn register_application_resources(
        registry: &mut ApplicationResourceRegistry,
        stage: Stage,
        system: &'static str,
    ) {
        registry.require::<T>(stage, system);
    }

    fn uses_application_resources() -> bool {
        true
    }
}
impl<A: Action> supported::Param for FrameInput<'_, A> {}
impl<A: Action> supported::Param for FixedInput<'_, A> {}
impl<F: QueryFilter> supported::Param for CircleOverlapEntities<'_, '_, F> {}
impl<F: QueryFilter> supported::Param for RectangleOverlapEntities<'_, '_, F> {}
impl supported::Param for FrameTime<'_> {}
impl supported::Param for FixedTime<'_> {}
impl supported::Param for FrameViewport<'_> {}
impl supported::Param for LogicCommands<'_> {}
#[allow(
    private_interfaces,
    reason = "this implementation registers events through a sealed private hook"
)]
impl<E: WorldEvent> supported::Param for EventWriter<'_, E> {
    fn register_events(registry: &mut EventRegistry) {
        registry.register::<E>();
    }

    fn writes_events() -> bool {
        true
    }
}
#[allow(
    private_interfaces,
    reason = "this implementation registers events through a sealed private hook"
)]
impl<E: WorldEvent> supported::Param for EventReader<'_, E> {
    fn register_events(registry: &mut EventRegistry) {
        registry.register::<E>();
    }
}

#[allow(
    private_interfaces,
    reason = "this implementation is hidden with the sealed tuple trait"
)]
impl supported::Tuple for () {
    fn register_events(_registry: &mut EventRegistry) {}

    fn register_application_resources(
        _registry: &mut ApplicationResourceRegistry,
        _stage: Stage,
        _system: &'static str,
    ) {
    }

    fn writes_events() -> bool {
        false
    }

    fn uses_application_resources() -> bool {
        false
    }
}

macro_rules! impl_supported_param_tuple {
    ($($parameter:ident),+) => {
        #[allow(
            private_interfaces,
            reason = "this implementation is hidden with the sealed tuple trait"
        )]
        impl<$($parameter),+> supported::Tuple for ($($parameter,)+)
        where
            $($parameter: supported::Param,)+
        {
            fn register_events(registry: &mut EventRegistry) {
                $(<$parameter as supported::Param>::register_events(registry);)+
            }

            fn register_application_resources(
                registry: &mut ApplicationResourceRegistry,
                stage: Stage,
                system: &'static str,
            ) {
                $(<$parameter as supported::Param>::register_application_resources(
                    registry,
                    stage,
                    system,
                );)+
            }

            fn writes_events() -> bool {
                false $(|| <$parameter as supported::Param>::writes_events())+
            }

            fn uses_application_resources() -> bool {
                false $(|| <$parameter as supported::Param>::uses_application_resources())+
            }
        }
    };
}

impl_supported_param_tuple!(P0);
impl_supported_param_tuple!(P0, P1);
impl_supported_param_tuple!(P0, P1, P2);
impl_supported_param_tuple!(P0, P1, P2, P3);
impl_supported_param_tuple!(P0, P1, P2, P3, P4);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12);
impl_supported_param_tuple!(P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13);
impl_supported_param_tuple!(
    P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14
);
impl_supported_param_tuple!(
    P0, P1, P2, P3, P4, P5, P6, P7, P8, P9, P10, P11, P12, P13, P14, P15
);

pub(crate) fn register_system_events<P: SupportedSystemParamTuple>(registry: &mut EventRegistry) {
    <P as supported::Tuple>::register_events(registry);
}

pub(crate) fn register_system_application_resources<P: SupportedSystemParamTuple>(
    registry: &mut ApplicationResourceRegistry,
    stage: Stage,
    system: &'static str,
) {
    <P as supported::Tuple>::register_application_resources(registry, stage, system);
}

/// A user-facing System stage in the first Sim;Logic execution model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Runs once while a World candidate is still isolated.
    Startup,
    /// Runs zero or more times using the configured fixed step.
    FixedUpdate,
    /// Runs once per successful application frame when no fixed-stage
    /// transition is pending.
    FrameUpdate,
}

/// A System cannot participate in Sim;Logic's managed schedule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemSetupError {
    /// The System owns a Bevy deferred buffer such as `bevy_ecs::Commands`.
    Deferred {
        /// Bevy's diagnostic System name.
        system: String,
    },
    /// The System requests exclusive access such as `&mut bevy_ecs::World`.
    Exclusive {
        /// Bevy's diagnostic System name.
        system: String,
    },
    /// A System requests write access to interpolation-owned transforms in
    /// FrameUpdate.
    FrameTransformWrite {
        /// Bevy's diagnostic System name.
        system: String,
    },
    /// A System requests write access to the interpolation-owned camera in
    /// FrameUpdate.
    FrameCameraWrite {
        /// Bevy's diagnostic System name.
        system: String,
    },
    /// Startup requested live Application state while its World was provisional.
    StartupApplicationResourceAccess {
        /// Bevy's diagnostic System name.
        system: String,
    },
}

impl fmt::Display for SystemSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Deferred { system } => write!(
                formatter,
                "system `{system}` uses a deferred Bevy buffer; use Sim;Logic Commands"
            ),
            Self::Exclusive { system } => write!(
                formatter,
                "system `{system}` requests exclusive World access, which bypasses Sim;Logic"
            ),
            Self::FrameTransformWrite { system } => write!(
                formatter,
                "FrameUpdate system `{system}` requests mutable Transform2d access"
            ),
            Self::FrameCameraWrite { system } => write!(
                formatter,
                "FrameUpdate system `{system}` requests mutable ActiveCamera2d access"
            ),
            Self::StartupApplicationResourceAccess { system } => write!(
                formatter,
                "Startup system `{system}` requests an Application Resource; candidate Startup is isolated from live Application state"
            ),
        }
    }
}

impl Error for SystemSetupError {}

/// A typed failure returned while running one managed System.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemRunFailure {
    system: String,
    reason: String,
}

impl SystemRunFailure {
    /// Returns Bevy's diagnostic name for the failed System.
    pub fn system(&self) -> &str {
        &self.system
    }

    /// Returns the parameter-validation, execution, or user-returned failure text.
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

impl fmt::Display for SystemRunFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "system `{}` could not run: {}",
            self.system, self.reason
        )
    }
}

impl Error for SystemRunFailure {}

type ManagedSystemOutcome = Result<(), String>;
type ManagedBoxedSystem = BoxedSystem<(), ManagedSystemOutcome>;

fn completed_system((): ()) -> ManagedSystemOutcome {
    Ok(())
}

fn returned_system<E: fmt::Display>(result: Result<(), E>) -> ManagedSystemOutcome {
    result.map_err(|error| format!("returned an error: {error}"))
}

trait RecreateSystem: Send + Sync + 'static {
    fn create(&self) -> ManagedBoxedSystem;
    fn diagnostic_name(&self) -> &'static str;
    fn writes_events(&self) -> bool;
    fn uses_application_resources(&self) -> bool;
}

struct ClonedSystemFactory<S, FunctionMarker> {
    template: S,
    diagnostic_name: &'static str,
    marker: PhantomData<fn() -> FunctionMarker>,
}

impl<S, FunctionMarker> RecreateSystem for ClonedSystemFactory<S, FunctionMarker>
where
    S: SystemParamFunction<FunctionMarker, In = (), Out = ()> + Clone + Send + Sync + 'static,
    S::Param: SupportedSystemParamTuple,
    FunctionMarker: 'static,
{
    fn create(&self) -> ManagedBoxedSystem {
        let system = IntoSystem::<(), (), (IsFunctionSystem, FunctionMarker)>::into_system(
            self.template.clone(),
        );
        Box::new(IntoSystem::into_system(system.map(completed_system)))
    }

    fn diagnostic_name(&self) -> &'static str {
        self.diagnostic_name
    }

    fn writes_events(&self) -> bool {
        <S::Param as supported::Tuple>::writes_events()
    }

    fn uses_application_resources(&self) -> bool {
        <S::Param as supported::Tuple>::uses_application_resources()
    }
}

struct FallibleClonedSystemFactory<S, FunctionMarker, E> {
    template: S,
    diagnostic_name: &'static str,
    marker: PhantomData<fn() -> (FunctionMarker, E)>,
}

impl<S, FunctionMarker, E> RecreateSystem for FallibleClonedSystemFactory<S, FunctionMarker, E>
where
    S: SystemParamFunction<FunctionMarker, In = (), Out = Result<(), E>>
        + Clone
        + Send
        + Sync
        + 'static,
    S::Param: SupportedSystemParamTuple,
    FunctionMarker: 'static,
    E: fmt::Display + 'static,
{
    fn create(&self) -> ManagedBoxedSystem {
        let system =
            IntoSystem::<(), Result<(), E>, (IsFunctionSystem, FunctionMarker)>::into_system(
                self.template.clone(),
            );
        Box::new(IntoSystem::into_system(system.map(returned_system::<E>)))
    }

    fn diagnostic_name(&self) -> &'static str {
        self.diagnostic_name
    }

    fn writes_events(&self) -> bool {
        <S::Param as supported::Tuple>::writes_events()
    }

    fn uses_application_resources(&self) -> bool {
        <S::Param as supported::Tuple>::uses_application_resources()
    }
}

#[derive(Clone, Default)]
pub(crate) struct StageFactories {
    factories: Vec<Arc<dyn RecreateSystem>>,
}

impl StageFactories {
    pub(crate) fn first_application_resource_system(&self) -> Option<String> {
        self.factories
            .iter()
            .find(|factory| factory.uses_application_resources())
            .map(|factory| factory.diagnostic_name().to_owned())
    }

    pub(crate) fn add<FunctionMarker, S>(&mut self, system: S)
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = ()> + Clone + Send + Sync + 'static,
        S::Param: SupportedSystemParamTuple,
        FunctionMarker: 'static,
    {
        self.factories.push(Arc::new(ClonedSystemFactory {
            template: system,
            diagnostic_name: std::any::type_name::<S>(),
            marker: PhantomData,
        }));
    }

    pub(crate) fn add_fallible<FunctionMarker, S, E>(&mut self, system: S)
    where
        S: SystemParamFunction<FunctionMarker, In = (), Out = Result<(), E>>
            + Clone
            + Send
            + Sync
            + 'static,
        S::Param: SupportedSystemParamTuple,
        FunctionMarker: 'static,
        E: fmt::Display + 'static,
    {
        self.factories.push(Arc::new(FallibleClonedSystemFactory {
            template: system,
            diagnostic_name: std::any::type_name::<S>(),
            marker: PhantomData,
        }));
    }

    pub(crate) fn instantiate(
        &self,
        world: &mut World,
        stage: Stage,
    ) -> Result<SequentialStage, SystemSetupError> {
        let mut systems = Vec::with_capacity(self.factories.len());

        for factory in &self.factories {
            let mut system = factory.create();
            let writes_events = factory.writes_events();
            let name = system.name().to_string();

            if stage == Stage::Startup && factory.uses_application_resources() {
                return Err(SystemSetupError::StartupApplicationResourceAccess { system: name });
            }

            if system.has_deferred() {
                return Err(SystemSetupError::Deferred { system: name });
            }
            if system.is_exclusive() {
                return Err(SystemSetupError::Exclusive { system: name });
            }

            let access = system.initialize(world);
            if stage == Stage::FrameUpdate
                && world
                    .component_id::<Transform2d>()
                    .is_some_and(|component| access.combined_access().has_write(component))
            {
                return Err(SystemSetupError::FrameTransformWrite { system: name });
            }
            if stage == Stage::FrameUpdate
                && world
                    .component_id::<ActiveCamera2d>()
                    .is_some_and(|component| access.combined_access().has_write(component))
            {
                return Err(SystemSetupError::FrameCameraWrite { system: name });
            }

            systems.push(ManagedSystem {
                name,
                system,
                writes_events,
            });
        }

        let writes_events = systems.iter().any(|system| system.writes_events);
        Ok(SequentialStage {
            systems,
            writes_events,
        })
    }
}

pub(crate) struct SequentialStage {
    systems: Vec<ManagedSystem>,
    writes_events: bool,
}

struct ManagedSystem {
    name: String,
    system: ManagedBoxedSystem,
    writes_events: bool,
}

impl SequentialStage {
    pub(crate) fn is_empty(&self) -> bool {
        self.systems.is_empty()
    }

    pub(crate) const fn writes_events(&self) -> bool {
        self.writes_events
    }

    pub(crate) fn run(&mut self, world: &mut World) -> Result<(), SystemRunFailure> {
        for managed in &mut self.systems {
            let outcome = managed
                .system
                .run_without_applying_deferred((), world)
                .map_err(|error| SystemRunFailure {
                    system: managed.name.clone(),
                    reason: error.to_string(),
                })?;
            outcome.map_err(|reason| SystemRunFailure {
                system: managed.name.clone(),
                reason,
            })?;
            if !managed.writes_events {
                continue;
            }
            match event_stage_failure(world) {
                Ok(Some(failure)) => {
                    return Err(SystemRunFailure {
                        system: managed.name.clone(),
                        reason: failure.to_string(),
                    });
                }
                Ok(None) => {}
                Err(()) => {
                    return Err(SystemRunFailure {
                        system: managed.name.clone(),
                        reason: "event failure diagnostics are unavailable".to_owned(),
                    });
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::{ResMut, Resource};

    use super::*;

    #[derive(Resource, Default)]
    struct Trace(Vec<u8>);

    fn first(mut trace: ResMut<Trace>) {
        trace.0.push(1);
    }

    fn second(mut trace: ResMut<Trace>) {
        trace.0.push(2);
    }

    fn fallible_first(mut trace: ResMut<Trace>) -> Result<(), &'static str> {
        trace.0.push(1);
        Ok(())
    }

    fn fallible_stop(mut trace: ResMut<Trace>) -> Result<(), &'static str> {
        trace.0.push(2);
        Err("deliberate failure")
    }

    fn fallible_write_transform(
        mut transforms: Query<&mut Transform2d>,
    ) -> Result<(), &'static str> {
        for _transform in &mut transforms {}
        Ok(())
    }

    fn write_transform(mut transforms: Query<&mut Transform2d>) {
        for _transform in &mut transforms {}
    }

    fn write_single_transform(_transform: Single<&mut Transform2d>) {}

    fn write_camera(mut cameras: Query<&mut ActiveCamera2d>) {
        for _camera in &mut cameras {}
    }

    fn write_single_camera(_camera: Single<&mut ActiveCamera2d>) {}

    fn fallible_write_camera(mut cameras: Query<&mut ActiveCamera2d>) -> Result<(), &'static str> {
        for _camera in &mut cameras {}
        Ok(())
    }

    fn read_camera(cameras: Query<&ActiveCamera2d>) {
        for _camera in &cameras {}
    }

    #[test]
    fn executes_in_registration_order() {
        let mut factories = StageFactories::default();
        factories.add(first);
        factories.add(second);

        let mut world = World::new();
        world.insert_resource(Trace::default());
        EventRegistry::default().install(&mut world, 1);
        let stage = factories.instantiate(&mut world, Stage::FixedUpdate);
        assert!(stage.is_ok());
        let mut stage = match stage {
            Ok(stage) => stage,
            Err(error) => panic!("unexpected setup error: {error}"),
        };

        assert!(stage.run(&mut world).is_ok());
        assert_eq!(world.resource::<Trace>().0, [1, 2]);
    }

    #[test]
    fn sequential_stage_reports_whether_it_contains_any_systems() {
        let empty = StageFactories::default();
        let mut world = World::new();
        let stage = empty
            .instantiate(&mut world, Stage::FixedUpdate)
            .expect("empty stage should instantiate");
        assert!(stage.is_empty());

        let mut populated = StageFactories::default();
        populated.add(first);
        let stage = populated
            .instantiate(&mut world, Stage::FixedUpdate)
            .expect("populated stage should instantiate");
        assert!(!stage.is_empty());
    }

    #[test]
    fn fallible_error_stops_later_systems_and_retains_diagnostics() {
        let mut factories = StageFactories::default();
        factories.add_fallible(fallible_first);
        factories.add_fallible(fallible_stop);
        factories.add(second);

        let mut world = World::new();
        world.insert_resource(Trace::default());
        let stage = factories.instantiate(&mut world, Stage::FixedUpdate);
        assert!(stage.is_ok());
        let mut stage = match stage {
            Ok(stage) => stage,
            Err(error) => panic!("unexpected setup error: {error}"),
        };

        let error = stage
            .run(&mut world)
            .expect_err("second System should fail");

        assert!(
            error.system().ends_with("fallible_stop"),
            "unexpected System name: {}",
            error.system()
        );
        assert_eq!(error.reason(), "returned an error: deliberate failure");
        assert_eq!(world.resource::<Trace>().0, [1, 2]);
    }

    #[test]
    fn frame_system_transform_writes_are_rejected_at_setup() {
        let mut factories = StageFactories::default();
        factories.add(write_transform);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameTransformWrite { .. })
        ));
    }

    #[test]
    fn fallible_adapter_preserves_frame_transform_access() {
        let mut factories = StageFactories::default();
        factories.add_fallible(fallible_write_transform);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameTransformWrite { .. })
        ));
    }

    #[test]
    fn fixed_system_transform_writes_remain_supported() {
        let mut factories = StageFactories::default();
        factories.add(write_transform);
        let mut world = World::new();

        assert!(
            factories
                .instantiate(&mut world, Stage::FixedUpdate)
                .is_ok()
        );
    }

    #[test]
    fn frame_single_transform_writes_are_rejected_at_setup() {
        let mut factories = StageFactories::default();
        factories.add(write_single_transform);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameTransformWrite { .. })
        ));
    }

    #[test]
    fn fixed_single_transform_writes_remain_supported() {
        let mut factories = StageFactories::default();
        factories.add(write_single_transform);
        let mut world = World::new();

        assert!(
            factories
                .instantiate(&mut world, Stage::FixedUpdate)
                .is_ok()
        );
    }

    #[test]
    fn frame_system_camera_writes_are_rejected_at_setup() {
        let mut factories = StageFactories::default();
        factories.add(write_camera);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameCameraWrite { .. })
        ));
    }

    #[test]
    fn fixed_system_camera_writes_remain_supported() {
        let mut factories = StageFactories::default();
        factories.add(write_camera);
        let mut world = World::new();

        assert!(
            factories
                .instantiate(&mut world, Stage::FixedUpdate)
                .is_ok()
        );
    }

    #[test]
    fn frame_single_camera_writes_are_rejected_at_setup() {
        let mut factories = StageFactories::default();
        factories.add(write_single_camera);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameCameraWrite { .. })
        ));
    }

    #[test]
    fn fixed_single_camera_writes_remain_supported() {
        let mut factories = StageFactories::default();
        factories.add(write_single_camera);
        let mut world = World::new();

        assert!(
            factories
                .instantiate(&mut world, Stage::FixedUpdate)
                .is_ok()
        );
    }

    #[test]
    fn fallible_adapter_preserves_frame_camera_access() {
        let mut factories = StageFactories::default();
        factories.add_fallible(fallible_write_camera);
        let mut world = World::new();

        let result = factories.instantiate(&mut world, Stage::FrameUpdate);

        assert!(matches!(
            result,
            Err(SystemSetupError::FrameCameraWrite { .. })
        ));
    }

    #[test]
    fn frame_system_camera_reads_remain_supported() {
        let mut factories = StageFactories::default();
        factories.add(read_camera);
        let mut world = World::new();

        assert!(
            factories
                .instantiate(&mut world, Stage::FrameUpdate)
                .is_ok()
        );
    }
}

//! Frozen component approval for managed structural commands.

use std::{
    any::{TypeId, type_name},
    collections::HashSet,
    error::Error,
    fmt,
};

use bevy_ecs::{
    bundle::{Bundle, NoBundleEffect},
    component::{Component, ComponentId},
    lifecycle::{ComponentHook, HookContext},
    prelude::Resource,
    resource::IsResource,
    world::{DeferredWorld, World},
};

use crate::identity::ManagedEntity;

mod supported {
    use bevy_ecs::component::Component;

    use super::{ComponentApprovalError, ComponentRegistry};

    #[allow(
        private_interfaces,
        reason = "this method seals tuple implementations around the private registry"
    )]
    pub trait Tuple {
        fn approve(registry: &mut ComponentRegistry) -> Result<(), ComponentApprovalError>;
    }

    macro_rules! impl_component_tuple {
        ($($component:ident),+) => {
            #[allow(
                private_interfaces,
                reason = "this implementation is hidden with the sealed tuple trait"
            )]
            impl<$($component: Component),+> Tuple for ($($component,)+) {
                fn approve(
                    registry: &mut ComponentRegistry,
                ) -> Result<(), ComponentApprovalError> {
                    $(registry.approve::<$component>()?;)+
                    Ok(())
                }
            }
        };
    }

    impl_component_tuple!(C0);
    impl_component_tuple!(C0, C1);
    impl_component_tuple!(C0, C1, C2);
    impl_component_tuple!(C0, C1, C2, C3);
    impl_component_tuple!(C0, C1, C2, C3, C4);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8, C9);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12);
    impl_component_tuple!(C0, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13);
    impl_component_tuple!(
        C0, C1, C2, C3, C4, C5, C6, C7, C8, C9, C10, C11, C12, C13, C14
    );
}

/// A flat tuple of component types approved together before application start.
///
/// Sim;Logic implements this sealed trait for tuples of one through fifteen
/// [`Component`] types. It is a setup convenience only; every tuple member
/// still passes the same lifecycle-hook and required-component checks as an
/// individual approval.
pub trait ComponentTuple: supported::Tuple {}

impl<T: supported::Tuple> ComponentTuple for T {}

/// A Bevy lifecycle hook excluded from managed first-slice components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleHook {
    /// Runs when a component is first added.
    Add,
    /// Runs whenever a component value is inserted.
    Insert,
    /// Runs before an existing value is discarded.
    Discard,
    /// Runs when a component is removed.
    Remove,
    /// Runs when the owning entity is despawned.
    Despawn,
}

/// A component or bundle cannot enter the frozen managed component set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentApprovalError {
    /// The component declares a lifecycle hook that may generate unvalidated
    /// structural work.
    LifecycleHook {
        /// Rust type name of the rejected component.
        component: &'static str,
        /// First rejected hook in lifecycle order.
        hook: LifecycleHook,
    },
    /// An approved component requires another component that was not explicitly
    /// approved before startup.
    UnapprovedRequiredComponent {
        /// Rust type name of the component declaring the requirement.
        component: &'static str,
        /// Bevy diagnostic name of the missing required component.
        required: String,
    },
    /// A bundle mentions a component type outside the frozen approved set.
    UnapprovedBundleComponent {
        /// Bevy diagnostic name when the type was registered by another
        /// managed path, or a placeholder for an unknown type.
        component: String,
    },
    /// User code attempted to supply the runtime-owned entity identity.
    RuntimeIdentityInBundle,
}

impl fmt::Display for ComponentApprovalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LifecycleHook { component, hook } => {
                write!(
                    formatter,
                    "component `{component}` uses the {hook:?} lifecycle hook"
                )
            }
            Self::UnapprovedRequiredComponent {
                component,
                required,
            } => write!(
                formatter,
                "component `{component}` requires unapproved component `{required}`"
            ),
            Self::UnapprovedBundleComponent { component } => {
                write!(
                    formatter,
                    "bundle contains unapproved component `{component}`"
                )
            }
            Self::RuntimeIdentityInBundle => formatter.write_str(
                "bundle contains LogicEntity, which is attached only by the Sim;Logic runtime",
            ),
        }
    }
}

impl Error for ComponentApprovalError {}

struct ComponentRegistration {
    type_id: TypeId,
    name: &'static str,
    register: fn(&mut World) -> ComponentId,
}

#[derive(Default)]
pub(crate) struct ComponentRegistry {
    registrations: Vec<ComponentRegistration>,
}

impl ComponentRegistry {
    pub(crate) fn approve<T: Component>(&mut self) -> Result<(), ComponentApprovalError> {
        if let Some(hook) = first_hook::<T>() {
            return Err(ComponentApprovalError::LifecycleHook {
                component: type_name::<T>(),
                hook,
            });
        }

        if self
            .registrations
            .iter()
            .any(|registration| registration.type_id == TypeId::of::<T>())
        {
            return Ok(());
        }

        self.registrations.push(ComponentRegistration {
            type_id: TypeId::of::<T>(),
            name: type_name::<T>(),
            register: World::register_component::<T>,
        });
        Ok(())
    }

    pub(crate) fn approve_tuple<T: ComponentTuple>(
        &mut self,
    ) -> Result<(), ComponentApprovalError> {
        let checkpoint = self.registrations.len();
        if let Err(error) = <T as supported::Tuple>::approve(self) {
            self.registrations.truncate(checkpoint);
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn install(
        &self,
        world: &mut World,
    ) -> Result<ApprovedComponents, ComponentApprovalError> {
        let registered: Vec<_> = self
            .registrations
            .iter()
            .map(|registration| (registration, (registration.register)(world)))
            .collect();
        let approved: HashSet<_> = registered.iter().map(|(_, id)| *id).collect();

        for (registration, component_id) in &registered {
            if let Some(hook) = first_registered_hook(world, *component_id) {
                return Err(ComponentApprovalError::LifecycleHook {
                    component: registration.name,
                    hook,
                });
            }
            let Some(required) = world.get_required_components_by_id(*component_id) else {
                continue;
            };
            for required_id in required.iter_ids() {
                if !approved.contains(&required_id) {
                    let required_name = world
                        .components()
                        .get_info(required_id)
                        .map(|info| info.name().to_string())
                        .unwrap_or_else(|| format!("{required_id:?}"));
                    return Err(ComponentApprovalError::UnapprovedRequiredComponent {
                        component: registration.name,
                        required: required_name,
                    });
                }
            }
        }

        let identity = world.register_component::<ManagedEntity>();
        Ok(ApprovedComponents {
            approved,
            identity,
            validated_bundles: Vec::new(),
            last_validated_bundle: None,
        })
    }
}

fn first_hook<T: Component>() -> Option<LifecycleHook> {
    if T::on_add().is_some() {
        Some(LifecycleHook::Add)
    } else if T::on_insert().is_some() {
        Some(LifecycleHook::Insert)
    } else if T::on_discard().is_some() {
        Some(LifecycleHook::Discard)
    } else if T::on_remove().is_some() {
        Some(LifecycleHook::Remove)
    } else if T::on_despawn().is_some() {
        Some(LifecycleHook::Despawn)
    } else {
        None
    }
}

fn no_op_component_hook(_world: DeferredWorld<'_>, _context: HookContext) {}

fn first_registered_hook(world: &World, component: ComponentId) -> Option<LifecycleHook> {
    let mut hooks = world.components().get_info(component)?.hooks().clone();
    let hook: ComponentHook = no_op_component_hook;
    if hooks.try_on_add(hook).is_none() {
        Some(LifecycleHook::Add)
    } else if hooks.try_on_insert(hook).is_none() {
        Some(LifecycleHook::Insert)
    } else if hooks.try_on_discard(hook).is_none() {
        Some(LifecycleHook::Discard)
    } else if hooks.try_on_remove(hook).is_none() {
        Some(LifecycleHook::Remove)
    } else if hooks.try_on_despawn(hook).is_none() {
        Some(LifecycleHook::Despawn)
    } else {
        None
    }
}

pub(crate) fn validate_resource<R: Resource>(
    world: &mut World,
) -> Result<(), ComponentApprovalError> {
    if let Some(hook) = first_hook::<R>() {
        return Err(ComponentApprovalError::LifecycleHook {
            component: type_name::<R>(),
            hook,
        });
    }

    let resource_id = world.register_component::<R>();
    if let Some(hook) = first_registered_hook(world, resource_id) {
        return Err(ComponentApprovalError::LifecycleHook {
            component: type_name::<R>(),
            hook,
        });
    }
    let is_resource_id = world.register_component::<IsResource>();
    if let Some(required) = world.get_required_components_by_id(resource_id) {
        for required_id in required.iter_ids() {
            // Every Bevy resource requires this internal marker. It is part of
            // Bevy's storage machinery, not a user-declared required component.
            if required_id == is_resource_id {
                continue;
            }
            return Err(ComponentApprovalError::UnapprovedRequiredComponent {
                component: type_name::<R>(),
                required: component_name(world, required_id),
            });
        }
    }
    Ok(())
}

pub(crate) struct ApprovedComponents {
    approved: HashSet<ComponentId>,
    identity: ComponentId,
    validated_bundles: Vec<TypeId>,
    last_validated_bundle: Option<TypeId>,
}

impl ApprovedComponents {
    pub(crate) fn validate_bundle<B>(
        &mut self,
        world: &mut World,
    ) -> Result<(), ComponentApprovalError>
    where
        B: Bundle<Effect: NoBundleEffect>,
    {
        let bundle = TypeId::of::<B>();
        if self.last_validated_bundle == Some(bundle) {
            return Ok(());
        }
        if self.validated_bundles.contains(&bundle) {
            self.last_validated_bundle = Some(bundle);
            return Ok(());
        }

        let invalid_explicit = B::get_component_ids(world.components()).find(|component_id| {
            component_id.is_none_or(|component_id| {
                component_id == self.identity || !self.approved.contains(&component_id)
            })
        });
        if let Some(component_id) = invalid_explicit {
            let Some(component_id) = component_id else {
                return Err(ComponentApprovalError::UnapprovedBundleComponent {
                    component: "unregistered Rust component type".to_owned(),
                });
            };
            if component_id == self.identity {
                return Err(ComponentApprovalError::RuntimeIdentityInBundle);
            }
            return Err(ComponentApprovalError::UnapprovedBundleComponent {
                component: component_name(world, component_id),
            });
        }

        let invalid_contributed = world
            .register_bundle::<B>()
            .iter_contributed_components()
            .find(|component_id| {
                *component_id == self.identity || !self.approved.contains(component_id)
            });
        if let Some(component_id) = invalid_contributed {
            if component_id == self.identity {
                return Err(ComponentApprovalError::RuntimeIdentityInBundle);
            }
            return Err(ComponentApprovalError::UnapprovedBundleComponent {
                component: component_name(world, component_id),
            });
        }

        self.last_validated_bundle = Some(bundle);
        if self.validated_bundles.try_reserve(1).is_ok() {
            self.validated_bundles.push(bundle);
        }
        Ok(())
    }
}

fn component_name(world: &World, component_id: ComponentId) -> String {
    world
        .components()
        .get_info(component_id)
        .map(|info| info.name().to_string())
        .unwrap_or_else(|| format!("{component_id:?}"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bevy_ecs::{
        component::{Component, Mutable, StorageType},
        lifecycle::ComponentHook,
        world::{DeferredWorld, World},
    };

    use super::*;

    #[derive(Component)]
    struct Plain;

    fn on_add(_world: DeferredWorld<'_>, _context: bevy_ecs::lifecycle::HookContext) {}

    #[derive(Component)]
    #[component(on_add = on_add)]
    struct Hooked;

    #[derive(Component)]
    #[require(Required)]
    struct Requires;

    #[derive(Component, Default)]
    struct Required;

    macro_rules! define_tuple_components {
        ($($component:ident),+) => {
            $(
                #[derive(Component)]
                struct $component;
            )+
        };
    }

    define_tuple_components!(
        Tuple0, Tuple1, Tuple2, Tuple3, Tuple4, Tuple5, Tuple6, Tuple7, Tuple8, Tuple9, Tuple10,
        Tuple11, Tuple12, Tuple13, Tuple14
    );

    static STATEFUL_HOOK_QUERIES: AtomicUsize = AtomicUsize::new(0);
    static STATEFUL_HOOK_RUNS: AtomicUsize = AtomicUsize::new(0);

    struct StatefulHook;

    impl Component for StatefulHook {
        const STORAGE_TYPE: StorageType = StorageType::Table;
        type Mutability = Mutable;

        fn on_add() -> Option<ComponentHook> {
            if STATEFUL_HOOK_QUERIES.fetch_add(1, Ordering::SeqCst) == 0 {
                None
            } else {
                Some(stateful_on_add)
            }
        }
    }

    fn stateful_on_add(_world: DeferredWorld<'_>, _context: bevy_ecs::lifecycle::HookContext) {
        STATEFUL_HOOK_RUNS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn rejects_lifecycle_hooks_before_world_creation() {
        let mut registry = ComponentRegistry::default();
        let result = registry.approve::<Hooked>();

        assert!(matches!(
            result,
            Err(ComponentApprovalError::LifecycleHook {
                hook: LifecycleHook::Add,
                ..
            })
        ));
    }

    #[test]
    fn component_tuple_approves_all_supported_members() {
        let mut registry = ComponentRegistry::default();
        assert!(
            registry
                .approve_tuple::<(
                    Tuple0,
                    Tuple1,
                    Tuple2,
                    Tuple3,
                    Tuple4,
                    Tuple5,
                    Tuple6,
                    Tuple7,
                    Tuple8,
                    Tuple9,
                    Tuple10,
                    Tuple11,
                    Tuple12,
                    Tuple13,
                    Tuple14,
                )>()
                .is_ok()
        );

        let mut world = World::new();
        let mut approved = registry
            .install(&mut world)
            .expect("flat tuple members should install");
        assert!(
            approved
                .validate_bundle::<(
                    Tuple0,
                    Tuple1,
                    Tuple2,
                    Tuple3,
                    Tuple4,
                    Tuple5,
                    Tuple6,
                    Tuple7,
                    Tuple8,
                    Tuple9,
                    Tuple10,
                    Tuple11,
                    Tuple12,
                    Tuple13,
                    Tuple14,
                )>(&mut world)
                .is_ok()
        );
    }

    #[test]
    fn component_tuple_failure_rolls_back_only_new_approvals() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Required>().is_ok());

        let result = registry.approve_tuple::<(Plain, Hooked)>();

        assert!(matches!(
            result,
            Err(ComponentApprovalError::LifecycleHook { .. })
        ));
        assert_eq!(registry.registrations.len(), 1);
        assert_eq!(registry.registrations[0].type_id, TypeId::of::<Required>());
    }

    #[test]
    fn component_tuple_is_idempotent_and_closes_required_components() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve_tuple::<(Requires, Required)>().is_ok());
        assert!(registry.approve_tuple::<(Required, Requires)>().is_ok());
        assert_eq!(registry.registrations.len(), 2);
        assert!(registry.install(&mut World::new()).is_ok());
    }

    #[test]
    fn required_closure_must_be_explicitly_approved() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Requires>().is_ok());

        let result = registry.install(&mut World::new());
        assert!(matches!(
            result,
            Err(ComponentApprovalError::UnapprovedRequiredComponent { .. })
        ));
    }

    #[test]
    fn approved_plain_bundle_passes_without_runtime_identity() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Plain>().is_ok());
        let mut world = World::new();
        let mut approved = match registry.install(&mut world) {
            Ok(approved) => approved,
            Err(error) => panic!("unexpected component approval failure: {error}"),
        };

        assert!(approved.validate_bundle::<Plain>(&mut world).is_ok());
        assert!(matches!(
            approved.validate_bundle::<ManagedEntity>(&mut world),
            Err(ComponentApprovalError::RuntimeIdentityInBundle)
        ));
    }

    #[test]
    fn successful_bundle_validation_is_cached_once_across_alternating_types() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Plain>().is_ok());
        assert!(registry.approve::<Required>().is_ok());
        let mut world = World::new();
        let mut approved = registry
            .install(&mut world)
            .expect("plain components should install");

        assert!(approved.validated_bundles.is_empty());
        assert!(approved.validate_bundle::<Plain>(&mut world).is_ok());
        assert_eq!(approved.validated_bundles, [TypeId::of::<Plain>()]);
        assert_eq!(approved.last_validated_bundle, Some(TypeId::of::<Plain>()));

        assert!(approved.validate_bundle::<Plain>(&mut world).is_ok());
        assert_eq!(approved.validated_bundles.len(), 1);

        assert!(approved.validate_bundle::<Required>(&mut world).is_ok());
        assert_eq!(approved.validated_bundles.len(), 2);
        assert_eq!(
            approved.last_validated_bundle,
            Some(TypeId::of::<Required>())
        );

        assert!(approved.validate_bundle::<Plain>(&mut world).is_ok());
        assert_eq!(approved.validated_bundles.len(), 2);
        assert_eq!(approved.last_validated_bundle, Some(TypeId::of::<Plain>()));
    }

    #[test]
    fn rejected_bundle_validation_is_never_cached() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Plain>().is_ok());
        let mut world = World::new();
        let mut approved = registry
            .install(&mut world)
            .expect("plain component should install");

        for _ in 0..2 {
            assert!(matches!(
                approved.validate_bundle::<Required>(&mut world),
                Err(ComponentApprovalError::UnapprovedBundleComponent { .. })
            ));
            assert!(approved.validated_bundles.is_empty());
            assert_eq!(approved.last_validated_bundle, None);
        }
    }

    #[test]
    fn installed_worlds_do_not_share_bundle_validation_caches() {
        let mut registry = ComponentRegistry::default();
        assert!(registry.approve::<Plain>().is_ok());
        let mut first_world = World::new();
        let mut second_world = World::new();
        let mut first = registry
            .install(&mut first_world)
            .expect("first component set should install");
        let second = registry
            .install(&mut second_world)
            .expect("second component set should install");

        assert!(first.validate_bundle::<Plain>(&mut first_world).is_ok());
        assert_eq!(first.validated_bundles.len(), 1);
        assert!(second.validated_bundles.is_empty());
        assert_eq!(second.last_validated_bundle, None);
    }

    #[test]
    fn install_rechecks_the_hooks_actually_registered_by_bevy() {
        STATEFUL_HOOK_QUERIES.store(0, Ordering::SeqCst);
        STATEFUL_HOOK_RUNS.store(0, Ordering::SeqCst);
        let mut registry = ComponentRegistry::default();

        assert!(registry.approve::<StatefulHook>().is_ok());
        let result = registry.install(&mut World::new());

        assert!(matches!(
            result,
            Err(ComponentApprovalError::LifecycleHook {
                component,
                hook: LifecycleHook::Add,
            }) if component == std::any::type_name::<StatefulHook>()
        ));
        assert_eq!(STATEFUL_HOOK_RUNS.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn hook_function_has_expected_bevy_signature() {
        let _: ComponentHook = on_add;
    }
}

//! Application-owned typed state that survives World replacement.
//!
//! The runtime gives candidate factories and Startup systems no access to
//! these values. It cannot revoke external aliases created by user code, such
//! as a shared `Arc`; mutating one from a factory remains a forbidden external
//! side effect. Panics, including `Drop` panics while committing a replacement,
//! are outside typed recovery and do not leave a resumable runner guarantee.

use std::{
    any::TypeId,
    error::Error,
    fmt,
    ops::{Deref, DerefMut},
};

use bevy_ecs::{
    change_detection::DetectChanges,
    prelude::{Res, ResMut, Resource},
    system::SystemParam,
    world::World,
};

use crate::system::Stage;

/// Failure while registering Application-owned state before startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApplicationResourceError {
    /// One value of this concrete type is already registered.
    Duplicate {
        /// Rust type name used only for diagnostics.
        resource: &'static str,
    },
    /// The frozen maximum number of Application Resources was reached.
    LimitExceeded {
        /// Configured maximum number of distinct resource types.
        limit: usize,
    },
}

impl fmt::Display for ApplicationResourceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { resource } => {
                write!(
                    formatter,
                    "Application Resource `{resource}` is already registered"
                )
            }
            Self::LimitExceeded { limit } => write!(
                formatter,
                "Application Resource registry reached its limit of {limit} types"
            ),
        }
    }
}

impl Error for ApplicationResourceError {}

#[derive(Resource)]
struct StoredApplicationResource<T: Send + Sync + 'static>(T);

/// Shared access to state owned by the Application rather than the active World.
///
/// Register `T` once with
/// [`Application::register_app_resource`](crate::app::Application::register_app_resource).
/// The same value is then available to FixedUpdate and FrameUpdate systems in
/// every installed World generation. Startup cannot request this parameter,
/// because candidate construction must remain isolated from live state.
///
/// Unlike Bevy's [`Resource`] trait, `T` needs no derive or marker trait.
#[derive(SystemParam)]
pub struct AppRes<'w, T: Send + Sync + 'static> {
    inner: Res<'w, StoredApplicationResource<T>>,
}

impl<T: Send + Sync + 'static> Deref for AppRes<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.0
    }
}

impl<T: Send + Sync + 'static> AppRes<'_, T> {
    /// Returns whether this value changed since this System last ran.
    ///
    /// Change tracking belongs to the active ECS World and therefore starts
    /// fresh after successful World replacement.
    pub fn is_changed(&self) -> bool {
        self.inner.is_changed()
    }

    /// Returns whether this value was installed since this System last ran.
    ///
    /// A transferred value is newly installed in the replacement ECS World,
    /// even though the owned `T` itself was not cloned or reset.
    pub fn is_added(&self) -> bool {
        self.inner.is_added()
    }
}

/// Exclusive access to state owned by the Application rather than the active
/// World.
///
/// Mutations are immediately visible to later systems in stage order and
/// survive successful World replacement. Like direct `ResMut` changes, they
/// are not rolled back when a later system, command batch, or candidate
/// preparation fails.
#[derive(SystemParam)]
pub struct AppResMut<'w, T: Send + Sync + 'static> {
    inner: ResMut<'w, StoredApplicationResource<T>>,
}

impl<T: Send + Sync + 'static> Deref for AppResMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner.0
    }
}

impl<T: Send + Sync + 'static> DerefMut for AppResMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner.0
    }
}

impl<T: Send + Sync + 'static> AppResMut<'_, T> {
    /// Returns whether this value changed since this System last ran.
    ///
    /// Change tracking belongs to the active ECS World and therefore starts
    /// fresh after successful World replacement.
    pub fn is_changed(&self) -> bool {
        self.inner.is_changed()
    }

    /// Returns whether this value was installed since this System last ran.
    ///
    /// A transferred value is newly installed in the replacement ECS World,
    /// even though the owned `T` itself was not cloned or reset.
    pub fn is_added(&self) -> bool {
        self.inner.is_added()
    }
}

type InitialInstaller = Box<dyn FnOnce(&mut World) + Send + 'static>;

struct Registration {
    type_id: TypeId,
    initial: Option<InitialInstaller>,
    contains: fn(&World) -> bool,
    move_between: fn(&mut World, &mut World),
}

#[derive(Clone, Copy)]
struct Requirement {
    type_id: TypeId,
    type_name: &'static str,
    stage: Stage,
    system: &'static str,
}

/// Frozen type registry and transfer plan for Application Resources.
#[derive(Default)]
pub(crate) struct ApplicationResourceRegistry {
    registrations: Vec<Registration>,
    requirements: Vec<Requirement>,
}

impl ApplicationResourceRegistry {
    pub(crate) fn insert<T: Send + Sync + 'static>(
        &mut self,
        value: T,
        limit: usize,
    ) -> Result<(), ApplicationResourceError> {
        let type_id = TypeId::of::<T>();
        let type_name = std::any::type_name::<T>();
        if self
            .registrations
            .iter()
            .any(|registration| registration.type_id == type_id)
        {
            return Err(ApplicationResourceError::Duplicate {
                resource: type_name,
            });
        }
        if self.registrations.len() >= limit {
            return Err(ApplicationResourceError::LimitExceeded { limit });
        }

        self.registrations.push(Registration {
            type_id,
            initial: Some(Box::new(move |world| {
                world.insert_resource(StoredApplicationResource(value));
            })),
            contains: contains::<T>,
            move_between: move_between::<T>,
        });
        Ok(())
    }

    pub(crate) fn require<T: Send + Sync + 'static>(&mut self, stage: Stage, system: &'static str) {
        let type_id = TypeId::of::<T>();
        if self
            .requirements
            .iter()
            .all(|requirement| requirement.type_id != type_id)
        {
            self.requirements.push(Requirement {
                type_id,
                type_name: std::any::type_name::<T>(),
                stage,
                system,
            });
        }
    }

    pub(crate) fn first_missing_requirement(&self) -> Option<(&'static str, Stage, &'static str)> {
        self.requirements
            .iter()
            .find(|requirement| {
                self.registrations
                    .iter()
                    .all(|registration| registration.type_id != requirement.type_id)
            })
            .map(|requirement| (requirement.type_name, requirement.stage, requirement.system))
    }

    pub(crate) fn install_initial(&mut self, world: &mut World) -> bool {
        if self
            .registrations
            .iter()
            .any(|registration| registration.initial.is_none())
        {
            return false;
        }

        for registration in &mut self.registrations {
            let Some(install) = registration.initial.take() else {
                return false;
            };
            install(world);
        }
        true
    }

    /// Checks the complete transfer set before the irreversible lifecycle
    /// boundary and without changing either World.
    pub(crate) fn can_transfer(&self, source: &World, target: &World) -> bool {
        self.registrations
            .iter()
            .all(|registration| (registration.contains)(source) && !(registration.contains)(target))
    }

    /// Moves a set already accepted by [`Self::can_transfer`].
    ///
    /// No user code runs between preflight and this operation. A violated
    /// condition is therefore a runtime bug and follows the crate's existing
    /// policy that panics are outside typed recovery.
    pub(crate) fn transfer_prevalidated(&self, source: &mut World, target: &mut World) {
        for registration in &self.registrations {
            (registration.move_between)(source, target);
        }
    }

    pub(crate) fn get<'a, T: Send + Sync + 'static>(&self, world: &'a World) -> Option<&'a T> {
        world
            .get_resource::<StoredApplicationResource<T>>()
            .map(|resource| &resource.0)
    }
}

fn contains<T: Send + Sync + 'static>(world: &World) -> bool {
    world.contains_resource::<StoredApplicationResource<T>>()
}

fn move_between<T: Send + Sync + 'static>(source: &mut World, target: &mut World) {
    assert!(
        !target.contains_resource::<StoredApplicationResource<T>>(),
        "prevalidated Application Resource target became occupied"
    );
    let resource = source
        .remove_resource::<StoredApplicationResource<T>>()
        .expect("prevalidated Application Resource source became empty");
    target.insert_resource(resource);
}

#[cfg(test)]
mod tests {
    use bevy_ecs::prelude::Resource;

    use super::*;

    #[derive(Debug, PartialEq, Eq, Resource)]
    struct Score(u32);

    #[test]
    fn rejects_duplicate_types_without_replacing_the_first_value() {
        let mut registry = ApplicationResourceRegistry::default();
        assert!(registry.insert(Score(7), 2).is_ok());
        assert!(matches!(
            registry.insert(Score(99), 2),
            Err(ApplicationResourceError::Duplicate { .. })
        ));

        let mut world = World::new();
        assert!(registry.install_initial(&mut world));
        assert_eq!(registry.get::<Score>(&world), Some(&Score(7)));
    }

    #[test]
    fn enforces_the_frozen_type_limit() {
        let mut registry = ApplicationResourceRegistry::default();
        assert!(registry.insert(Score(7), 1).is_ok());
        assert!(matches!(
            registry.insert("settings", 1),
            Err(ApplicationResourceError::LimitExceeded { limit: 1 })
        ));
    }

    #[test]
    fn application_and_world_resource_namespaces_are_distinct() {
        let mut registry = ApplicationResourceRegistry::default();
        assert!(registry.insert(Score(7), 1).is_ok());
        let mut world = World::new();
        world.insert_resource(Score(99));
        assert!(registry.install_initial(&mut world));

        assert_eq!(registry.get::<Score>(&world), Some(&Score(7)));
        assert_eq!(world.resource::<Score>(), &Score(99));
    }

    #[test]
    fn transfer_moves_identity_without_cloning() {
        let mut registry = ApplicationResourceRegistry::default();
        assert!(registry.insert(Score(7), 1).is_ok());
        let mut first = World::new();
        let mut second = World::new();
        assert!(registry.install_initial(&mut first));

        assert!(registry.can_transfer(&first, &second));
        registry.transfer_prevalidated(&mut first, &mut second);
        assert!(registry.get::<Score>(&first).is_none());
        assert_eq!(registry.get::<Score>(&second), Some(&Score(7)));
    }

    #[test]
    fn rejected_transfer_does_not_partially_move_values() {
        let mut registry = ApplicationResourceRegistry::default();
        assert!(registry.insert(Score(7), 2).is_ok());
        assert!(registry.insert(String::from("live"), 2).is_ok());
        let mut first = World::new();
        let mut second = World::new();
        assert!(registry.install_initial(&mut first));
        second.insert_resource(StoredApplicationResource(String::from("collision")));

        assert!(!registry.can_transfer(&first, &second));
        assert_eq!(registry.get::<Score>(&first), Some(&Score(7)));
        assert_eq!(
            registry.get::<String>(&first).map(String::as_str),
            Some("live")
        );
    }
}

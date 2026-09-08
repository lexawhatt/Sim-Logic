//! Count-bounded typed events visible only within one System-stage invocation.

use std::{
    any::{TypeId, type_name},
    error::Error,
    fmt,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use bevy_ecs::{
    prelude::{Res, ResMut, Resource},
    system::SystemParam,
    world::World,
};

/// A small value that can be broadcast between Systems in one World stage.
///
/// This first-slice contract requires `Copy` so an expired event cannot retain
/// owned heap memory or defer user `Drop` code beyond its stage. Prefer compact
/// values and opaque handles; the configured limit bounds record count, not
/// `size_of::<E>()` bytes.
///
/// ```compile_fail
/// use sim_logic::events::WorldEvent;
///
/// fn accepts_world_event<E: WorldEvent>() {}
/// accepts_world_event::<String>();
/// ```
pub trait WorldEvent: Copy + Send + Sync + 'static {}

impl<T: Copy + Send + Sync + 'static> WorldEvent for T {}

/// A typed event could not be retained by the current stage invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventSendError {
    /// This event type already retained its configured maximum record count.
    LimitExceeded {
        /// Rust type name of the channel that first failed.
        event: &'static str,
        /// Maximum records for each event type in one stage invocation.
        limit: usize,
    },
    /// The channel could not reserve storage for one more record.
    AllocationFailed {
        /// Rust type name of the channel that first failed.
        event: &'static str,
    },
    /// An earlier event send already failed in this stage invocation.
    StageAlreadyFailed,
    /// The runtime did not expose an active event-delivery stage.
    StageUnavailable {
        /// Rust type name of the channel used outside a running stage.
        event: &'static str,
    },
}

impl fmt::Display for EventSendError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded { event, limit } => {
                write!(formatter, "event `{event}` exceeded its limit of {limit}")
            }
            Self::AllocationFailed { event } => {
                write!(formatter, "event `{event}` could not reserve storage")
            }
            Self::StageAlreadyFailed => {
                formatter.write_str("an earlier event send already failed in this stage")
            }
            Self::StageUnavailable { event } => {
                write!(
                    formatter,
                    "event `{event}` cannot be sent outside a running stage"
                )
            }
        }
    }
}

impl Error for EventSendError {}

#[derive(Resource)]
struct EventChannel<E: WorldEvent> {
    events: Vec<E>,
    written_epoch: Option<u64>,
    limit: usize,
}

impl<E: WorldEvent> EventChannel<E> {
    const fn new(limit: usize) -> Self {
        Self {
            events: Vec::new(),
            written_epoch: None,
            limit,
        }
    }

    fn begin_writing(&mut self, epoch: u64) {
        if self.written_epoch != Some(epoch) {
            self.events.clear();
            self.written_epoch = Some(epoch);
        }
    }

    fn current(&self, epoch: Option<u64>) -> &[E] {
        if self.written_epoch == epoch && epoch.is_some() {
            &self.events
        } else {
            &[]
        }
    }
}

#[derive(Debug, Default, Resource)]
pub(crate) struct EventStageState {
    last_epoch: u64,
    active_epoch: Option<u64>,
}

impl EventStageState {
    fn begin(&mut self) -> bool {
        if self.active_epoch.is_some() {
            return false;
        }
        let Some(epoch) = self.last_epoch.checked_add(1) else {
            self.active_epoch = None;
            return false;
        };
        self.last_epoch = epoch;
        self.active_epoch = Some(epoch);
        true
    }

    fn end(&mut self) -> bool {
        self.active_epoch.take().is_some()
    }

    const fn active_epoch(&self) -> Option<u64> {
        self.active_epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EventStageFailure {
    error: EventSendError,
}

impl fmt::Display for EventStageFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(formatter)
    }
}

#[derive(Debug, Default, Resource)]
struct EventFailureState {
    failed: AtomicBool,
    first: Mutex<Option<EventStageFailure>>,
}

impl EventFailureState {
    fn reset(&mut self) -> bool {
        let Ok(first) = self.first.get_mut() else {
            return false;
        };
        *first = None;
        self.failed.store(false, Ordering::Release);
        true
    }

    fn is_failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }

    fn record(&self, error: EventSendError) {
        let Ok(mut first) = self.first.lock() else {
            self.failed.store(true, Ordering::Release);
            return;
        };
        if first.is_none() {
            *first = Some(EventStageFailure { error });
        }
        self.failed.store(true, Ordering::Release);
    }

    fn failure(&self) -> Result<Option<EventStageFailure>, ()> {
        if !self.is_failed() {
            return Ok(None);
        }
        self.first.lock().map(|first| *first).map_err(|_| ())
    }
}

/// Writes ordered typed events for later Systems in the current stage.
///
/// A send failure poisons the complete stage even when its returned error is
/// ignored. The scheduler stops before the next System and discards that
/// stage's pending structural and transition Commands. Successful sends become
/// immediately visible to later [`EventReader`] parameters of the same type.
/// To inspect earlier events and then write the same type in one System, use
/// this writer's [`Self::iter`] method. Requesting both `EventReader<E>` and
/// `EventWriter<E>` in one System conflicts over the same ECS channel.
#[derive(SystemParam)]
pub struct EventWriter<'w, E: WorldEvent> {
    channel: ResMut<'w, EventChannel<E>>,
    stage: Res<'w, EventStageState>,
    failure: Res<'w, EventFailureState>,
}

impl<E: WorldEvent> EventWriter<'_, E> {
    /// Retains one event in call order for later Systems in this stage.
    pub fn send(&mut self, event: E) -> Result<(), EventSendError> {
        if self.failure.is_failed() {
            return Err(EventSendError::StageAlreadyFailed);
        }
        let Some(epoch) = self.stage.active_epoch() else {
            let error = EventSendError::StageUnavailable {
                event: type_name::<E>(),
            };
            self.failure.record(error);
            return Err(error);
        };

        self.channel.begin_writing(epoch);
        if self.channel.events.len() >= self.channel.limit {
            let error = EventSendError::LimitExceeded {
                event: type_name::<E>(),
                limit: self.channel.limit,
            };
            self.failure.record(error);
            return Err(error);
        }
        if self.channel.events.try_reserve(1).is_err() {
            let error = EventSendError::AllocationFailed {
                event: type_name::<E>(),
            };
            self.failure.record(error);
            return Err(error);
        }
        self.channel.events.push(event);
        Ok(())
    }

    /// Iterates over events sent earlier in this stage, including by this writer.
    pub fn iter(&self) -> std::slice::Iter<'_, E> {
        self.channel.current(self.stage.active_epoch()).iter()
    }

    /// Returns the number of events currently visible to this writer.
    pub fn len(&self) -> usize {
        self.channel.current(self.stage.active_epoch()).len()
    }

    /// Returns `true` when this stage has not retained an event of this type.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, E: WorldEvent> IntoIterator for &'a EventWriter<'_, E> {
    type Item = &'a E;
    type IntoIter = std::slice::Iter<'a, E>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A repeatable read-only view of events sent earlier in the current stage.
///
/// Reading does not consume records. Every later reader sees the same ordered
/// slice, while a reader that ran before a writer is not invoked again. Do not
/// request a reader and writer of the same event type in one System; use the
/// writer's read-only methods when both operations are needed.
#[derive(SystemParam)]
pub struct EventReader<'w, E: WorldEvent> {
    channel: Res<'w, EventChannel<E>>,
    stage: Res<'w, EventStageState>,
}

impl<E: WorldEvent> EventReader<'_, E> {
    /// Iterates over the complete event slice currently visible to this stage.
    pub fn iter(&self) -> std::slice::Iter<'_, E> {
        self.channel.current(self.stage.active_epoch()).iter()
    }

    /// Returns the number of events visible to this reader.
    pub fn len(&self) -> usize {
        self.channel.current(self.stage.active_epoch()).len()
    }

    /// Returns `true` when no earlier System sent this event type in the stage.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl<'a, E: WorldEvent> IntoIterator for &'a EventReader<'_, E> {
    type Item = &'a E;
    type IntoIter = std::slice::Iter<'a, E>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

struct RegisteredEvent {
    type_id: TypeId,
    install: fn(&mut World, usize),
}

#[derive(Default)]
pub(crate) struct EventRegistry {
    events: Vec<RegisteredEvent>,
}

impl EventRegistry {
    pub(crate) fn register<E: WorldEvent>(&mut self) {
        if self
            .events
            .iter()
            .any(|event| event.type_id == TypeId::of::<E>())
        {
            return;
        }
        self.events.push(RegisteredEvent {
            type_id: TypeId::of::<E>(),
            install: install_channel::<E>,
        });
    }

    pub(crate) fn install(&self, world: &mut World, event_limit: usize) {
        world.insert_resource(EventStageState::default());
        world.insert_resource(EventFailureState::default());
        for event in &self.events {
            (event.install)(world, event_limit);
        }
    }
}

fn install_channel<E: WorldEvent>(world: &mut World, limit: usize) {
    world.insert_resource(EventChannel::<E>::new(limit));
}

pub(crate) fn begin_event_stage(world: &mut World) -> bool {
    let reset = world
        .get_resource_mut::<EventFailureState>()
        .is_some_and(|mut failure| failure.reset());
    reset
        && world
            .get_resource_mut::<EventStageState>()
            .is_some_and(|mut stage| stage.begin())
}

pub(crate) fn end_event_stage(world: &mut World) -> bool {
    let Some(mut stage) = world.get_resource_mut::<EventStageState>() else {
        return false;
    };
    stage.end()
}

pub(crate) fn event_stage_failure(world: &World) -> Result<Option<EventStageFailure>, ()> {
    world
        .get_resource::<EventFailureState>()
        .ok_or(())?
        .failure()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epochs_hide_old_records_and_reuse_capacity() {
        let mut channel = EventChannel::<u8>::new(2);
        channel.begin_writing(1);
        channel.events.extend([3, 4]);
        let capacity = channel.events.capacity();

        assert_eq!(channel.current(Some(1)), [3, 4]);
        assert!(channel.current(None).is_empty());
        assert!(channel.current(Some(2)).is_empty());

        channel.begin_writing(2);
        channel.events.push(9);
        assert_eq!(channel.current(Some(2)), [9]);
        assert_eq!(channel.events.capacity(), capacity);
    }

    #[test]
    fn stage_epoch_exhaustion_closes_delivery() {
        let mut stage = EventStageState {
            last_epoch: u64::MAX,
            active_epoch: None,
        };

        assert!(!stage.begin());
        assert_eq!(stage.active_epoch(), None);
    }

    #[test]
    fn stage_lifecycle_rejects_nested_begin_and_unmatched_end() {
        let mut stage = EventStageState::default();

        assert!(!stage.end());
        assert!(stage.begin());
        assert!(!stage.begin());
        assert!(stage.end());
        assert!(!stage.end());
    }
}

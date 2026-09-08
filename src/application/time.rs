//! Validated frame and fixed-step timekeeping.

use std::{error::Error, fmt, time::Duration};

use bevy_ecs::{
    prelude::{Res, Resource},
    system::SystemParam,
};

/// Nearest nanosecond `Duration` representation of one sixtieth of a second.
pub const DEFAULT_FIXED_STEP: Duration = Duration::from_nanos(16_666_667);

/// Default maximum number of fixed ticks attempted in one application frame.
pub const DEFAULT_MAX_CATCH_UP_TICKS: u32 = 8;

/// Failure to construct or change fixed-step time configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeConfigurationError {
    /// A zero fixed step cannot advance simulation time.
    ZeroFixedStep,
    /// A zero catch-up limit would silently disable fixed simulation.
    ZeroCatchUpLimit,
    /// Time scale must be finite and non-negative.
    InvalidTimeScale {
        /// The rejected scale value.
        value: f64,
    },
}

impl fmt::Display for TimeConfigurationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroFixedStep => formatter.write_str("the fixed step must be positive"),
            Self::ZeroCatchUpLimit => {
                formatter.write_str("the maximum catch-up tick count must be positive")
            }
            Self::InvalidTimeScale { value } => write!(
                formatter,
                "time scale must be finite and non-negative, but received {value}"
            ),
        }
    }
}

impl Error for TimeConfigurationError {}

/// Validated fixed-step timing configuration.
///
/// The configuration is frozen before the runner starts. Pause and time scale
/// may later be changed by the runtime without altering the fixed step or
/// catch-up limit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeConfig {
    fixed_step: Duration,
    max_catch_up_ticks: u32,
    time_scale: f64,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            fixed_step: DEFAULT_FIXED_STEP,
            max_catch_up_ticks: DEFAULT_MAX_CATCH_UP_TICKS,
            time_scale: 1.0,
        }
    }
}

impl TimeConfig {
    /// Creates a configuration with time scale `1.0`.
    pub fn new(
        fixed_step: Duration,
        max_catch_up_ticks: u32,
    ) -> Result<Self, TimeConfigurationError> {
        if fixed_step.is_zero() {
            return Err(TimeConfigurationError::ZeroFixedStep);
        }
        if max_catch_up_ticks == 0 {
            return Err(TimeConfigurationError::ZeroCatchUpLimit);
        }

        Ok(Self {
            fixed_step,
            max_catch_up_ticks,
            time_scale: 1.0,
        })
    }

    /// Returns the exact `Duration` supplied to every FixedUpdate tick.
    pub const fn fixed_step(self) -> Duration {
        self.fixed_step
    }

    /// Returns the maximum fixed ticks attempted in one application frame.
    pub const fn max_catch_up_ticks(self) -> u32 {
        self.max_catch_up_ticks
    }

    /// Returns the multiplier applied before wall delta enters the accumulator.
    pub const fn time_scale(self) -> f64 {
        self.time_scale
    }

    /// Sets the initial finite, non-negative simulation time scale.
    ///
    /// The configuration remains unchanged when validation fails.
    pub fn set_time_scale(&mut self, time_scale: f64) -> Result<(), TimeConfigurationError> {
        validate_time_scale(time_scale)?;
        self.time_scale = normalize_zero(time_scale);
        Ok(())
    }

    /// Returns this configuration with a validated initial time scale.
    pub fn with_time_scale(mut self, time_scale: f64) -> Result<Self, TimeConfigurationError> {
        self.set_time_scale(time_scale)?;
        Ok(self)
    }
}

/// Unscaled elapsed wall time accepted for one application frame.
///
/// FrameUpdate receives this value even while fixed simulation is paused.
/// Time scale affects the fixed accumulator, not this presentation delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) struct FrameTimeState {
    delta: Duration,
    paused: bool,
}

impl FrameTimeState {
    pub const fn delta(self) -> Duration {
        self.delta
    }

    pub fn seconds(self) -> f64 {
        self.delta.as_secs_f64()
    }

    #[inline]
    pub fn seconds_f32(self) -> f32 {
        self.delta.as_secs_f32()
    }

    pub const fn is_paused(self) -> bool {
        self.paused
    }

    pub(crate) const fn new(delta: Duration, paused: bool) -> Self {
        Self { delta, paused }
    }
}

/// Read-only unscaled wall time for the current FrameUpdate.
///
/// This custom System parameter prevents systems from replacing the
/// runtime-owned clock value through `ResMut`. It is not available during an
/// active FixedUpdate stage.
#[derive(SystemParam)]
pub struct FrameTime<'w> {
    state: Res<'w, FrameTimeState>,
}

impl FrameTime<'_> {
    /// Returns the unscaled elapsed duration supplied by the runner.
    pub fn delta(&self) -> Duration {
        self.state.delta()
    }

    /// Returns the unscaled elapsed duration in seconds.
    pub fn seconds(&self) -> f64 {
        self.state.seconds()
    }

    /// Returns the unscaled elapsed duration using [`Duration::as_secs_f32`].
    ///
    /// This uses [`Duration::as_secs_f32`] rather than narrowing the result of
    /// [`Self::seconds`]. It is convenient for Sim;Engine's `f32` geometry.
    /// Use [`Self::seconds`] for wider floating-point arithmetic or
    /// [`Self::delta`] for exact `Duration` operations.
    #[inline]
    pub fn seconds_f32(&self) -> f32 {
        self.state.seconds_f32()
    }

    /// Returns whether fixed simulation was paused when FrameUpdate began.
    ///
    /// Pause changes queued by Systems in this same stage are deferred until
    /// its Commands barrier, so every System sees the same entry snapshot.
    pub fn is_paused(&self) -> bool {
        self.state.is_paused()
    }
}

/// Exact fixed delta and application-wide index for one FixedUpdate attempt.
///
/// The runtime consumes the tick before invoking systems. Its index therefore
/// advances even when a later system fails after earlier systems mutated the
/// World.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Resource)]
pub(crate) struct FixedTimeState {
    delta: Duration,
    tick_index: u64,
}

impl FixedTimeState {
    pub const fn delta(self) -> Duration {
        self.delta
    }

    pub fn seconds(self) -> f64 {
        self.delta.as_secs_f64()
    }

    #[inline]
    pub fn seconds_f32(self) -> f32 {
        self.delta.as_secs_f32()
    }

    pub const fn tick_index(self) -> u64 {
        self.tick_index
    }

    pub(crate) const fn initial(delta: Duration) -> Self {
        Self {
            delta,
            tick_index: 0,
        }
    }
}

/// Read-only exact fixed delta and application-wide tick index.
///
/// The runtime consumes the tick before invoking systems. Its index therefore
/// advances even when a later system fails. This parameter is not available
/// during FrameUpdate.
#[derive(SystemParam)]
pub struct FixedTime<'w> {
    state: Res<'w, FixedTimeState>,
}

impl FixedTime<'_> {
    /// Returns the exact configured fixed simulation delta.
    pub fn delta(&self) -> Duration {
        self.state.delta()
    }

    /// Returns the exact fixed simulation delta in seconds.
    pub fn seconds(&self) -> f64 {
        self.state.seconds()
    }

    /// Returns the configured fixed-step delta using [`Duration::as_secs_f32`].
    ///
    /// This uses [`Duration::as_secs_f32`] rather than narrowing the result of
    /// [`Self::seconds`]. It is convenient for Sim;Engine's `f32` geometry.
    /// Use [`Self::seconds`] for wider floating-point arithmetic or
    /// [`Self::delta`] for exact `Duration` operations.
    #[inline]
    pub fn seconds_f32(&self) -> f32 {
        self.state.seconds_f32()
    }

    /// Returns the zero-based application-wide index of this fixed tick.
    pub fn tick_index(&self) -> u64 {
        self.state.tick_index()
    }
}

/// Failure while scaling or accumulating a supplied frame delta.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimeAdvanceError {
    /// Multiplying the wall delta by time scale cannot produce a `Duration`.
    ScaledDeltaOutOfRange {
        /// Unscaled wall delta supplied by the runner.
        elapsed: Duration,
        /// Configured multiplier used for fixed simulation.
        time_scale: f64,
    },
    /// Adding the scaled delta would exceed `Duration` capacity.
    AccumulatorOverflow,
    /// Internal duration accounting exceeded the representable range.
    DurationAccountingOverflow,
}

impl fmt::Display for TimeAdvanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScaledDeltaOutOfRange {
                elapsed,
                time_scale,
            } => write!(
                formatter,
                "scaling frame delta {elapsed:?} by {time_scale} cannot produce a Duration"
            ),
            Self::AccumulatorOverflow => {
                formatter.write_str("the fixed-step accumulator would overflow")
            }
            Self::DurationAccountingOverflow => {
                formatter.write_str("fixed-step duration accounting overflowed")
            }
        }
    }
}

impl Error for TimeAdvanceError {}

/// Planned and dropped fixed work for one application frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedFramePlan {
    ticks_to_attempt: u32,
    dropped_ticks: u128,
    dropped_time: Duration,
    scaled_delta: Duration,
}

impl FixedFramePlan {
    /// Returns the fixed ticks the runtime may attempt in this frame.
    pub const fn ticks_to_attempt(self) -> u32 {
        self.ticks_to_attempt
    }

    /// Returns the complete fixed ticks removed by bounded catch-up.
    pub const fn dropped_ticks(self) -> u128 {
        self.dropped_ticks
    }

    /// Returns the simulation duration removed by bounded catch-up.
    pub const fn dropped_time(self) -> Duration {
        self.dropped_time
    }

    /// Returns the wall delta after applying time scale.
    pub const fn scaled_delta(self) -> Duration {
        self.scaled_delta
    }
}

/// Fixed simulation time explicitly discarded after update planning.
///
/// The first slice uses this report when a pending transition stops catch-up
/// but arbitration rejects the transition as malformed or conflicting. Whole
/// ticks are discarded so the retained accumulator is again a valid
/// interpolation remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DroppedFixedTime {
    ticks: u128,
    duration: Duration,
}

impl DroppedFixedTime {
    /// Returns the number of complete fixed ticks that were discarded.
    pub const fn ticks(self) -> u128 {
        self.ticks
    }

    /// Returns the total fixed simulation duration that was discarded.
    pub const fn duration(self) -> Duration {
        self.duration
    }
}

/// Failure to begin one of the fixed ticks planned for the current frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixedTickError {
    /// Fixed execution is paused.
    Paused,
    /// No planned fixed tick remains in the current frame.
    NoPlannedTick,
    /// The application-wide fixed tick index cannot advance further.
    TickIndexExhausted,
}

impl fmt::Display for FixedTickError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Paused => formatter.write_str("fixed simulation is paused"),
            Self::NoPlannedTick => {
                formatter.write_str("no planned fixed tick remains in this frame")
            }
            Self::TickIndexExhausted => formatter.write_str("the fixed tick index is exhausted"),
        }
    }
}

impl Error for FixedTickError {}

#[derive(Debug, Clone)]
pub(crate) struct TimeState {
    config: TimeConfig,
    paused: bool,
    accumulator: Duration,
    next_tick_index: u64,
    remaining_planned_ticks: u32,
}

impl TimeState {
    pub(crate) fn new(config: TimeConfig) -> Self {
        Self {
            config,
            paused: false,
            accumulator: Duration::ZERO,
            next_tick_index: 0,
            remaining_planned_ticks: 0,
        }
    }

    pub(crate) fn frame_time(&self, elapsed: Duration) -> FrameTimeState {
        FrameTimeState::new(elapsed, self.paused)
    }

    pub(crate) fn plan_frame(
        &mut self,
        elapsed: Duration,
    ) -> Result<FixedFramePlan, TimeAdvanceError> {
        if self.paused {
            self.remaining_planned_ticks = 0;
            return Ok(FixedFramePlan {
                ticks_to_attempt: 0,
                dropped_ticks: 0,
                dropped_time: Duration::ZERO,
                scaled_delta: Duration::ZERO,
            });
        }

        let scaled_delta = scale_duration(elapsed, self.config.time_scale)?;
        let accumulated = self
            .accumulator
            .checked_add(scaled_delta)
            .ok_or(TimeAdvanceError::AccumulatorOverflow)?;

        let step_nanoseconds = self.config.fixed_step.as_nanos();
        let accumulated_nanoseconds = accumulated.as_nanos();
        let available_ticks = accumulated_nanoseconds / step_nanoseconds;
        let maximum_ticks = u128::from(self.config.max_catch_up_ticks);
        let planned_u128 = available_ticks.min(maximum_ticks);
        let planned_ticks = u32::try_from(planned_u128)
            .map_err(|_| TimeAdvanceError::DurationAccountingOverflow)?;
        let dropped_ticks = available_ticks - planned_u128;
        let dropped_nanoseconds = dropped_ticks
            .checked_mul(step_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let retained_nanoseconds = accumulated_nanoseconds
            .checked_sub(dropped_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let retained = duration_from_nanoseconds(retained_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let dropped_time = duration_from_nanoseconds(dropped_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;

        self.accumulator = retained;
        self.remaining_planned_ticks = planned_ticks;

        Ok(FixedFramePlan {
            ticks_to_attempt: planned_ticks,
            dropped_ticks,
            dropped_time,
            scaled_delta,
        })
    }

    pub(crate) fn begin_fixed_tick(&mut self) -> Result<FixedTimeState, FixedTickError> {
        if self.paused {
            return Err(FixedTickError::Paused);
        }
        if self.remaining_planned_ticks == 0 || self.accumulator < self.config.fixed_step {
            return Err(FixedTickError::NoPlannedTick);
        }

        let Some(next_tick_index) = self.next_tick_index.checked_add(1) else {
            return Err(FixedTickError::TickIndexExhausted);
        };

        let fixed_time = FixedTimeState {
            delta: self.config.fixed_step,
            tick_index: self.next_tick_index,
        };
        self.accumulator -= self.config.fixed_step;
        self.remaining_planned_ticks -= 1;
        self.next_tick_index = next_tick_index;
        Ok(fixed_time)
    }

    pub(crate) fn stop_remaining_ticks(&mut self) {
        self.remaining_planned_ticks = 0;
    }

    pub(crate) fn drop_remaining_whole_ticks(
        &mut self,
    ) -> Result<DroppedFixedTime, TimeAdvanceError> {
        let step_nanoseconds = self.config.fixed_step.as_nanos();
        let accumulated_nanoseconds = self.accumulator.as_nanos();
        let dropped_ticks = accumulated_nanoseconds / step_nanoseconds;
        let dropped_nanoseconds = dropped_ticks
            .checked_mul(step_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let remainder_nanoseconds = accumulated_nanoseconds
            .checked_sub(dropped_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let remainder = duration_from_nanoseconds(remainder_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;
        let duration = duration_from_nanoseconds(dropped_nanoseconds)
            .ok_or(TimeAdvanceError::DurationAccountingOverflow)?;

        self.accumulator = remainder;
        self.remaining_planned_ticks = 0;

        Ok(DroppedFixedTime {
            ticks: dropped_ticks,
            duration,
        })
    }

    pub(crate) fn clear_accumulator(&mut self) {
        self.accumulator = Duration::ZERO;
        self.remaining_planned_ticks = 0;
    }

    pub(crate) fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
        if paused {
            self.remaining_planned_ticks = 0;
        }
    }

    pub(crate) const fn is_paused(&self) -> bool {
        self.paused
    }

    pub(crate) fn set_time_scale(&mut self, time_scale: f64) -> Result<(), TimeConfigurationError> {
        self.config.set_time_scale(time_scale)
    }

    #[cfg(test)]
    pub(crate) fn accumulator(&self) -> Duration {
        self.accumulator
    }

    #[cfg(test)]
    pub(crate) fn remaining_planned_ticks(&self) -> u32 {
        self.remaining_planned_ticks
    }

    pub(crate) fn interpolation_alpha(&self) -> Option<f64> {
        if self.paused {
            return Some(0.0);
        }
        if self.accumulator >= self.config.fixed_step {
            return None;
        }

        Some(self.accumulator.as_secs_f64() / self.config.fixed_step.as_secs_f64())
    }
}

fn validate_time_scale(time_scale: f64) -> Result<(), TimeConfigurationError> {
    if !time_scale.is_finite() || time_scale < 0.0 {
        return Err(TimeConfigurationError::InvalidTimeScale { value: time_scale });
    }
    Ok(())
}

fn normalize_zero(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}

fn scale_duration(elapsed: Duration, time_scale: f64) -> Result<Duration, TimeAdvanceError> {
    if time_scale == 0.0 {
        return Ok(Duration::ZERO);
    }
    if time_scale == 1.0 {
        return Ok(elapsed);
    }

    let scaled_seconds = elapsed.as_secs_f64() * time_scale;
    Duration::try_from_secs_f64(scaled_seconds).map_err(|_| {
        TimeAdvanceError::ScaledDeltaOutOfRange {
            elapsed,
            time_scale,
        }
    })
}

fn duration_from_nanoseconds(nanoseconds: u128) -> Option<Duration> {
    const NANOS_PER_SECOND: u128 = 1_000_000_000;

    let seconds = u64::try_from(nanoseconds / NANOS_PER_SECOND).ok()?;
    let subsecond_nanoseconds = u32::try_from(nanoseconds % NANOS_PER_SECOND).ok()?;
    Some(Duration::new(seconds, subsecond_nanoseconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(step_milliseconds: u64, maximum_ticks: u32) -> TimeConfig {
        let result = TimeConfig::new(Duration::from_millis(step_milliseconds), maximum_ticks);
        let Ok(config) = result else {
            panic!("test time configuration should be valid");
        };
        config
    }

    #[test]
    fn defaults_are_one_sixtieth_and_eight_ticks() {
        let config = TimeConfig::default();

        assert_eq!(config.fixed_step(), Duration::from_nanos(16_666_667));
        assert_eq!(config.max_catch_up_ticks(), 8);
        assert_eq!(config.time_scale(), 1.0);
    }

    #[test]
    fn time_states_follow_the_standard_duration_float_conversions_exactly() {
        let durations = [
            Duration::ZERO,
            Duration::from_nanos(1),
            DEFAULT_FIXED_STEP,
            Duration::new(42, 987_654_321),
            Duration::MAX,
        ];

        for duration in durations {
            let frame = FrameTimeState::new(duration, false);
            let fixed = FixedTimeState::initial(duration);
            assert_eq!(frame.seconds().to_bits(), duration.as_secs_f64().to_bits());
            assert_eq!(fixed.seconds().to_bits(), duration.as_secs_f64().to_bits());
            assert_eq!(
                frame.seconds_f32().to_bits(),
                duration.as_secs_f32().to_bits()
            );
            assert_eq!(
                fixed.seconds_f32().to_bits(),
                duration.as_secs_f32().to_bits()
            );
            assert!(!frame.is_paused());
        }

        assert!(FrameTimeState::new(Duration::ZERO, true).is_paused());

        let direct_rounding_witness = Duration::new(0, 16_874_317);
        assert_ne!(
            direct_rounding_witness.as_secs_f32().to_bits(),
            (direct_rounding_witness.as_secs_f64() as f32).to_bits()
        );
    }

    #[test]
    fn invalid_configuration_is_rejected() {
        assert_eq!(
            TimeConfig::new(Duration::ZERO, 8),
            Err(TimeConfigurationError::ZeroFixedStep)
        );
        assert_eq!(
            TimeConfig::new(Duration::from_millis(1), 0),
            Err(TimeConfigurationError::ZeroCatchUpLimit)
        );

        let mut config = TimeConfig::default();
        assert!(matches!(
            config.set_time_scale(f64::NAN),
            Err(TimeConfigurationError::InvalidTimeScale { .. })
        ));
        assert!(matches!(
            config.set_time_scale(-1.0),
            Err(TimeConfigurationError::InvalidTimeScale { .. })
        ));
        assert_eq!(config.time_scale(), 1.0);
    }

    #[test]
    fn accumulator_plans_zero_one_and_multiple_ticks() {
        let mut time = TimeState::new(config(10, 8));

        let first = time.plan_frame(Duration::from_millis(5));
        let Ok(first) = first else {
            panic!("small frame should be representable");
        };
        assert_eq!(first.ticks_to_attempt(), 0);
        assert_eq!(time.interpolation_alpha(), Some(0.5));

        let second = time.plan_frame(Duration::from_millis(5));
        let Ok(second) = second else {
            panic!("second frame should be representable");
        };
        assert_eq!(second.ticks_to_attempt(), 1);
        let tick = time.begin_fixed_tick();
        let Ok(tick) = tick else {
            panic!("one fixed tick should be planned");
        };
        assert_eq!(tick.tick_index(), 0);
        assert_eq!(time.accumulator(), Duration::ZERO);

        let third = time.plan_frame(Duration::from_millis(25));
        let Ok(third) = third else {
            panic!("multi-tick frame should be representable");
        };
        assert_eq!(third.ticks_to_attempt(), 2);
    }

    #[test]
    fn catch_up_is_bounded_and_reports_dropped_time() {
        let mut time = TimeState::new(config(10, 3));
        let plan = time.plan_frame(Duration::from_millis(105));
        let Ok(plan) = plan else {
            panic!("bounded frame should be representable");
        };

        assert_eq!(plan.ticks_to_attempt(), 3);
        assert_eq!(plan.dropped_ticks(), 7);
        assert_eq!(plan.dropped_time(), Duration::from_millis(70));
        assert_eq!(time.accumulator(), Duration::from_millis(35));

        for _ in 0..3 {
            assert!(time.begin_fixed_tick().is_ok());
        }
        assert_eq!(time.accumulator(), Duration::from_millis(5));
        assert_eq!(time.interpolation_alpha(), Some(0.5));
    }

    #[test]
    fn beginning_a_tick_consumes_it_even_if_systems_later_fail() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(25)).is_ok());

        let attempted = time.begin_fixed_tick();
        let Ok(attempted) = attempted else {
            panic!("the first fixed tick should begin");
        };
        assert_eq!(attempted.tick_index(), 0);

        // This is the runtime action after a failed system: no rollback of the
        // consumed tick, while the remaining accumulated time is preserved.
        time.stop_remaining_ticks();
        assert_eq!(time.accumulator(), Duration::from_millis(15));
        assert_eq!(time.remaining_planned_ticks(), 0);

        let next_plan = time.plan_frame(Duration::ZERO);
        let Ok(next_plan) = next_plan else {
            panic!("preserved accumulated time should remain representable");
        };
        assert_eq!(next_plan.ticks_to_attempt(), 1);
        let next_tick = time.begin_fixed_tick();
        let Ok(next_tick) = next_tick else {
            panic!("the preserved fixed tick should be attempted later");
        };
        assert_eq!(next_tick.tick_index(), 1);
        assert_eq!(time.accumulator(), Duration::from_millis(5));
    }

    #[test]
    fn pause_freezes_accumulated_time_and_accepts_no_fixed_work() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(5)).is_ok());
        time.set_paused(true);

        let paused = time.plan_frame(Duration::from_secs(1));
        let Ok(paused) = paused else {
            panic!("paused frame should not scale wall time");
        };
        assert_eq!(paused.ticks_to_attempt(), 0);
        assert_eq!(time.accumulator(), Duration::from_millis(5));
        assert_eq!(time.begin_fixed_tick(), Err(FixedTickError::Paused));

        time.set_paused(false);
        assert!(time.plan_frame(Duration::from_millis(5)).is_ok());
        assert!(time.begin_fixed_tick().is_ok());
        assert_eq!(time.accumulator(), Duration::ZERO);
    }

    #[test]
    fn paused_interpolation_is_defined_even_with_a_whole_tick_retained() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(25)).is_ok());
        assert!(time.begin_fixed_tick().is_ok());
        time.stop_remaining_ticks();
        assert_eq!(time.interpolation_alpha(), None);

        time.set_paused(true);

        assert_eq!(time.accumulator(), Duration::from_millis(15));
        assert_eq!(time.interpolation_alpha(), Some(0.0));
    }

    #[test]
    fn time_scale_changes_accumulator_input_not_frame_time() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.set_time_scale(2.0).is_ok());

        let elapsed = Duration::from_millis(6);
        let plan = time.plan_frame(elapsed);
        let Ok(plan) = plan else {
            panic!("scaled frame should be representable");
        };
        assert_eq!(plan.scaled_delta(), Duration::from_millis(12));
        assert_eq!(plan.ticks_to_attempt(), 1);
        assert_eq!(time.frame_time(elapsed).delta(), elapsed);
    }

    #[test]
    fn reset_discards_accumulated_and_planned_work() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(25)).is_ok());

        time.clear_accumulator();

        assert_eq!(time.accumulator(), Duration::ZERO);
        assert_eq!(time.remaining_planned_ticks(), 0);
        assert_eq!(time.interpolation_alpha(), Some(0.0));
    }

    #[test]
    fn interpolation_is_unavailable_while_whole_ticks_remain() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(25)).is_ok());
        assert_eq!(time.interpolation_alpha(), None);

        assert!(time.begin_fixed_tick().is_ok());
        assert!(time.begin_fixed_tick().is_ok());
        assert_eq!(time.interpolation_alpha(), Some(0.5));
    }

    #[test]
    fn rejected_transition_drops_whole_ticks_and_preserves_remainder() {
        let mut time = TimeState::new(config(10, 8));
        assert!(time.plan_frame(Duration::from_millis(35)).is_ok());
        assert!(time.begin_fixed_tick().is_ok());
        time.stop_remaining_ticks();

        let dropped = time.drop_remaining_whole_ticks();
        let Ok(dropped) = dropped else {
            panic!("remaining fixed time should be representable");
        };

        assert_eq!(dropped.ticks(), 2);
        assert_eq!(dropped.duration(), Duration::from_millis(20));
        assert_eq!(time.accumulator(), Duration::from_millis(5));
        assert_eq!(time.remaining_planned_ticks(), 0);
        assert_eq!(time.interpolation_alpha(), Some(0.5));
    }
}

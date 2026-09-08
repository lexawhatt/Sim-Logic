//! Opt-in fixed-step motion components and systems.

use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;
use sim_engine::{Camera2dError, Vec2};

use crate::{
    input::{Action, DigitalAxis2d, FixedInput},
    query::{Query, QuerySingleError},
    time::FixedTime,
    visual::{ActiveCamera2d, Transform2d, VisualValueError},
};

/// Finite world-space translation per second for fixed-step motion.
///
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. The component stores motion data but
/// does not move an entity by itself. Explicitly register the standard adapter
/// with [`crate::app::Application::add_linear_velocity2d_system`], or read the
/// value from an application-defined System.
///
/// This is linear integration only. It does not apply acceleration, forces,
/// collision detection or response, world bounds, rotation, or drag.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct LinearVelocity2d {
    velocity: Vec2,
}

impl LinearVelocity2d {
    /// Creates a finite world-space velocity in units per second.
    pub fn new(velocity: Vec2) -> Result<Self, LinearVelocity2dError> {
        validate_velocity(velocity)?;
        Ok(Self { velocity })
    }

    /// Returns the world-space velocity in units per second.
    pub const fn velocity(&self) -> Vec2 {
        self.velocity
    }

    /// Replaces the velocity after finite-value validation.
    ///
    /// A rejected value leaves the previous velocity unchanged.
    pub fn set_velocity(&mut self, velocity: Vec2) -> Result<(), LinearVelocity2dError> {
        validate_velocity(velocity)?;
        self.velocity = velocity;
        Ok(())
    }
}

impl Default for LinearVelocity2d {
    fn default() -> Self {
        Self {
            velocity: Vec2::ZERO,
        }
    }
}

/// A [`LinearVelocity2d`] value is not finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearVelocity2dError {
    /// At least one velocity coordinate was NaN or infinite.
    InvalidVelocity {
        /// Rejected world-space velocity.
        value: Vec2,
    },
}

impl fmt::Display for LinearVelocity2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVelocity { value } => {
                write!(
                    formatter,
                    "linear velocity must be finite, but received {value:?}"
                )
            }
        }
    }
}

impl Error for LinearVelocity2dError {}

/// Finite world-space velocity change per second for fixed-step motion.
///
/// Spawning this component without a [`LinearVelocity2d`] automatically inserts
/// a zero velocity, which transitively supplies a default [`Transform2d`] at the
/// world origin. The component stores motion data but does not change velocity
/// by itself. Explicitly register the standard adapter with
/// [`crate::app::Application::add_linear_acceleration2d_system`] at the desired
/// point in FixedUpdate order.
///
/// This is linear integration only. It does not represent force or mass, choose
/// an integrator, move the Transform directly, or apply gravity, drag, or
/// collision response.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(LinearVelocity2d)]
pub struct LinearAcceleration2d {
    acceleration: Vec2,
}

impl LinearAcceleration2d {
    /// Creates a finite world-space acceleration in units per second squared.
    pub fn new(acceleration: Vec2) -> Result<Self, LinearAcceleration2dError> {
        validate_acceleration(acceleration)?;
        Ok(Self { acceleration })
    }

    /// Returns the world-space acceleration in units per second squared.
    pub const fn acceleration(&self) -> Vec2 {
        self.acceleration
    }

    /// Replaces the acceleration after finite-value validation.
    ///
    /// A rejected value leaves the previous acceleration unchanged.
    pub fn set_acceleration(
        &mut self,
        acceleration: Vec2,
    ) -> Result<(), LinearAcceleration2dError> {
        validate_acceleration(acceleration)?;
        self.acceleration = acceleration;
        Ok(())
    }
}

impl Default for LinearAcceleration2d {
    fn default() -> Self {
        Self {
            acceleration: Vec2::ZERO,
        }
    }
}

/// A [`LinearAcceleration2d`] value is not finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LinearAcceleration2dError {
    /// At least one acceleration coordinate was NaN or infinite.
    InvalidAcceleration {
        /// Complete rejected world-space acceleration.
        value: Vec2,
    },
}

impl fmt::Display for LinearAcceleration2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidAcceleration { value } => write!(
                formatter,
                "linear acceleration must be finite, but received {value:?}"
            ),
        }
    }
}

impl Error for LinearAcceleration2dError {}

/// Marks the one enabled entity followed by the active 2D camera.
///
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. Its finite world-space camera offset
/// is stored as data but does not move a camera by itself. Explicitly register
/// the standard adapter with
/// [`crate::app::Application::add_camera_follow2d_system`] at the desired point
/// in FixedUpdate order.
///
/// The adapter follows zero or one enabled target. Zero targets is a no-op so
/// other Worlds in the same application do not need a follow target; multiple
/// targets fail the current stage rather than choosing one by ECS order.
/// Initial camera alignment and target/camera teleports remain explicit.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct CameraFollowTarget2d {
    camera_offset: Vec2,
}

impl CameraFollowTarget2d {
    /// Creates a target with a finite world-space offset from target to camera.
    pub fn new(camera_offset: Vec2) -> Result<Self, CameraFollowTarget2dError> {
        validate_camera_offset(camera_offset)?;
        Ok(Self { camera_offset })
    }

    /// Returns the world-space offset added to the target translation.
    pub const fn camera_offset(&self) -> Vec2 {
        self.camera_offset
    }

    /// Replaces the camera offset after finite-value validation.
    ///
    /// A rejected value leaves the previous offset unchanged.
    pub fn set_camera_offset(
        &mut self,
        camera_offset: Vec2,
    ) -> Result<(), CameraFollowTarget2dError> {
        validate_camera_offset(camera_offset)?;
        self.camera_offset = camera_offset;
        Ok(())
    }
}

impl Default for CameraFollowTarget2d {
    fn default() -> Self {
        Self {
            camera_offset: Vec2::ZERO,
        }
    }
}

/// A [`CameraFollowTarget2d`] camera offset is not finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CameraFollowTarget2dError {
    /// At least one camera-offset coordinate was NaN or infinite.
    InvalidCameraOffset {
        /// Complete rejected world-space camera offset.
        value: Vec2,
    },
}

impl fmt::Display for CameraFollowTarget2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCameraOffset { value } => write!(
                formatter,
                "camera follow offset must be finite, but received {value:?}"
            ),
        }
    }
}

impl Error for CameraFollowTarget2dError {}

/// Constant-speed movement driven by a typed digital input axis.
///
/// Spawning this component without a [`Transform2d`] automatically inserts a
/// default transform at the world origin. The component stores movement
/// configuration but does not read input or move by itself. Explicitly
/// register the standard adapter with
/// [`crate::app::Application::add_digital_movement2d_system`]. Physical key
/// bindings remain separate application configuration. `Application<A>::new`
/// approves `DigitalMovement2d<A>` for that exact action type; movement
/// components instantiated with another action type are not implicitly
/// approved in the same application.
///
/// Digital input is normalized before applying `speed`, so cardinal and
/// diagonal movement have the same magnitude. An idle or exactly opposed
/// direction is a no-op before speed/time multiplication. This adapter does
/// not perform collision detection or response, acceleration, facing, or
/// analog input.
#[derive(Debug, Clone, Copy, PartialEq, Component)]
#[require(Transform2d)]
pub struct DigitalMovement2d<A: Action> {
    axis: DigitalAxis2d<A>,
    speed: f32,
}

impl<A: Action> DigitalMovement2d<A> {
    /// Creates movement for one typed digital axis at a finite nonnegative speed.
    pub fn new(axis: DigitalAxis2d<A>, speed: f32) -> Result<Self, DigitalMovement2dError> {
        validate_speed(speed)?;
        Ok(Self { axis, speed })
    }

    /// Returns the four-action axis sampled by the standard adapter.
    pub const fn axis(&self) -> DigitalAxis2d<A> {
        self.axis
    }

    /// Returns the constant movement speed in world units per second.
    pub const fn speed(&self) -> f32 {
        self.speed
    }

    /// Replaces the infallible typed axis descriptor.
    pub fn set_axis(&mut self, axis: DigitalAxis2d<A>) {
        self.axis = axis;
    }

    /// Replaces the speed after finite nonnegative validation.
    ///
    /// A rejected value leaves the previous speed unchanged.
    pub fn set_speed(&mut self, speed: f32) -> Result<(), DigitalMovement2dError> {
        validate_speed(speed)?;
        self.speed = speed;
        Ok(())
    }
}

/// A [`DigitalMovement2d`] speed is negative or non-finite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DigitalMovement2dError {
    /// Rejected speed in world units per second.
    InvalidSpeed {
        /// Complete rejected scalar, including its floating-point payload.
        value: f32,
    },
}

impl fmt::Display for DigitalMovement2dError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSpeed { value } => write!(
                formatter,
                "digital movement speed must be finite and nonnegative, but received {value}"
            ),
        }
    }
}

impl Error for DigitalMovement2dError {}

pub(crate) fn integrate_linear_velocity2d(
    time: FixedTime,
    mut moving: Query<(&LinearVelocity2d, &mut Transform2d)>,
) -> Result<(), VisualValueError> {
    let delta_seconds = time.seconds_f32();
    for (velocity, mut transform) in &mut moving {
        transform.translate_by(velocity.velocity * delta_seconds)?;
    }
    Ok(())
}

pub(crate) fn integrate_linear_acceleration2d(
    time: FixedTime,
    mut accelerating: Query<(&LinearAcceleration2d, &mut LinearVelocity2d)>,
) -> Result<(), LinearVelocity2dError> {
    let delta_seconds = time.seconds_f32();
    for (acceleration, mut velocity) in &mut accelerating {
        if acceleration.acceleration == Vec2::ZERO {
            continue;
        }
        let next = velocity.velocity + acceleration.acceleration * delta_seconds;
        velocity.set_velocity(next)?;
    }
    Ok(())
}

#[derive(Debug)]
pub(crate) enum CameraFollow2dSystemError {
    MultipleTargets,
    InvalidCenter(Camera2dError),
}

impl fmt::Display for CameraFollow2dSystemError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MultipleTargets => {
                formatter.write_str("camera follow has more than one enabled target")
            }
            Self::InvalidCenter(error) => write!(formatter, "camera follow failed: {error}"),
        }
    }
}

impl Error for CameraFollow2dSystemError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidCenter(error) => Some(error),
            Self::MultipleTargets => None,
        }
    }
}

pub(crate) fn follow_camera_target2d(
    targets: Query<(&Transform2d, &CameraFollowTarget2d)>,
    mut cameras: Query<&mut ActiveCamera2d>,
) -> Result<(), CameraFollow2dSystemError> {
    let (transform, target) = match targets.single() {
        Ok(target) => target,
        Err(QuerySingleError::NoEntities) => return Ok(()),
        Err(QuerySingleError::MultipleEntities) => {
            return Err(CameraFollow2dSystemError::MultipleTargets);
        }
    };
    let Ok(mut camera) = cameras.single_mut() else {
        // Extraction owns active-camera cardinality and its recovery path.
        // Do not poison a stage that queued the camera repair at this barrier.
        return Ok(());
    };
    let translation = transform.translation();
    let center = if target.camera_offset == Vec2::ZERO {
        translation
    } else {
        translation + target.camera_offset
    };
    camera
        .set_center(center)
        .map_err(CameraFollow2dSystemError::InvalidCenter)
}

pub(crate) fn integrate_digital_movement2d<A: Action>(
    input: FixedInput<A>,
    time: FixedTime,
    mut moving: Query<(&DigitalMovement2d<A>, &mut Transform2d)>,
) -> Result<(), VisualValueError> {
    let delta_seconds = time.seconds_f32();
    // Homogeneous query runs normally share one axis. Reuse only the
    // immediately preceding immutable sample: this stays allocation-free and
    // falls back to exact per-row sampling whenever the axis changes.
    let mut last_axis = None;
    for (movement, mut transform) in &mut moving {
        let direction = match last_axis {
            Some((axis, direction)) if axis == movement.axis => direction,
            _ => {
                let direction = input.normalized_digital_axis(movement.axis);
                last_axis = Some((movement.axis, direction));
                direction
            }
        };
        if direction == Vec2::ZERO {
            continue;
        }
        transform.translate_by(direction * (movement.speed * delta_seconds))?;
    }
    Ok(())
}

fn validate_velocity(value: Vec2) -> Result<(), LinearVelocity2dError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(LinearVelocity2dError::InvalidVelocity { value })
}

fn validate_acceleration(value: Vec2) -> Result<(), LinearAcceleration2dError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(LinearAcceleration2dError::InvalidAcceleration { value })
}

fn validate_camera_offset(value: Vec2) -> Result<(), CameraFollowTarget2dError> {
    value
        .is_finite()
        .then_some(())
        .ok_or(CameraFollowTarget2dError::InvalidCameraOffset { value })
}

fn validate_speed(value: f32) -> Result<(), DigitalMovement2dError> {
    (value.is_finite() && value >= 0.0)
        .then_some(())
        .ok_or(DigitalMovement2dError::InvalidSpeed { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn velocity_validation_and_mutation_are_atomic_and_bit_exact() {
        let valid = [
            Vec2::ZERO,
            Vec2::new(-0.0, 0.0),
            Vec2::new(f32::from_bits(1), -f32::from_bits(1)),
            Vec2::new(12.5, -98.25),
            Vec2::new(f32::MAX, -f32::MAX),
        ];
        for value in valid {
            let velocity = LinearVelocity2d::new(value).expect("finite velocity should be valid");
            assert_vec2_bits_eq(velocity.velocity(), value);
        }
        assert_vec2_bits_eq(LinearVelocity2d::default().velocity(), Vec2::ZERO);

        let initial = Vec2::new(-0.0, f32::from_bits(1));
        let mut velocity =
            LinearVelocity2d::new(initial).expect("initial velocity should be valid");
        let updated = Vec2::new(-12.5, f32::MAX);
        velocity
            .set_velocity(updated)
            .expect("finite replacement velocity should be valid");
        assert_vec2_bits_eq(velocity.velocity(), updated);
        let nan = f32::from_bits(0x7fc0_1234);
        for value in [
            Vec2::new(nan, -0.0),
            Vec2::new(-0.0, f32::INFINITY),
            Vec2::new(f32::NEG_INFINITY, nan),
        ] {
            let error = velocity
                .set_velocity(value)
                .expect_err("non-finite velocity should be rejected");
            match error {
                LinearVelocity2dError::InvalidVelocity { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
            assert_vec2_bits_eq(velocity.velocity(), updated);

            let constructor = LinearVelocity2d::new(value)
                .expect_err("constructor should reject the same non-finite velocity");
            match constructor {
                LinearVelocity2dError::InvalidVelocity { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
        }
    }

    #[test]
    fn acceleration_validation_and_mutation_are_atomic_and_bit_exact() {
        let valid = [
            Vec2::ZERO,
            Vec2::new(-0.0, 0.0),
            Vec2::new(f32::from_bits(1), -f32::from_bits(1)),
            Vec2::new(12.5, -98.25),
            Vec2::new(f32::MAX, -f32::MAX),
        ];
        for value in valid {
            let acceleration =
                LinearAcceleration2d::new(value).expect("finite acceleration should be valid");
            assert_vec2_bits_eq(acceleration.acceleration(), value);
        }
        assert_vec2_bits_eq(LinearAcceleration2d::default().acceleration(), Vec2::ZERO);

        let initial = Vec2::new(-0.0, f32::from_bits(1));
        let mut acceleration =
            LinearAcceleration2d::new(initial).expect("initial acceleration should be valid");
        let updated = Vec2::new(-12.5, f32::MAX);
        acceleration
            .set_acceleration(updated)
            .expect("finite replacement acceleration should be valid");
        assert_vec2_bits_eq(acceleration.acceleration(), updated);
        let nan = f32::from_bits(0x7fc0_4321);
        for value in [
            Vec2::new(nan, -0.0),
            Vec2::new(-0.0, f32::INFINITY),
            Vec2::new(f32::NEG_INFINITY, nan),
        ] {
            let error = acceleration
                .set_acceleration(value)
                .expect_err("non-finite acceleration should be rejected");
            match error {
                LinearAcceleration2dError::InvalidAcceleration { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
            assert_vec2_bits_eq(acceleration.acceleration(), updated);

            let constructor = LinearAcceleration2d::new(value)
                .expect_err("constructor should reject the same non-finite acceleration");
            match constructor {
                LinearAcceleration2dError::InvalidAcceleration { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
        }
    }

    #[test]
    fn camera_follow_offset_validation_and_mutation_are_atomic_and_bit_exact() {
        let valid = [
            Vec2::ZERO,
            Vec2::new(-0.0, 0.0),
            Vec2::new(f32::from_bits(1), -f32::from_bits(1)),
            Vec2::new(12.5, -98.25),
            Vec2::new(f32::MAX, -f32::MAX),
        ];
        for value in valid {
            let target =
                CameraFollowTarget2d::new(value).expect("finite camera offset should be valid");
            assert_vec2_bits_eq(target.camera_offset(), value);
        }
        assert_vec2_bits_eq(CameraFollowTarget2d::default().camera_offset(), Vec2::ZERO);

        let initial = Vec2::new(-0.0, f32::from_bits(1));
        let mut target =
            CameraFollowTarget2d::new(initial).expect("initial camera offset should be valid");
        let updated = Vec2::new(-12.5, f32::MAX);
        target
            .set_camera_offset(updated)
            .expect("finite replacement camera offset should be valid");
        assert_vec2_bits_eq(target.camera_offset(), updated);
        let nan = f32::from_bits(0x7fc0_5678);
        for value in [
            Vec2::new(nan, -0.0),
            Vec2::new(-0.0, f32::INFINITY),
            Vec2::new(f32::NEG_INFINITY, nan),
        ] {
            let error = target
                .set_camera_offset(value)
                .expect_err("non-finite camera offset should be rejected");
            match error {
                CameraFollowTarget2dError::InvalidCameraOffset { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
            assert_vec2_bits_eq(target.camera_offset(), updated);

            let constructor = CameraFollowTarget2d::new(value)
                .expect_err("constructor should reject the same non-finite camera offset");
            match constructor {
                CameraFollowTarget2dError::InvalidCameraOffset { value: rejected } => {
                    assert_vec2_bits_eq(rejected, value);
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestAction {
        Left,
        Right,
        Down,
        Up,
        Alternate,
    }

    const AXIS: DigitalAxis2d<TestAction> = DigitalAxis2d::new(
        TestAction::Left,
        TestAction::Right,
        TestAction::Down,
        TestAction::Up,
    );

    #[test]
    fn digital_movement_validation_and_mutation_are_atomic_and_bit_exact() {
        for speed in [0.0, -0.0, f32::from_bits(1), 12.5, f32::MAX] {
            let movement = DigitalMovement2d::new(AXIS, speed)
                .expect("finite nonnegative speed should be valid");
            assert_eq!(movement.axis(), AXIS);
            assert_eq!(movement.speed().to_bits(), speed.to_bits());
        }

        let mut movement =
            DigitalMovement2d::new(AXIS, 4.0).expect("initial speed should be valid");
        let alternate = DigitalAxis2d::new(
            TestAction::Alternate,
            TestAction::Left,
            TestAction::Up,
            TestAction::Down,
        );
        movement.set_axis(alternate);
        assert_eq!(movement.axis(), alternate);
        movement
            .set_speed(f32::MAX)
            .expect("finite replacement speed should be valid");
        assert_eq!(movement.speed().to_bits(), f32::MAX.to_bits());

        let nan = f32::from_bits(0x7fc0_1234);
        for speed in [
            -f32::from_bits(1),
            -1.0,
            nan,
            f32::INFINITY,
            f32::NEG_INFINITY,
        ] {
            let error = movement
                .set_speed(speed)
                .expect_err("invalid replacement speed should be rejected");
            match error {
                DigitalMovement2dError::InvalidSpeed { value } => {
                    assert_eq!(value.to_bits(), speed.to_bits());
                }
            }
            assert_eq!(movement.speed().to_bits(), f32::MAX.to_bits());

            let constructor = DigitalMovement2d::new(AXIS, speed)
                .expect_err("constructor should reject the same invalid speed");
            match constructor {
                DigitalMovement2dError::InvalidSpeed { value } => {
                    assert_eq!(value.to_bits(), speed.to_bits());
                }
            }
        }
    }

    fn assert_vec2_bits_eq(actual: Vec2, expected: Vec2) {
        assert_eq!(actual.x().to_bits(), expected.x().to_bits());
        assert_eq!(actual.y().to_bits(), expected.y().to_bits());
    }
}

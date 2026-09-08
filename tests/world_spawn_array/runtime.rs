use std::error::Error;

use sim_logic::{ComponentApprovalError, bevy_ecs::entity_disabling::Disabled, prelude::*};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TestAction {}

#[derive(Component)]
struct Indexed(u8);

#[derive(Component)]
struct Rejected;

#[derive(Resource)]
struct SpawnedArrays {
    one: LogicEntity,
    implicit: [LogicEntity; 3],
    explicit: [LogicEntity; 2],
}

#[test]
fn public_fixed_arrays_preserve_values_requirements_identity_and_capacity()
-> Result<(), Box<dyn Error>> {
    let mut config = AppConfig::default();
    config.set_entity_limit(7)?;
    let mut application = Application::<TestAction>::new(config)?;
    application.approve_component::<Indexed>()?;

    let camera = ActiveCamera2d::centered(20.0)?;
    let visual = CircleVisual::new(0.5, Color::WHITE)?;
    let explicit_positions = [
        Transform2d::from_xy(4.0, -2.0)?,
        Transform2d::from_xy(-3.0, 5.0)?,
    ];
    let initial = application.register_world("spawn-array", move |world| {
        world.spawn(camera)?;
        assert_eq!(world.spawn_array::<Indexed, 0>([])?, []);
        assert!(matches!(
            world.spawn_array::<Rejected, 0>([]),
            Err(WorldBuildError::Component(
                ComponentApprovalError::UnapprovedBundleComponent { .. }
            ))
        ));

        let [one] = world.spawn_array([Indexed(1)])?;
        let implicit = world.spawn_array([
            (Indexed(2), visual, Disabled),
            (Indexed(3), visual, Disabled),
            (Indexed(4), visual, Disabled),
        ])?;
        let explicit = world.spawn_array([
            (Indexed(5), visual, explicit_positions[0]),
            (Indexed(6), visual, explicit_positions[1]),
        ])?;
        assert!(matches!(
            world.spawn(Indexed(7)),
            Err(WorldBuildError::EntityLimitExceeded { limit: 7 })
        ));
        world.insert_resource(SpawnedArrays {
            one,
            implicit,
            explicit,
        })?;
        Ok(())
    })?;
    let runner = application.build_headless(initial)?;

    let spawned = runner
        .resource::<SpawnedArrays>()
        .ok_or("spawned handles should remain available")?;
    let generation = spawned.one.world_generation();
    assert_eq!(runner.component::<Indexed>(spawned.one)?.0, 1);
    for (handle, expected) in spawned.implicit.into_iter().zip([2, 3, 4]) {
        assert_eq!(handle.world_generation(), generation);
        assert_eq!(runner.component::<Indexed>(handle)?.0, expected);
        assert!(runner.component::<Disabled>(handle).is_ok());
        assert_eq!(
            runner.component::<Transform2d>(handle)?.translation(),
            Vec2::ZERO
        );
    }
    for ((handle, expected_value), expected_position) in spawned
        .explicit
        .into_iter()
        .zip([5, 6])
        .zip(explicit_positions)
    {
        assert_eq!(handle.world_generation(), generation);
        assert_eq!(runner.component::<Indexed>(handle)?.0, expected_value);
        assert_eq!(
            runner.component::<Transform2d>(handle)?.translation(),
            expected_position.translation()
        );
    }
    assert_eq!(
        runner
            .extracted_frame()
            .ok_or("initial extraction should exist")?
            .resolved_circles()
            .len(),
        2
    );
    Ok(())
}

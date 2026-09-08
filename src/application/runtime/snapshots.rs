//! Stage-scoped input publication with reusable storage and invariant checks.

use bevy_ecs::world::World;

use crate::input::{Action, FixedInputState, FrameInputState, InputState};

pub(super) fn clear_frame_snapshot<A: Action>(world: &mut World) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FrameInputState<A>>() else {
        return false;
    };
    snapshot.clear_reusing_storage();
    true
}

pub(super) fn clear_fixed_snapshot<A: Action>(world: &mut World) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FixedInputState<A>>() else {
        return false;
    };
    snapshot.clear_reusing_storage();
    true
}

pub(super) fn copy_fixed_snapshot<A: Action>(world: &mut World, input: &InputState<A>) -> bool {
    let Some(mut snapshot) = world.get_resource_mut::<FixedInputState<A>>() else {
        return false;
    };
    input.copy_fixed_snapshot_into(&mut snapshot);
    true
}

pub(super) fn prepare_frame_snapshots<A: Action>(world: &mut World, input: &InputState<A>) -> bool {
    if !world.contains_resource::<FrameInputState<A>>()
        || !world.contains_resource::<FixedInputState<A>>()
    {
        return false;
    }

    {
        let Some(mut frame_snapshot) = world.get_resource_mut::<FrameInputState<A>>() else {
            return false;
        };
        input.copy_frame_snapshot_into(&mut frame_snapshot);
    }

    clear_fixed_snapshot::<A>(world)
}

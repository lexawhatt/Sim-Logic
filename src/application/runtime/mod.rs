//! Shared execution state beneath the public headless runner.
//!
//! The active World owns its stage instances, approved component cache, query
//! caches, and reusable interpolation history. Only the inspection facade needs
//! access outside this module; frame coordination lives in its descendants.

use bevy_ecs::world::World;

use crate::{
    component::ApprovedComponents,
    identity::{WorldFactoryId, WorldGeneration},
    system::SequentialStage,
};

mod barriers;
mod frame;
mod lifecycle;
mod rendering;
pub(super) mod reports;
mod snapshots;
mod transitions;

use rendering::{ExtractionQueries, InterpolationQueries};

pub(super) struct RuntimeWorld {
    pub(super) factory: WorldFactoryId,
    pub(super) generation: WorldGeneration,
    pub(super) world: World,
    approved: ApprovedComponents,
    managed_entities: usize,
    previous_translations: Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
    previous_camera_centers: Vec<(crate::identity::LogicEntity, sim_engine::Vec2)>,
    interpolation_queries: InterpolationQueries,
    extraction_queries: ExtractionQueries,
    fixed: SequentialStage,
    frame: SequentialStage,
}

impl RuntimeWorld {
    pub(super) fn snap_interpolation(&mut self) {
        rendering::snap_runtime_interpolation(self);
    }
}

#[cfg(test)]
mod tests;

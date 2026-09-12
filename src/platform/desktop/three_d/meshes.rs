//! Current-snapshot mesh residency; no cache of retired chunk revisions.

use super::*;
use crate::three_d::{MeshAsset3d, ResolvedMesh3d};
use sim_engine::Mesh3dUploadBudget;

pub(super) struct CustomMeshes {
    assets: Vec<CachedAsset>,
    slots: Vec<MeshSlot>,
}

struct CachedAsset {
    source: MeshAsset3d,
    retained: RetainedMesh3d,
    used: bool,
}

struct MeshSlot {
    asset_key: usize,
    scene: SceneSlot,
}

impl CustomMeshes {
    pub(super) const fn new() -> Self {
        Self {
            assets: Vec::new(),
            slots: Vec::new(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.assets.clear();
        self.slots.clear();
    }

    pub(super) fn clear_slots(&mut self) {
        self.slots.clear();
    }

    pub(super) fn object_sources(
        &self,
    ) -> impl Iterator<Item = (Object3dId, LogicEntity, bool)> + '_ {
        self.slots
            .iter()
            .map(|slot| (slot.scene.id, slot.scene.applied.source, true))
    }

    pub(super) fn prepare(
        &mut self,
        renderer: &WgpuRenderer,
        scene: &mut Scene3d,
        records: &[ResolvedMesh3d],
        limits: ThreeDRenderLimits,
        cube: &RetainedMesh3d,
    ) -> Result<(), DesktopThreeDError> {
        // Retire obsolete scene references before admitting any new upload.
        // Slots keep no mesh handles themselves. Engine may retain submitted
        // buffers until GPU completion, but host caches cannot accumulate edits.
        // The record index is not identity: removing an early chunk must not
        // reinsert every later object. Match complete managed sources; the
        // sorted raw entity bits are only a lookup accelerator, never authority.
        for index in (0..self.slots.len()).rev() {
            let slot = &self.slots[index];
            let source = slot.scene.applied.source;
            let incoming = records
                .binary_search_by_key(&source.stable_bits(), |record| {
                    record.source().stable_bits()
                })
                .ok()
                .map(|index| {
                    (
                        records[index].source(),
                        records[index].visual().asset().key(),
                    )
                });
            if !same_source_asset(source, slot.asset_key, incoming) {
                scene
                    .remove(slot.scene.id)
                    .map_err(DesktopThreeDError::Scene)?;
                self.slots.remove(index);
            }
        }
        for asset in &mut self.assets {
            asset.used = false;
        }
        for record in records {
            if let Ok(index) = self.asset_index(record.visual().asset().key()) {
                self.assets[index].used = true;
            }
        }
        self.assets.retain(|asset| asset.used);

        for record in records {
            let source = record.source();
            let key = record.visual().asset().key();
            let asset_index = match self.asset_index(key) {
                Ok(index) => index,
                Err(index) => {
                    reserve_one(&mut self.assets)?;
                    let cpu = self
                        .assets
                        .iter()
                        .fold(cube.recovery_memory_bytes(), |sum, asset| {
                            sum.saturating_add(asset.retained.recovery_memory_bytes())
                        });
                    let gpu = self
                        .assets
                        .iter()
                        .fold(cube.gpu_allocation_bytes(), |sum, asset| {
                            sum.saturating_add(asset.retained.gpu_allocation_bytes())
                        });
                    let budget = remaining_upload_budget(limits, cpu, gpu)?;
                    let retained = renderer
                        .create_mesh3d_with_budget(record.visual().asset().mesh().clone(), budget)
                        .map_err(DesktopThreeDError::Resource)?;
                    self.assets.insert(
                        index,
                        CachedAsset {
                            source: record.visual().asset().clone(),
                            retained,
                            used: true,
                        },
                    );
                    index
                }
            };
            let desired = AppliedState {
                source,
                transform: record.transform(),
                style: record
                    .visual()
                    .style()
                    .map_err(|error| DesktopThreeDError::CustomMesh { source, error })?,
                visible: true,
            };
            match self
                .slots
                .binary_search_by_key(&source.stable_bits(), |slot| {
                    slot.scene.applied.source.stable_bits()
                }) {
                Ok(index) => self.slots[index]
                    .scene
                    .update(scene, desired)
                    .map_err(|error| DesktopThreeDError::Object { source, error })?,
                Err(index) => {
                    reserve_one(&mut self.slots)?;
                    let id = scene
                        .try_push(
                            &self.assets[asset_index].retained,
                            desired.transform,
                            desired.style,
                        )
                        .map_err(|error| DesktopThreeDError::Object { source, error })?;
                    self.slots.insert(
                        index,
                        MeshSlot {
                            asset_key: key,
                            scene: SceneSlot {
                                id,
                                applied: desired,
                            },
                        },
                    );
                }
            }
        }
        Ok(())
    }

    fn asset_index(&self, key: usize) -> Result<usize, usize> {
        self.assets
            .binary_search_by_key(&key, |asset| asset.source.key())
    }
}

fn same_source_asset<Id: Eq>(source: Id, asset: usize, incoming: Option<(Id, usize)>) -> bool {
    incoming.is_some_and(|(incoming_source, incoming_asset)| {
        source == incoming_source && asset == incoming_asset
    })
}

fn remaining_upload_budget(
    limits: ThreeDRenderLimits,
    retained_cpu: usize,
    retained_gpu: usize,
) -> Result<Mesh3dUploadBudget, DesktopThreeDError> {
    let scene = engine_scene_budget(limits)?;
    Mesh3dUploadBudget::new(
        scene
            .max_mesh_cpu_bytes()
            .saturating_sub(retained_cpu)
            .min(limits.max_mesh_source_bytes()),
        scene.max_mesh_gpu_bytes().saturating_sub(retained_gpu),
        Mesh3dUploadBudget::default().max_staging_bytes(),
    )
    .map_err(DesktopThreeDError::Resource)
}

fn reserve_one<T>(values: &mut Vec<T>) -> Result<(), DesktopThreeDError> {
    if values.len() == values.capacity() {
        values
            .try_reserve_exact(1)
            .map_err(|_| DesktopThreeDError::AllocationFailed {
                requested_bytes: size_of::<T>(),
            })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_assets_keep_slots_but_replacement_and_disappearance_retire() {
        assert!(same_source_asset((1, 7), 9, Some(((1, 7), 9))));
        assert!(!same_source_asset((1, 7), 9, None));
        assert!(!same_source_asset((1, 7), 9, Some(((1, 7), 10))));
        assert!(!same_source_asset((1, 7), 9, Some(((2, 7), 9))));
    }

    #[test]
    fn removing_first_chunk_preserves_every_surviving_source_slot() {
        let current = [(1_u32, 11), (2, 22), (3, 33), (4, 44)];
        let incoming = [(2_u32, 22), (3, 33), (4, 44)];
        let retired: Vec<_> = current
            .iter()
            .copied()
            .filter(|(source, asset)| {
                let found = incoming
                    .binary_search_by_key(source, |(source, _)| *source)
                    .ok()
                    .map(|index| incoming[index]);
                !same_source_asset(*source, *asset, found)
            })
            .collect();
        assert_eq!(retired, [(1, 11)]);
    }

    #[test]
    fn upload_allowance_respects_remaining_aggregate_bytes_not_only_incoming_size() {
        let scene = Scene3dBudget::default();
        let limits = ThreeDRenderLimits::new(0, 100, 1).with_mesh_limits(8, 4096);
        let budget = remaining_upload_budget(
            limits,
            scene.max_mesh_cpu_bytes() - 100,
            scene.max_mesh_gpu_bytes() - 200,
        )
        .unwrap();
        assert_eq!(budget.max_recovery_bytes(), 100);
        assert_eq!(budget.max_gpu_bytes(), 200);
        assert_eq!(
            budget.max_staging_bytes(),
            Mesh3dUploadBudget::default().max_staging_bytes()
        );
        let source_limited = remaining_upload_budget(limits, 0, 0).unwrap();
        assert_eq!(source_limited.max_recovery_bytes(), 4096);
        for (cpu, gpu) in [
            (scene.max_mesh_cpu_bytes(), 0),
            (0, scene.max_mesh_gpu_bytes()),
            (usize::MAX, usize::MAX),
        ] {
            assert!(remaining_upload_budget(limits, cpu, gpu).is_err());
        }
    }
}

//! Scene-owned GPU revisions, with only immutable CPU identities in host slots.

use super::*;
use crate::three_d::{MeshAsset3d, ResolvedMesh3d, TextureVisual3d};
use sim_engine::{DynamicMesh3dBudget, Mesh3dUploadBudget, SurfaceLighting3d};

pub(super) struct CustomMeshes {
    slots: Vec<MeshSlot>,
    pub(super) updates: DesktopThreeDUpdates,
}

pub(super) struct MeshSlot {
    pub(super) asset: MeshAsset3d,
    pub(super) texture: Option<TextureVisual3d>,
    pub(super) scene: SceneSlot,
}

impl CustomMeshes {
    pub(super) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            updates: DesktopThreeDUpdates::empty(),
        }
    }

    pub(super) fn clear(&mut self) {
        self.slots.clear();
        self.updates = DesktopThreeDUpdates::empty();
    }

    pub(super) fn clear_slots(&mut self) {
        self.clear();
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
        renderer: &mut WgpuRenderer,
        scene: &mut Scene3d,
        records: &[ResolvedMesh3d],
        limits: ThreeDRenderLimits,
    ) -> Result<(), DesktopThreeDError> {
        self.updates = DesktopThreeDUpdates::empty();
        for index in (0..self.slots.len()).rev() {
            let slot = &self.slots[index];
            let source = slot.scene.applied.source;
            let incoming = records
                .binary_search_by_key(&source.stable_bits(), |record| {
                    record.source().stable_bits()
                })
                .ok()
                .map(|index| records[index].source());
            if !same_source(source, incoming) {
                scene
                    .remove(slot.scene.id)
                    .map_err(DesktopThreeDError::Scene)?;
                self.slots.remove(index);
            }
        }

        for record in records {
            let source = record.source();
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
                Ok(index) => self.update(index, renderer, scene, record, desired, limits)?,
                Err(index) => {
                    reserve_one(&mut self.slots)?;
                    let mesh = self.initial_mesh(renderer, scene, record, limits)?;
                    let id = scene
                        .try_push(&mesh, desired.transform, desired.style)
                        .map_err(|error| DesktopThreeDError::Object { source, error })?;
                    // No host GPU clone survives insertion. Unique bundles are
                    // scene-owned; other objects remain explicit immutable aliases.
                    self.slots.insert(
                        index,
                        MeshSlot {
                            asset: record.visual().asset().clone(),
                            texture: record.visual().texture().cloned(),
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

    fn initial_mesh(
        &self,
        renderer: &WgpuRenderer,
        scene: &Scene3d,
        record: &ResolvedMesh3d,
        limits: ThreeDRenderLimits,
    ) -> Result<RetainedMesh3d, DesktopThreeDError> {
        let incoming = record.visual();
        let shared = self
            .slots
            .iter()
            .find(|slot| slot.asset.shares_storage(incoming.asset()));
        let mesh = if let Some(slot) = shared {
            scene
                .instance(slot.scene.id)
                .map_err(DesktopThreeDError::Scene)?
                .mesh()
                .clone()
        } else {
            renderer
                .create_mesh3d_with_budget(
                    incoming.asset().mesh().clone(),
                    upload_budget(limits, scene, false)?,
                )
                .map_err(DesktopThreeDError::Resource)?
        };
        if let Some(texture) = incoming.texture() {
            textures::bind(
                renderer,
                scene,
                &self.slots,
                &mesh,
                record.source(),
                texture,
            )
        } else {
            // A material-free alias retains all shared topology/attributes.
            // No second geometry upload is needed for an untextured instance.
            Ok(mesh.without_material())
        }
    }

    fn update(
        &mut self,
        index: usize,
        renderer: &mut WgpuRenderer,
        scene: &mut Scene3d,
        record: &ResolvedMesh3d,
        desired: AppliedState,
        limits: ThreeDRenderLimits,
    ) -> Result<(), DesktopThreeDError> {
        let source = record.source();
        let id = self.slots[index].scene.id;
        let incoming = record.visual();
        let changing_mesh = self.slots[index].asset != *incoming.asset();
        let removing_texture = self.slots[index].texture.is_some() && incoming.texture().is_none();
        if changing_mesh || removing_texture {
            // Old Lambert must not reject normal removal before the final Unlit
            // style can be installed. Record each successful intermediate setter.
            let applied = &mut self.slots[index].scene.applied;
            if incoming.asset().mesh().normals().is_empty()
                && let Some(surface) = applied.style.surface_style()
                && surface.lighting() == SurfaceLighting3d::Lambert
            {
                let unlit = MeshStyle3d::surface(surface.with_lighting(SurfaceLighting3d::Unlit));
                scene
                    .set_style(id, unlit)
                    .map_err(|error| DesktopThreeDError::Object { source, error })?;
                applied.style = unlit;
            }
            if removing_texture {
                // Detach first: a simultaneous geometry edit may remove UVs.
                // Record success before another fallible update so retries do
                // not assume a material that Engine has already removed.
                scene
                    .set_texture_material(id, None)
                    .map_err(|error| DesktopThreeDError::Object { source, error })?;
                self.slots[index].texture = None;
            }
            if changing_mesh {
                let budget = dynamic_budget(limits, scene, id)?;
                let report = renderer
                    .update_scene3d_mesh(scene, id, incoming.asset().mesh().clone(), budget)
                    .map_err(|error| DesktopThreeDError::DynamicMesh { source, error })?;
                self.updates.mesh_updates = self.updates.mesh_updates.saturating_add(1);
                self.updates.mesh_upload_bytes = self
                    .updates
                    .mesh_upload_bytes
                    .saturating_add(report.uploaded_bytes());
                self.updates.mesh_gpu_allocations = self
                    .updates
                    .mesh_gpu_allocations
                    .saturating_add(report.gpu_allocation_count());
            }
            self.slots[index].asset = incoming.asset().clone();
        }
        if let Some(texture) = incoming.texture() {
            textures::update(
                renderer,
                scene,
                &mut self.slots,
                index,
                texture,
                &mut self.updates,
            )?;
        }
        self.slots[index]
            .scene
            .update(scene, desired)
            .map_err(|error| DesktopThreeDError::Object { source, error })
    }
}

fn same_source<Id: Eq>(source: Id, incoming: Option<Id>) -> bool {
    incoming.is_some_and(|incoming| source == incoming)
}

fn upload_budget(
    limits: ThreeDRenderLimits,
    scene: &Scene3d,
    replacing: bool,
) -> Result<Mesh3dUploadBudget, DesktopThreeDError> {
    let budget = scene.budget();
    let statistics = scene.statistics();
    let overlap = usize::from(replacing) + 1;
    Mesh3dUploadBudget::new(
        budget
            .max_mesh_cpu_bytes()
            .saturating_mul(overlap)
            .saturating_sub(statistics.mesh_cpu_bytes())
            .min(limits.max_mesh_source_bytes()),
        budget
            .max_mesh_gpu_bytes()
            .saturating_mul(overlap)
            .saturating_sub(statistics.mesh_gpu_bytes())
            .min(budget.max_mesh_gpu_bytes()),
        Mesh3dUploadBudget::default().max_staging_bytes(),
    )
    .map_err(DesktopThreeDError::Resource)
}

fn dynamic_budget(
    limits: ThreeDRenderLimits,
    scene: &Scene3d,
    id: Object3dId,
) -> Result<DynamicMesh3dBudget, DesktopThreeDError> {
    let upload = upload_budget(limits, scene, true)?;
    let old = scene
        .instance(id)
        .map_err(DesktopThreeDError::Scene)?
        .mesh();
    let statistics = scene.statistics();
    let budget = scene.budget();
    // Aggregate old/new overlap is at most twice the scene ceiling, including
    // unrelated residency. Engine separately enforces exact final deduplication.
    Ok(DynamicMesh3dBudget::new(upload).with_peak_limits(
        aggregate_peak_allowance(
            budget.max_mesh_cpu_bytes(),
            statistics.mesh_cpu_bytes(),
            old.recovery_memory_bytes(),
        ),
        aggregate_peak_allowance(
            budget.max_mesh_gpu_bytes(),
            statistics.mesh_gpu_bytes(),
            old.gpu_allocation_bytes(),
        ),
        upload.max_staging_bytes().saturating_mul(2),
    ))
}

pub(super) fn aggregate_peak_allowance(limit: usize, current: usize, outgoing: usize) -> usize {
    limit
        .saturating_mul(2)
        .saturating_sub(current.saturating_sub(outgoing))
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
    fn retirement_uses_full_entity_generation_not_asset_or_record_position() {
        assert!(same_source((1, 7), Some((1, 7))));
        assert!(!same_source((1, 7), None));
        assert!(!same_source((1, 7), Some((2, 7))));
    }

    #[test]
    fn overlap_allowance_charges_unrelated_residency() {
        assert_eq!(aggregate_peak_allowance(100, 90, 20), 130);
        assert_eq!(aggregate_peak_allowance(100, 100, 1), 101);
        assert_eq!(aggregate_peak_allowance(100, 300, 20), 0);
    }

    #[test]
    fn first_upload_respects_available_aggregate_and_source_caps() {
        let scene = Scene3d::new(sim_engine::Color::BLACK).unwrap();
        let limits = ThreeDRenderLimits::new(0, 100, 1).with_mesh_limits(8, 4096);
        let budget = upload_budget(limits, &scene, false).unwrap();
        assert_eq!(budget.max_recovery_bytes(), 4096);
        assert_eq!(budget.max_gpu_bytes(), scene.budget().max_mesh_gpu_bytes());
    }
}

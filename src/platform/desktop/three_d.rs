//! One retained cube mesh, bounded scene slots, and one reusable depth target.

use std::{error::Error, fmt, mem::size_of};

use sim_engine::{
    DynamicMesh3dError, LogicalViewport, LogicalViewportError, Mesh3d, Mesh3dError,
    Mesh3dRenderBudget, Mesh3dRenderError, Mesh3dRenderReport, Mesh3dResourceError, MeshEdge3d,
    MeshStyle3d, Object3dId, RenderTarget2d, RenderTarget3d, RetainedMesh3d, Scene3d,
    Scene3dBudget, Scene3dError, SurfaceRasterization3d, Texture3dError, Texture3dUpdateError,
    Transform3d, Vec3, WgpuRenderer,
};

use crate::{
    identity::LogicEntity,
    three_d::{
        CORNERS, CuboidVisualError, MeshVisualError, TRIANGLES, ThreeDLimitResource,
        ThreeDRenderLimits, ThreeDSnapshot, ThreeDSurfacePolicy, View3dError,
    },
};

#[path = "three_d/meshes.rs"]
mod meshes;
use meshes::CustomMeshes;
#[path = "three_d/textures.rs"]
mod textures;

/// A bounded 3D preparation or separate depth-prepass failure.
///
/// Immutable mesh uploads, target allocation, and earlier successful prepasses
/// may remain cached when later composition fails. No partial surface frame is
/// presented. Instance-local Engine preflight and object-update failures preserve
/// their current managed source. Camera, target, and aggregate-capacity failures
/// remain scene-wide rather than being attributed to an arbitrary cuboid.
#[derive(Debug)]
pub enum DesktopThreeDError {
    /// An independent 3D allowance was exceeded before preparing GPU resources.
    LimitExceeded {
        /// Resource exceeding its frozen cap.
        resource: ThreeDLimitResource,
        /// Requested resource count.
        requested: u64,
        /// Frozen maximum count.
        limit: u64,
    },
    /// Bounded mesh or scene metadata allocation failed.
    AllocationFailed {
        /// Minimum additional requested bytes.
        requested_bytes: usize,
    },
    /// The current logical viewport was invalid.
    Viewport(LogicalViewportError),
    /// The resize-aware camera could not be represented.
    View(View3dError),
    /// Built-in retained topology construction failed.
    Mesh(Mesh3dError),
    /// Mesh or target creation failed in the current renderer.
    Resource(Mesh3dResourceError),
    /// Retained scene creation or a hidden-slot update failed.
    Scene(Scene3dError),
    /// A managed cuboid's validated presentation could not form its GPU style.
    Cuboid {
        /// Source requesting the failed value.
        source: LogicEntity,
        /// Concrete value error.
        error: CuboidVisualError,
    },
    /// A custom mesh's validated value could not form its GPU style.
    CustomMesh {
        /// Source requesting the failed value.
        source: LogicEntity,
        /// Concrete value error.
        error: MeshVisualError,
    },
    /// Engine rejected a scene-owned mesh revision before publishing it.
    DynamicMesh {
        /// Managed source of the revision.
        source: LogicEntity,
        /// Original bounded update error.
        error: DynamicMesh3dError,
    },
    /// Texture creation or material binding failed.
    Texture {
        /// Managed source of the texture.
        source: LogicEntity,
        /// Original Engine texture error.
        error: Texture3dError,
    },
    /// An isolated texture patch failed before publication.
    TextureUpdate {
        /// Managed source of the patch.
        source: LogicEntity,
        /// Original Engine update error.
        error: Texture3dUpdateError,
    },
    /// A managed 3D object could not be inserted or updated in the retained scene.
    Object {
        /// Source requesting the failed scene update.
        source: LogicEntity,
        /// Concrete scene failure.
        error: Scene3dError,
    },
    /// Engine's authoritative preflight rejected one managed 3D object.
    ObjectRender {
        /// Current managed source occupying the retained Engine slot.
        source: LogicEntity,
        /// Concrete preflight failure, retaining its complete Engine object ID.
        error: Mesh3dRenderError,
    },
    /// Engine rejected the scene or an object with no current managed source.
    ///
    /// Camera, ownership, and aggregate-capacity failures are scene-wide. An
    /// unrecognized Engine object ID remains in the error without guessing its
    /// source or discarding the original diagnostic.
    Render(Mesh3dRenderError),
    /// Private retained cache state was inconsistent.
    InvalidCache,
    /// The surface format or target byte count could not be represented.
    InvalidTargetLayout,
}

impl fmt::Display for DesktopThreeDError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LimitExceeded {
                resource,
                requested,
                limit,
            } => write!(
                formatter,
                "desktop 3D {resource:?} requested {requested}, exceeding {limit}"
            ),
            Self::AllocationFailed { requested_bytes } => write!(
                formatter,
                "desktop 3D could not reserve {requested_bytes} additional bytes"
            ),
            Self::Viewport(error) => write!(formatter, "3D target viewport failed: {error}"),
            Self::View(error) => write!(formatter, "3D camera preparation failed: {error}"),
            Self::Mesh(error) => write!(formatter, "built-in cube topology failed: {error}"),
            Self::Resource(error) => write!(formatter, "retained 3D resource failed: {error}"),
            Self::Scene(error) => write!(formatter, "retained 3D scene failed: {error}"),
            Self::Cuboid { source, error } => {
                write!(formatter, "3D cuboid {source:?} style failed: {error}")
            }
            Self::CustomMesh { source, error } => {
                write!(formatter, "3D mesh {source:?} style failed: {error}")
            }
            Self::DynamicMesh { source, error } => {
                write!(formatter, "3D mesh {source:?} update failed: {error}")
            }
            Self::Texture { source, error } => {
                write!(formatter, "3D texture {source:?} failed: {error}")
            }
            Self::TextureUpdate { source, error } => {
                write!(formatter, "3D texture {source:?} patch failed: {error}")
            }
            Self::Object { source, error } => write!(
                formatter,
                "3D object {source:?} scene update failed: {error}"
            ),
            Self::ObjectRender { source, error } => write!(
                formatter,
                "3D object {source:?} depth prepass failed: {error}"
            ),
            Self::Render(error) => write!(formatter, "3D depth prepass failed: {error}"),
            Self::InvalidCache => formatter.write_str("retained 3D cache is inconsistent"),
            Self::InvalidTargetLayout => {
                formatter.write_str("3D color-target byte layout is unrepresentable")
            }
        }
    }
}

impl Error for DesktopThreeDError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Viewport(error) => Some(error),
            Self::View(error) => Some(error),
            Self::Mesh(error) => Some(error),
            Self::Resource(error) => Some(error),
            Self::Scene(error) => Some(error),
            Self::Cuboid { error, .. } => Some(error),
            Self::CustomMesh { error, .. } => Some(error),
            Self::DynamicMesh { error, .. } => Some(error),
            Self::Texture { error, .. } => Some(error),
            Self::TextureUpdate { error, .. } => Some(error),
            Self::Object { error, .. } => Some(error),
            Self::ObjectRender { error, .. } => Some(error),
            Self::Render(error) => Some(error),
            _ => None,
        }
    }
}

/// Actual uploads performed by scene-owned updates during one 3D preparation.
/// Initial resource creation and ordinary frame uniforms are not included.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesktopThreeDUpdates {
    /// Accepted mesh revision count, including alias detachment and growth.
    pub mesh_updates: usize,
    /// Bytes uploaded by accepted mesh revisions.
    pub mesh_upload_bytes: usize,
    /// New GPU buffers allocated by mesh revisions, not initial creation.
    pub mesh_gpu_allocations: usize,
    /// Accepted immediate-parent texture patches.
    pub texture_updates: usize,
    /// Patch and regenerated lower-mip upload bytes.
    pub texture_upload_bytes: usize,
    /// New GPU textures allocated by patches, including alias detachment.
    pub texture_gpu_allocations: usize,
}

impl DesktopThreeDUpdates {
    pub(super) fn accumulate(&mut self, other: Self) {
        self.mesh_updates = self.mesh_updates.saturating_add(other.mesh_updates);
        self.mesh_upload_bytes = self
            .mesh_upload_bytes
            .saturating_add(other.mesh_upload_bytes);
        self.mesh_gpu_allocations = self
            .mesh_gpu_allocations
            .saturating_add(other.mesh_gpu_allocations);
        self.texture_updates = self.texture_updates.saturating_add(other.texture_updates);
        self.texture_upload_bytes = self
            .texture_upload_bytes
            .saturating_add(other.texture_upload_bytes);
        self.texture_gpu_allocations = self
            .texture_gpu_allocations
            .saturating_add(other.texture_gpu_allocations);
    }
    const fn empty() -> Self {
        Self {
            mesh_updates: 0,
            mesh_upload_bytes: 0,
            mesh_gpu_allocations: 0,
            texture_updates: 0,
            texture_upload_bytes: 0,
            texture_gpu_allocations: 0,
        }
    }
}

pub(super) struct DesktopThreeD {
    mesh: Option<RetainedMesh3d>,
    scene: Option<Scene3d>,
    slots: Vec<SceneSlot>,
    custom: CustomMeshes,
    target: TargetCache<RenderTarget3d>,
}

impl DesktopThreeD {
    pub(super) const fn new() -> Self {
        Self {
            mesh: None,
            scene: None,
            slots: Vec::new(),
            custom: CustomMeshes::new(),
            target: TargetCache::new(),
        }
    }

    /// Invalidates renderer-bound resources after recovery or renderer creation.
    pub(super) fn clear(&mut self) {
        self.mesh = None;
        self.scene = None;
        self.slots.clear();
        self.custom.clear();
        self.target.clear();
    }

    pub(super) fn color_target(&self) -> Option<&RenderTarget2d> {
        self.target.get().map(RenderTarget3d::color_target)
    }

    pub(super) fn updates(&self) -> DesktopThreeDUpdates {
        self.custom.updates
    }

    /// Restore the scene as a unit: Engine preserves IDs, aliases and material
    /// settings. CPU slot metadata remains valid; targets are recreated lazily.
    pub(super) fn restore(&mut self, renderer: &WgpuRenderer) -> Result<(), DesktopThreeDError> {
        if let Some(scene) = &mut self.scene {
            renderer
                .restore_scene3d(scene)
                .map_err(DesktopThreeDError::Resource)?;
        }
        if let Some(slot) = self.slots.first() {
            self.mesh = Some(
                self.scene
                    .as_ref()
                    .ok_or(DesktopThreeDError::InvalidCache)?
                    .instance(slot.id)
                    .map_err(DesktopThreeDError::Scene)?
                    .mesh()
                    .clone(),
            );
        } else if let Some(mesh) = &self.mesh {
            // This built-in mesh has no scene instance. Custom meshes restore
            // with their scene so shared resources and stable IDs remain intact.
            self.mesh = Some(
                renderer
                    .restore_mesh3d(mesh)
                    .map_err(DesktopThreeDError::Resource)?,
            );
        }
        self.target.clear();
        Ok(())
    }

    pub(super) fn prepare(
        &mut self,
        renderer: &mut WgpuRenderer,
        snapshot: ThreeDSnapshot<'_>,
        limits: ThreeDRenderLimits,
    ) -> Result<Mesh3dRenderReport, DesktopThreeDError> {
        let (width, height) = renderer.size();
        check_limits(snapshot.cuboids().len(), width, height, limits)?;
        let render_budget = snapshot_render_budget(snapshot, limits);
        let viewport = renderer
            .logical_viewport()
            .map_err(DesktopThreeDError::Viewport)?;
        let camera = snapshot
            .view()
            .camera(viewport)
            .map_err(DesktopThreeDError::View)?;
        if self.mesh.is_none() {
            let mesh = unit_cube()?;
            self.mesh = Some(
                renderer
                    .create_mesh3d(mesh)
                    .map_err(DesktopThreeDError::Resource)?,
            );
        }
        let mesh = self.mesh.as_ref().ok_or(DesktopThreeDError::InvalidCache)?;
        if self.scene.is_none() {
            let scene = Scene3d::with_alpha_background_and_budget(
                snapshot.view().background(),
                engine_scene_budget(limits)?,
            )
            .map_err(DesktopThreeDError::Scene)?;
            self.scene = Some(scene);
            // New scene IDs also invalidate every last-applied field together.
            self.slots.clear();
            self.custom.clear_slots();
        }
        let scene = self
            .scene
            .as_mut()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        // Clear-color changes must not discard object IDs, dynamic capacities
        // or texture lineage. Engine validates and mutates this field alone.
        scene
            .set_background(snapshot.view().background())
            .map_err(DesktopThreeDError::Scene)?;
        scene.set_lighting(snapshot.view().lighting());
        scene.set_fog(snapshot.view().fog());
        for (index, record) in snapshot.cuboids().iter().enumerate() {
            let source = record.source();
            let style = record
                .visual()
                .style()
                .map_err(|error| DesktopThreeDError::Cuboid { source, error })?;
            let desired = AppliedState {
                source,
                transform: record.transform(),
                style,
                visible: true,
            };
            if index == self.slots.len() {
                if self.slots.len() == self.slots.capacity() {
                    self.slots.try_reserve_exact(1).map_err(|_| {
                        DesktopThreeDError::AllocationFailed {
                            requested_bytes: size_of::<SceneSlot>(),
                        }
                    })?;
                }
                let id = scene
                    .try_push(mesh, record.transform(), style)
                    .map_err(|error| DesktopThreeDError::Object { source, error })?;
                // A successful insertion already applies all three fields.
                self.slots.push(SceneSlot {
                    id,
                    applied: desired,
                });
            } else {
                self.slots[index]
                    .update(scene, desired)
                    .map_err(|error| DesktopThreeDError::Object { source, error })?;
            }
        }
        for slot in &mut self.slots[snapshot.cuboids().len()..] {
            let hidden = AppliedState {
                visible: false,
                ..slot.applied
            };
            slot.update(scene, hidden)
                .map_err(DesktopThreeDError::Scene)?;
        }
        self.custom
            .prepare(renderer, scene, snapshot.meshes(), limits)?;
        let descriptor = TargetDescriptor {
            width,
            height,
            viewport,
        };
        if !self.target.matches(descriptor) {
            // Engine 0.3 composition caches bindings that retain this target's
            // texture. Release those host references before replacing it, not
            // only our target handle. Steady-state frames keep their cache.
            renderer.clear_frame_cache();
        }
        self.target.ensure(descriptor, || {
            renderer
                .create_render_target3d(width, height, viewport)
                .map_err(DesktopThreeDError::Resource)
        })?;
        let target = self.target.get().ok_or(DesktopThreeDError::InvalidCache)?;
        renderer
            .render_scene3d_to_target_with_budget(target, scene, camera, render_budget)
            .map_err(|error| {
                if let Some(source) = error
                    .object_id()
                    .and_then(|id| find_object_source(id, self.custom.object_sources()))
                {
                    DesktopThreeDError::ObjectRender { source, error }
                } else {
                    map_render_error(error, &self.slots)
                }
            })
    }
}

struct SceneSlot {
    id: Object3dId,
    applied: AppliedState,
}

impl SceneSlot {
    fn update(&mut self, scene: &mut Scene3d, desired: AppliedState) -> Result<(), Scene3dError> {
        let id = self.id;
        self.applied.update(desired, |change| match change {
            SlotChange::Transform(transform) => scene.set_transform(id, transform),
            SlotChange::Style(style) => scene.set_style(id, style),
            SlotChange::Visible(visible) => scene.set_visible(id, visible),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct AppliedState {
    source: LogicEntity,
    transform: Transform3d,
    style: MeshStyle3d,
    visible: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SlotChange {
    Transform(Transform3d),
    Style(MeshStyle3d),
    Visible(bool),
}

impl AppliedState {
    fn update<E>(
        &mut self,
        desired: Self,
        mut apply: impl FnMut(SlotChange) -> Result<(), E>,
    ) -> Result<(), E> {
        // Engine 0.3 indexes retained IDs, but unchanged fields still avoid
        // unnecessary validation and setter work, including hidden slots.
        // Record only successful setters, so a later failed setter never makes
        // this cache claim a value that the retained scene has not accepted.
        if self.transform != desired.transform {
            apply(SlotChange::Transform(desired.transform))?;
            self.transform = desired.transform;
        }
        if self.style != desired.style {
            apply(SlotChange::Style(desired.style))?;
            self.style = desired.style;
        }
        if self.visible != desired.visible {
            apply(SlotChange::Visible(desired.visible))?;
            self.visible = desired.visible;
        }
        // Slots are reused by snapshot order, not permanently bound to an ECS
        // entity. Equal geometry must still refresh attribution after despawn,
        // reorder, or World replacement. A failed update is not rendered.
        self.source = desired.source;
        Ok(())
    }
}

fn map_render_error(error: Mesh3dRenderError, slots: &[SceneSlot]) -> DesktopThreeDError {
    let source = error.object_id().and_then(|id| {
        find_object_source(
            id,
            slots
                .iter()
                .map(|slot| (slot.id, slot.applied.source, slot.applied.visible)),
        )
    });
    match source {
        Some(source) => DesktopThreeDError::ObjectRender { source, error },
        None => DesktopThreeDError::Render(error),
    }
}

fn find_object_source<Id: Eq>(
    id: Id,
    slots: impl IntoIterator<Item = (Id, LogicEntity, bool)>,
) -> Option<LogicEntity> {
    // Compare complete opaque handles, never their scene-local diagnostic
    // numbers. This scan runs only on rejected prepasses, not every frame.
    slots
        .into_iter()
        .find_map(|(slot_id, source, visible)| (visible && slot_id == id).then_some(source))
}

fn engine_scene_budget(limits: ThreeDRenderLimits) -> Result<Scene3dBudget, DesktopThreeDError> {
    let defaults = Scene3dBudget::default();
    // Engine requires a nonzero object ceiling even for a background-only
    // scene. Logic's earlier count check still prohibits objects at a zero cap.
    // Keep finite Engine geometry ceilings. CPU texture source limits cover
    // base snapshots; the GPU texel allowance also covers Engine recovery mips.
    Scene3dBudget::new(
        limits
            .max_cuboids()
            .saturating_add(limits.max_meshes())
            .max(1),
        defaults.max_storage_bytes(),
        defaults.max_mesh_cpu_bytes(),
        defaults.max_mesh_gpu_bytes(),
    )
    .map(|budget| {
        budget.with_texture_limits(
            limits.max_texture_gpu_bytes(),
            limits.max_texture_gpu_bytes(),
        )
    })
    .map_err(DesktopThreeDError::Scene)
}

#[cfg(test)]
fn engine_render_budget(count: usize, limits: ThreeDRenderLimits) -> Mesh3dRenderBudget {
    generated_budget(count != 0, limits)
}

fn snapshot_render_budget(
    snapshot: ThreeDSnapshot<'_>,
    limits: ThreeDRenderLimits,
) -> Mesh3dRenderBudget {
    generated_budget(snapshot.source_triangle_count() != 0, limits).with_surface_policy(
        match snapshot.view().surface_policy() {
            ThreeDSurfacePolicy::StrictPortable => SurfaceRasterization3d::StrictPortable,
            ThreeDSurfacePolicy::Native => SurfaceRasterization3d::Native,
        },
    )
}

fn generated_budget(has_objects: bool, limits: ThreeDRenderLimits) -> Mesh3dRenderBudget {
    let defaults = Mesh3dRenderBudget::default();
    let triangles = if !has_objects {
        0
    } else {
        limits
            .max_triangles()
            .min(defaults.max_generated_triangles())
    };
    Mesh3dRenderBudget::new(
        triangles
            .saturating_mul(3)
            .min(defaults.max_generated_vertices()),
        triangles,
        // Engine owns its generated vertex/edge layouts. Do not encode their
        // private byte strides here; retain its explicit finite upload ceiling.
        defaults.max_generated_upload_bytes(),
    )
    .with_max_surface_triangles(limits.max_triangles())
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct TargetDescriptor {
    width: u32,
    height: u32,
    viewport: LogicalViewport,
}

struct TargetCache<T> {
    entry: Option<(TargetDescriptor, T)>,
}

impl<T> TargetCache<T> {
    const fn new() -> Self {
        Self { entry: None }
    }

    fn clear(&mut self) {
        self.entry = None;
    }

    fn get(&self) -> Option<&T> {
        self.entry.as_ref().map(|(_, target)| target)
    }

    fn matches(&self, descriptor: TargetDescriptor) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|(key, _)| *key == descriptor)
    }

    fn ensure<E>(
        &mut self,
        descriptor: TargetDescriptor,
        create: impl FnOnce() -> Result<T, E>,
    ) -> Result<(), E> {
        if self.matches(descriptor) {
            return Ok(());
        }
        // Release obsolete resources before allocation, including failed resize.
        self.clear();
        self.entry = Some((descriptor, create()?));
        Ok(())
    }
}

fn check_limits(
    count: usize,
    width: u32,
    height: u32,
    limits: ThreeDRenderLimits,
) -> Result<(), DesktopThreeDError> {
    let triangles = (count as u64)
        .checked_mul(12)
        .ok_or(DesktopThreeDError::LimitExceeded {
            resource: ThreeDLimitResource::Triangles,
            requested: u64::MAX,
            limit: limits.max_triangles() as u64,
        })?;
    for (resource, requested, limit) in [
        (
            ThreeDLimitResource::Cuboids,
            count as u64,
            limits.max_cuboids() as u64,
        ),
        (
            ThreeDLimitResource::Triangles,
            triangles,
            limits.max_triangles() as u64,
        ),
        (
            ThreeDLimitResource::TargetPixels,
            u64::from(width) * u64::from(height),
            limits.max_target_pixels(),
        ),
    ] {
        if requested > limit {
            return Err(DesktopThreeDError::LimitExceeded {
                resource,
                requested,
                limit,
            });
        }
    }
    Ok(())
}

pub(super) fn color_target_bytes(renderer: &WgpuRenderer) -> Result<usize, DesktopThreeDError> {
    let (width, height) = renderer.size();
    let bytes_per_texel = renderer
        .surface_format()
        .block_copy_size(None)
        .ok_or(DesktopThreeDError::InvalidTargetLayout)?;
    let bytes = u64::from(width)
        .checked_mul(u64::from(height))
        .and_then(|pixels| pixels.checked_mul(u64::from(bytes_per_texel)))
        .ok_or(DesktopThreeDError::InvalidTargetLayout)?;
    usize::try_from(bytes).map_err(|_| DesktopThreeDError::InvalidTargetLayout)
}

fn reserved<T>(capacity: usize) -> Result<Vec<T>, DesktopThreeDError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| DesktopThreeDError::AllocationFailed {
            requested_bytes: capacity.saturating_mul(size_of::<T>()),
        })?;
    Ok(values)
}

fn unit_cube() -> Result<Mesh3d, DesktopThreeDError> {
    let mut vertices = reserved(8)?;
    for [x, y, z] in CORNERS {
        vertices.push(
            Vec3::new(x, y, z)
                .map_err(|error| DesktopThreeDError::View(View3dError::Geometry(error)))?,
        );
    }
    let mut triangles = reserved(36)?;
    triangles.extend_from_slice(&TRIANGLES);
    let mut edges = reserved(12)?;
    for (start, end) in [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        edges.push(MeshEdge3d::new(start, end).map_err(DesktopThreeDError::Mesh)?);
    }
    Mesh3d::with_display_edges(vertices, triangles, edges).map_err(DesktopThreeDError::Mesh)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{ApplicationId, WorldGeneration};
    use bevy_ecs::entity::Entity;
    use sim_engine::{Color, Rotation3d, SurfaceStyle3d};

    fn source(sequence: u64, row: u32) -> LogicEntity {
        let application = ApplicationId::from_raw(1);
        LogicEntity::new(
            application,
            WorldGeneration::new(application, sequence),
            Entity::from_raw_u32(row).unwrap(),
        )
    }

    fn applied_state() -> AppliedState {
        AppliedState {
            source: source(1, 0),
            transform: Transform3d::IDENTITY,
            style: MeshStyle3d::surface(SurfaceStyle3d::opaque(Color::WHITE).unwrap()),
            visible: true,
        }
    }

    fn changed_state() -> AppliedState {
        AppliedState {
            source: source(1, 0),
            transform: Transform3d::new(
                Vec3::new(1.0, 2.0, 3.0).unwrap(),
                Rotation3d::IDENTITY,
                Vec3::new(2.0, 1.0, 3.0).unwrap(),
            )
            .unwrap(),
            style: MeshStyle3d::surface(SurfaceStyle3d::opaque(Color::BLACK).unwrap()),
            visible: false,
        }
    }

    #[test]
    fn unchanged_slots_never_invoke_engine_setters() {
        let mut states = [applied_state(); 64];
        let mut desired = states;
        for _ in 0..10 {
            for (applied, desired) in states.iter_mut().zip(desired) {
                applied
                    .update(desired, |_| -> Result<(), ()> {
                        panic!("unchanged cuboid must not perform an Engine lookup")
                    })
                    .unwrap();
            }
        }
        desired[57] = AppliedState {
            visible: true,
            ..changed_state()
        };
        let mut changes = Vec::new();
        for (index, (applied, desired)) in states.iter_mut().zip(desired).enumerate() {
            applied
                .update(desired, |change| -> Result<(), ()> {
                    changes.push((index, change));
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(
            changes,
            [
                (57, SlotChange::Transform(desired[57].transform)),
                (57, SlotChange::Style(desired[57].style)),
            ]
        );
        assert_eq!(states, desired);
    }

    #[test]
    fn diff_policy_updates_only_each_changed_field() {
        let initial = applied_state();
        let changed = changed_state();
        for (desired, expected) in [
            (
                AppliedState {
                    transform: changed.transform,
                    ..initial
                },
                SlotChange::Transform(changed.transform),
            ),
            (
                AppliedState {
                    style: changed.style,
                    ..initial
                },
                SlotChange::Style(changed.style),
            ),
            (
                AppliedState {
                    visible: false,
                    ..initial
                },
                SlotChange::Visible(false),
            ),
        ] {
            let mut applied = initial;
            let mut changes = Vec::new();
            applied
                .update(desired, |change| -> Result<(), ()> {
                    changes.push(change);
                    Ok(())
                })
                .unwrap();
            assert_eq!(changes, [expected]);
            assert_eq!(applied, desired);
        }
    }

    #[test]
    fn hidden_slots_change_visibility_only_once_then_reactivate() {
        let initial = applied_state();
        let hidden = AppliedState {
            visible: false,
            ..initial
        };
        let mut applied = initial;
        let mut changes = Vec::new();
        for desired in [hidden, hidden, hidden, initial, initial] {
            applied
                .update(desired, |change| -> Result<(), ()> {
                    changes.push(change);
                    Ok(())
                })
                .unwrap();
        }
        assert_eq!(
            changes,
            [SlotChange::Visible(false), SlotChange::Visible(true)]
        );
        assert_eq!(applied, initial);
    }

    #[test]
    fn partial_failure_records_only_successful_updates_and_retries_the_rest() {
        let initial = applied_state();
        let desired = changed_state();
        let ordered = [
            SlotChange::Transform(desired.transform),
            SlotChange::Style(desired.style),
            SlotChange::Visible(desired.visible),
        ];
        for failure_index in 0..ordered.len() {
            let mut applied = initial;
            let mut attempted = Vec::new();
            let failed = applied.update(desired, |change| {
                attempted.push(change);
                if change == ordered[failure_index] {
                    Err("setter failed")
                } else {
                    Ok(())
                }
            });
            assert_eq!(failed, Err("setter failed"));
            assert_eq!(attempted, ordered[..=failure_index]);
            assert_eq!(
                applied.transform,
                if failure_index > 0 {
                    desired.transform
                } else {
                    initial.transform
                }
            );
            assert_eq!(
                applied.style,
                if failure_index > 1 {
                    desired.style
                } else {
                    initial.style
                }
            );
            assert_eq!(applied.visible, initial.visible);
            attempted.clear();
            applied
                .update(desired, |change| -> Result<(), ()> {
                    attempted.push(change);
                    Ok(())
                })
                .unwrap();
            assert_eq!(attempted, ordered[failure_index..]);
            assert_eq!(applied, desired);
        }
    }

    #[test]
    fn retained_cube_has_exact_solid_and_outline_topology() {
        let mesh = unit_cube().unwrap();
        assert_eq!(mesh.vertices().len(), 8);
        assert_eq!(mesh.triangle_count(), 12);
        assert_eq!(mesh.display_edges().len(), 12);
    }

    #[test]
    fn independent_prepass_limits_reject_before_gpu_work() {
        let limits = ThreeDRenderLimits::new(2, 24, 640 * 360);
        assert!(check_limits(2, 640, 360, limits).is_ok());
        assert!(matches!(
            check_limits(3, 640, 360, limits),
            Err(DesktopThreeDError::LimitExceeded {
                resource: ThreeDLimitResource::Cuboids,
                ..
            })
        ));
        assert!(matches!(
            check_limits(2, 1280, 720, limits),
            Err(DesktopThreeDError::LimitExceeded {
                resource: ThreeDLimitResource::TargetPixels,
                ..
            })
        ));
        assert!(matches!(
            check_limits(2, 640, 360, ThreeDRenderLimits::new(2, 23, 640 * 360)),
            Err(DesktopThreeDError::LimitExceeded {
                resource: ThreeDLimitResource::Triangles,
                ..
            })
        ));
        assert!(
            check_limits(
                0,
                u32::MAX,
                u32::MAX,
                ThreeDRenderLimits::new(0, 0, u64::MAX)
            )
            .is_ok()
        );
    }

    #[test]
    fn cache_invalidation_discards_all_renderer_bound_resources() {
        let mut cache = DesktopThreeD::new();
        assert_eq!(cache.slots.capacity(), 0);
        cache.clear();
        assert!(cache.mesh.is_none());
        assert!(cache.scene.is_none());
        assert!(cache.target.get().is_none());
        assert!(cache.slots.is_empty());
    }

    #[test]
    fn target_cache_reuses_then_recreates_after_resize_scale_or_recovery() {
        let initial = TargetDescriptor {
            width: 1280,
            height: 720,
            viewport: LogicalViewport::new(1280.0, 720.0).unwrap(),
        };
        let scaled = TargetDescriptor {
            viewport: LogicalViewport::new(640.0, 360.0).unwrap(),
            ..initial
        };
        let resized = TargetDescriptor {
            width: 960,
            height: 540,
            viewport: LogicalViewport::new(960.0, 540.0).unwrap(),
        };
        let mut cache = TargetCache::new();
        let mut calls = 0;
        for descriptor in [initial, initial, scaled, scaled, resized] {
            cache
                .ensure(descriptor, || -> Result<usize, ()> {
                    calls += 1;
                    Ok(calls)
                })
                .unwrap();
        }
        assert_eq!(calls, 3);
        assert_eq!(cache.get(), Some(&3));
        cache.clear();
        assert!(cache.get().is_none());
        cache
            .ensure(resized, || -> Result<usize, ()> {
                calls += 1;
                Ok(calls)
            })
            .unwrap();
        assert_eq!(calls, 4);
        let failed = cache.ensure(initial, || -> Result<usize, &'static str> {
            Err("target allocation")
        });
        assert_eq!(failed, Err("target allocation"));
        assert!(cache.get().is_none());
    }
}

#[cfg(test)]
#[path = "three_d/migration_tests.rs"]
mod migration_tests;

#[cfg(all(test, target_os = "linux"))]
#[path = "three_d/gpu_tests.rs"]
mod gpu_tests;

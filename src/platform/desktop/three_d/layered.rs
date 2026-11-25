//! Two retained partitions and transactional Engine-owned layered targets.

use super::*;
use crate::three_d::{
    LayeredDimensions3d, LayeredRejection3d, LayeredView3d, RenderLayer3d, ResolvedCuboid3d,
    ResolvedMesh3d,
};
use sim_engine::{
    LayeredRenderError3d, LayeredTarget3d, Mesh3dRenderReport, RenderTargetBudget,
    RenderTargetLifecycleError,
};
use std::{
    mem::size_of,
    time::{Duration, Instant},
};

/// Attachment operation performed before this frame's complete layered redraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayeredTargetAction {
    /// Reused existing attachments, including effect-only edits.
    Reused,
    /// Created attachments for a previously absent layered target.
    Created,
    /// Transactionally replaced attachments while charging previous plus new bytes.
    Replaced,
}

/// Exact logical and renderer provenance of a layered submission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopLayeredProvenance {
    /// Runner frame index which supplied the complete snapshot.
    pub frame_index: u64,
    /// Snapshot's managed World generation.
    pub world: crate::identity::WorldGeneration,
    /// Host device generation, zero initially.
    pub device_generation: u64,
    /// Host configuration revision; changes on World/settings changes, not every frame.
    pub configuration_revision: u64,
}

/// Saturating successful layered lifecycle totals for this desktop run.
/// Failed preparations and opaque driver retirement are excluded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DesktopLayeredStatistics {
    /// New layered attachment bundles followed by successful redraw.
    pub created: u64,
    /// Transactional replacements followed by successful redraw.
    pub replaced: u64,
    /// Compatible attachment reuse followed by successful redraw.
    pub reused: u64,
    /// Successful restoration of both retained scenes after device replacement.
    pub restored: u64,
}
impl DesktopLayeredStatistics {
    pub(crate) fn record(&mut self, action: LayeredTargetAction) {
        let count = match action {
            LayeredTargetAction::Created => &mut self.created,
            LayeredTargetAction::Replaced => &mut self.replaced,
            LayeredTargetAction::Reused => &mut self.reused,
        };
        *count = count.saturating_add(1);
    }
}

/// One complete layered submission. Window presentation is reported separately.
/// Engine has no final depth-compositor timestamp; it is not reported as zero.
#[derive(Debug, Clone, Copy)]
pub struct DesktopLayeredFrame {
    /// Exact snapshot, device and configuration origin, including on later skipped presents.
    pub provenance: DesktopLayeredProvenance,
    /// Requested settings, before an explicit resolution fallback.
    pub requested: LayeredView3d,
    /// Resolved Base and Detail/output extents in physical texels.
    pub dimensions: LayeredDimensions3d,
    /// Target lifecycle action for this submission.
    pub target_action: LayeredTargetAction,
    /// Current nominal color/depth attachment storage across all four targets.
    pub target_bytes: usize,
    /// Old-plus-new bytes on replacement, current bytes otherwise.
    pub peak_target_bytes: usize,
    /// Base low-resolution workload.
    pub low: Mesh3dRenderReport,
    /// Base visibility workload at Detail/output resolution (also shades).
    pub base_visibility: Mesh3dRenderReport,
    /// Detail workload at Detail/output resolution.
    pub detail: Mesh3dRenderReport,
    /// Number of additional Engine depth-composition draws, without a GPU timestamp.
    pub depth_composition_draws: usize,
    /// CPU partition/resource preparation and target admission wall time.
    pub preparation_time: Duration,
    /// CPU time submitting all three scene passes and final depth composition.
    /// Overlaps asynchronous GPU execution; do not add it to GPU time.
    pub submission_time: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Descriptor {
    surface: (u32, u32),
    viewport: LogicalViewport,
    scale_factor: f64,
    base: (u32, u32),
    detail: (u32, u32),
}

pub(super) struct LayeredDesktop {
    pub(super) scenes: [RetainedScene; 2],
    cuboids: [Vec<ResolvedCuboid3d>; 2],
    meshes: [Vec<ResolvedMesh3d>; 2],
    pub(super) target: Option<LayeredTarget3d>,
    descriptor: Option<Descriptor>,
    rejected: Option<(
        Descriptor,
        crate::three_d::LayeredRenderLimits3d,
        LayeredRejection3d,
    )>,
    ready: bool,
}
impl LayeredDesktop {
    pub(super) const fn new() -> Self {
        Self {
            scenes: [RetainedScene::new(), RetainedScene::new()],
            cuboids: [Vec::new(), Vec::new()],
            meshes: [Vec::new(), Vec::new()],
            target: None,
            descriptor: None,
            rejected: None,
            ready: false,
        }
    }
    pub(super) fn clear(&mut self) {
        *self = Self::new();
    }
    pub(super) fn invalidate(&mut self) {
        // Renderer replacement is the device/format part of the descriptor.
        // Retain old attachment ownership only for replacement overlap accounting.
        // ready=false forbids composition until new-device redraw succeeds.
        self.ready = false;
        self.descriptor = None;
        self.rejected = None;
    }
    pub(super) fn restore(&mut self, renderer: &WgpuRenderer) -> Result<(), DesktopThreeDError> {
        self.invalidate();
        for scene in &mut self.scenes {
            scene.restore(renderer)?;
        }
        Ok(())
    }
    pub(super) fn color_target(&self) -> Option<&RenderTarget2d> {
        self.target
            .as_ref()
            .filter(|_| self.ready)
            .map(|target| target.output().color_target())
    }
    pub(super) fn updates(&self) -> DesktopThreeDUpdates {
        let mut updates = self.scenes[0].custom.updates;
        updates.accumulate(self.scenes[1].custom.updates);
        updates
    }
    fn render_error(&self, error: LayeredRenderError3d) -> DesktopThreeDError {
        if let LayeredRenderError3d::Scene(scene_error) = &error {
            if let Mesh3dRenderError::SurfaceTriangleBudgetExceeded { actual, limit } = *scene_error
            {
                return DesktopThreeDError::LayeredRejected(LayeredRejection3d::SurfaceTriangles {
                    requested: actual,
                    limit,
                });
            }
            if let Some(id) = scene_error.object_id() {
                for (index, scene) in self.scenes.iter().enumerate() {
                    let source =
                        find_object_source(id, scene.custom.object_sources()).or_else(|| {
                            find_object_source(
                                id,
                                scene.slots.iter().map(|slot| {
                                    (slot.id, slot.applied.source, slot.applied.visible)
                                }),
                            )
                        });
                    if let Some(source) = source {
                        return DesktopThreeDError::LayeredObject {
                            layer: if index == 0 {
                                RenderLayer3d::Base
                            } else {
                                RenderLayer3d::Detail
                            },
                            source,
                            error: *scene_error,
                        };
                    }
                }
            }
        }
        DesktopThreeDError::Layered(error)
    }
    pub(super) fn prepare(
        &mut self,
        renderer: &mut WgpuRenderer,
        snapshot: ThreeDSnapshot<'_>,
        limits: ThreeDRenderLimits,
        provenance: DesktopLayeredProvenance,
    ) -> Result<DesktopLayeredFrame, DesktopThreeDError> {
        self.ready = false;
        let start = Instant::now();
        let requested = snapshot
            .view()
            .layered()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        let counts = snapshot
            .validate_layered(limits.layered_limits())
            .map_err(DesktopThreeDError::LayeredRejected)?;
        let viewport = renderer
            .logical_viewport()
            .map_err(DesktopThreeDError::Viewport)?;
        let dimensions = requested
            .resolve(renderer.size(), viewport)
            .map_err(DesktopThreeDError::LayeredRejected)?
            .ok_or(DesktopThreeDError::InvalidTargetLayout)?;
        check_limits(
            snapshot.cuboids().len(),
            dimensions.detail.0,
            dimensions.detail.1,
            limits,
        )
        .map_err(classify_limit)?;
        let descriptor = Descriptor {
            surface: renderer.size(),
            viewport,
            scale_factor: renderer.scale_factor(),
            base: dimensions.base,
            detail: dimensions.detail,
        };
        let allowance = limits.layered_limits();
        if let Some((previous, previous_limits, reason)) = self.rejected
            && previous == descriptor
            && previous_limits == allowance
        {
            return Err(DesktopThreeDError::LayeredRejected(reason));
        }
        let budget =
            RenderTargetBudget::new(allowance.max_target_bytes(), allowance.max_peak_bytes());
        let (action, peak) = if self.descriptor == Some(descriptor) {
            (
                LayeredTargetAction::Reused,
                self.target
                    .as_ref()
                    .ok_or(DesktopThreeDError::InvalidCache)?
                    .allocation_bytes(),
            )
        } else {
            // Release FrameComposer references before overlap accounting. Engine
            // replacement preserves the old allocation when admission fails.
            renderer.clear_frame_cache();
            let result = if let Some(target) = &mut self.target {
                renderer
                    .replace_layered_target3d(
                        target,
                        dimensions.base,
                        dimensions.detail,
                        viewport,
                        budget,
                    )
                    .map(|report| (LayeredTargetAction::Replaced, report.peak_bytes))
            } else {
                renderer
                    .create_layered_target3d(dimensions.base, dimensions.detail, viewport, budget)
                    .map(|target| {
                        let bytes = target.allocation_bytes();
                        self.target = Some(target);
                        (LayeredTargetAction::Created, bytes)
                    })
            };
            match result {
                Ok(value) => {
                    self.descriptor = Some(descriptor);
                    self.rejected = None;
                    value
                }
                Err(LayeredRenderError3d::Target(RenderTargetLifecycleError::BudgetExceeded {
                    target_bytes,
                    peak_bytes,
                    ..
                })) => {
                    let reason = LayeredRejection3d::TargetBudget {
                        target_bytes,
                        peak_bytes,
                    };
                    self.rejected = Some((descriptor, allowance, reason));
                    return Err(DesktopThreeDError::LayeredRejected(reason));
                }
                Err(error) => return Err(DesktopThreeDError::Layered(error)),
            }
        };
        // Complete both partitions before mutating either scene. Reserve remains
        // bounded by extraction limits; release old source revisions on every exit.
        let prepared = (|| {
            for layer in 0..2 {
                self.cuboids[layer].clear();
                self.meshes[layer].clear();
            }
            for record in snapshot.cuboids() {
                push(
                    &mut self.cuboids[record.visual().render_layer().index()],
                    *record,
                )?;
            }
            for record in snapshot.meshes() {
                push(
                    &mut self.meshes[record.visual().render_layer().index()],
                    record.clone(),
                )?;
            }
            // Prune both old partitions before inserting migrated instances.
            for layer in 0..2 {
                self.scenes[layer].prune(ThreeDSnapshot::from_parts(
                    snapshot.view(),
                    &self.cuboids[layer],
                    &self.meshes[layer],
                ))?;
            }
            for layer in 0..2 {
                self.scenes[layer].prepare_scene(
                    renderer,
                    ThreeDSnapshot::from_parts(
                        snapshot.view(),
                        &self.cuboids[layer],
                        &self.meshes[layer],
                    ),
                    limits,
                )?;
            }
            Ok(())
        })();
        for layer in 0..2 {
            self.cuboids[layer].clear();
            self.meshes[layer].clear();
        }
        prepared?;
        let camera = snapshot
            .view()
            .camera(viewport)
            .map_err(DesktopThreeDError::View)?;
        let budgets = std::array::from_fn(|layer| {
            let settings = requested.layers()[layer];
            generated_budget(counts[layer] != 0, limits)
                .with_max_surface_triangles(
                    allowance
                        .max_triangles(if layer == 0 {
                            RenderLayer3d::Base
                        } else {
                            RenderLayer3d::Detail
                        })
                        .min(limits.max_triangles()),
                )
                .with_surface_effects(settings.effects())
                .with_surface_policy(match settings.surface_policy() {
                    ThreeDSurfacePolicy::Native => SurfaceRasterization3d::Native,
                    ThreeDSurfacePolicy::StrictPortable => SurfaceRasterization3d::StrictPortable,
                })
        });
        let target = self
            .target
            .as_ref()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        let base = self.scenes[0]
            .scene
            .as_ref()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        let detail = self.scenes[1]
            .scene
            .as_ref()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        // Surface-only geometry uses the same logical viewport/camera in both
        // Base passes. Engine's exact clipping count is resolution-independent;
        // validate against output to charge generated triangles too. Engine still
        // validates each private target before submitting its layered work.
        let base_work = renderer
            .validate_scene3d_for_target(target.output(), base, camera, budgets[0])
            .map_err(|error| self.render_error(LayeredRenderError3d::Scene(error)))?;
        let detail_work = renderer
            .validate_scene3d_for_target(target.output(), detail, camera, budgets[1])
            .map_err(|error| self.render_error(LayeredRenderError3d::Scene(error)))?;
        let submitted = base_work
            .submitted_triangle_count()
            .saturating_mul(2)
            .saturating_add(detail_work.submitted_triangle_count());
        if submitted > allowance.max_submitted_triangles() {
            return Err(DesktopThreeDError::LayeredRejected(
                LayeredRejection3d::Triangles {
                    layer: None,
                    requested: submitted,
                    limit: allowance.max_submitted_triangles(),
                },
            ));
        }
        let preparation_time = start.elapsed();
        let submitted = Instant::now();
        let reports = renderer
            .render_layered_scene3d(target, base, detail, camera, budgets)
            .map_err(|error| self.render_error(error))?;
        self.ready = true;
        Ok(DesktopLayeredFrame {
            provenance,
            requested,
            dimensions,
            target_action: action,
            target_bytes: target.allocation_bytes(),
            peak_target_bytes: peak,
            low: reports.low,
            base_visibility: reports.base,
            detail: reports.detail,
            depth_composition_draws: 1,
            preparation_time,
            submission_time: submitted.elapsed(),
        })
    }
}
fn push<T>(values: &mut Vec<T>, value: T) -> Result<(), DesktopThreeDError> {
    if values.len() == values.capacity() {
        values
            .try_reserve_exact(1)
            .map_err(|_| DesktopThreeDError::AllocationFailed {
                requested_bytes: size_of::<T>(),
            })?;
    }
    values.push(value);
    Ok(())
}

fn classify_limit(error: DesktopThreeDError) -> DesktopThreeDError {
    match error {
        DesktopThreeDError::LimitExceeded {
            resource,
            requested,
            limit,
        } => DesktopThreeDError::LayeredRejected(LayeredRejection3d::Budget {
            resource,
            requested,
            limit,
        }),
        other => other,
    }
}

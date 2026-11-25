//! CPU contracts for opt-in two-resolution rendering through the stock host.
use super::{ThreeDSnapshot, ThreeDSurfacePolicy};
use crate::identity::{LogicEntity, WorldGeneration};
use sim_engine::{LogicalViewport, SurfaceAlphaMode3d, SurfaceEffects3d};
use std::{error::Error, fmt, num::NonZeroU32};

/// Per-instance assignment; ignored by the ordinary single-scene path.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum RenderLayer3d {
    /// Lower-resolution color with a second visibility pass at output resolution.
    #[default]
    Base,
    /// Detail shading at the shared output/visibility resolution.
    Detail,
}
impl RenderLayer3d {
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Base => 0,
            Self::Detail => 1,
        }
    }
}
/// Physical target dimensions; all resolutions must preserve viewport aspect.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LayerResolution3d {
    /// Full physical surface size, including DPI scaling.
    #[default]
    FullSurface,
    /// Divide both physical axes exactly. Nondivisible dimensions reject.
    DivideSurface(NonZeroU32),
    /// Explicit nonzero physical width and height; supersampling is unsupported.
    FixedPhysical(NonZeroU32, NonZeroU32),
}
/// Action on incompatible target dimensions, never on material or budget errors.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LayerResolutionFallback3d {
    /// Reject without changing the requested camera or aspect.
    #[default]
    Reject,
    /// Use full surface resolution for both layers, still enforcing every budget.
    FullSurface,
}
/// Handling of expected layered presentation rejection after complete extraction.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LayeredFailurePolicy3d {
    /// Return the typed desktop failure, preserving ordinary host behavior.
    #[default]
    Stop,
    /// Present current 2D/UI only; requires registered LayeredPresentationFeedback.
    ContinueUi,
}
/// One pass's CPU settings. Effects default to identity and policy to StrictPortable.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayerSettings3d {
    resolution: LayerResolution3d,
    effects: SurfaceEffects3d,
    policy: ThreeDSurfacePolicy,
}
impl LayerSettings3d {
    /// Creates identity effects at the requested physical resolution.
    pub const fn new(resolution: LayerResolution3d) -> Self {
        Self {
            resolution,
            effects: SurfaceEffects3d::new(),
            policy: ThreeDSurfacePolicy::StrictPortable,
        }
    }
    /// Selects already-validated Engine effects. Compatibility is checked at admission.
    pub const fn with_effects(mut self, effects: SurfaceEffects3d) -> Self {
        self.effects = effects;
        self
    }
    /// Explicitly selects clipping policy; affine UV and snap require Native.
    pub const fn with_surface_policy(mut self, policy: ThreeDSurfacePolicy) -> Self {
        self.policy = policy;
        self
    }
    /// Requested physical target sizing.
    pub const fn resolution(self) -> LayerResolution3d {
        self.resolution
    }
    /// Engine-owned effect values; no Logic shader implementation is involved.
    pub const fn effects(self) -> SurfaceEffects3d {
        self.effects
    }
    /// This pass's policy, overriding View3d's ordinary policy in layered mode.
    pub const fn surface_policy(self) -> ThreeDSurfacePolicy {
        self.policy
    }
}
/// World-owned layered view settings. Detail resolution is also output/depth resolution.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayeredView3d {
    layers: [LayerSettings3d; 2],
    fallback: LayerResolutionFallback3d,
    failure: LayeredFailurePolicy3d,
}
impl LayeredView3d {
    /// Selects Base and Detail settings; defaults reject incompatibility and stop.
    pub const fn new(base: LayerSettings3d, detail: LayerSettings3d) -> Self {
        Self {
            layers: [base, detail],
            fallback: LayerResolutionFallback3d::Reject,
            failure: LayeredFailurePolicy3d::Stop,
        }
    }
    /// Explicit resolution-only fallback; never changes effects or surface policy.
    pub const fn with_resolution_fallback(mut self, fallback: LayerResolutionFallback3d) -> Self {
        self.fallback = fallback;
        self
    }
    /// Sets handling of expected desktop rejection; canonical updates are not rolled back.
    pub const fn with_failure_policy(mut self, failure: LayeredFailurePolicy3d) -> Self {
        self.failure = failure;
        self
    }
    /// Returns independent per-pass settings in Base, Detail order.
    pub const fn layers(self) -> [LayerSettings3d; 2] {
        self.layers
    }
    /// Returns requested failure handling.
    pub const fn failure_policy(self) -> LayeredFailurePolicy3d {
        self.failure
    }
    /// Resolves sizes without allocating or changing pointer/UI coordinates.
    /// Zero surfaces defer work (return None). Exact aspect is preserved; Engine
    /// remains the final authority for device/viewport validity.
    pub fn resolve(
        self,
        surface: (u32, u32),
        viewport: LogicalViewport,
    ) -> Result<Option<LayeredDimensions3d>, LayeredRejection3d> {
        if surface.0 == 0 || surface.1 == 0 {
            return Ok(None);
        }
        let resolve = || {
            let base = resolve_layer(self.layers[0].resolution, surface)?;
            let detail = resolve_layer(self.layers[1].resolution, surface)?;
            if base.0 > detail.0
                || base.1 > detail.1
                || !aspect_matches(base, viewport)
                || !aspect_matches(detail, viewport)
            {
                return Err(LayeredRejection3d::Resolution);
            }
            Ok(LayeredDimensions3d {
                base,
                detail,
                fallback: false,
            })
        };
        match resolve() {
            Ok(value) => Ok(Some(value)),
            Err(_)
                if self.fallback == LayerResolutionFallback3d::FullSurface
                    && aspect_matches(surface, viewport) =>
            {
                Ok(Some(LayeredDimensions3d {
                    base: surface,
                    detail: surface,
                    fallback: true,
                }))
            }
            Err(error) => Err(error),
        }
    }
}
fn resolve_layer(
    request: LayerResolution3d,
    surface: (u32, u32),
) -> Result<(u32, u32), LayeredRejection3d> {
    let size = match request {
        LayerResolution3d::FullSurface => surface,
        LayerResolution3d::DivideSurface(d)
            if surface.0.is_multiple_of(d.get()) && surface.1.is_multiple_of(d.get()) =>
        {
            (surface.0 / d.get(), surface.1 / d.get())
        }
        LayerResolution3d::FixedPhysical(w, h) => (w.get(), h.get()),
        _ => return Err(LayeredRejection3d::Resolution),
    };
    if size.0 == 0 || size.1 == 0 || size.0 > surface.0 || size.1 > surface.1 {
        return Err(LayeredRejection3d::Resolution);
    }
    Ok(size)
}
fn aspect_matches(size: (u32, u32), viewport: LogicalViewport) -> bool {
    // Mirrors Engine's public target acceptance; Engine validates again before allocation.
    let x = f64::from(size.0) / f64::from(viewport.width());
    let y = f64::from(size.1) / f64::from(viewport.height());
    (x - y).abs() <= x.abs().max(y.abs()) * 1e-5
}
/// Effective physical sizes; Detail and final visibility/output share dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayeredDimensions3d {
    /// Base color size in physical texels.
    pub base: (u32, u32),
    /// Detail, base visibility and output size in physical texels.
    pub detail: (u32, u32),
    /// True only when the explicitly selected FullSurface fallback was used.
    pub fallback: bool,
}
/// Frozen additional allowances. Zero defaults disable layered submission.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayeredRenderLimits3d {
    triangles: [usize; 2],
    submitted: usize,
    target_bytes: usize,
    peak_bytes: usize,
}
impl LayeredRenderLimits3d {
    /// Sets per-layer source/submitted triangles, aggregate submissions (2*Base+Detail),
    /// including generated clipping, all four targets' final bytes and old-plus-new replacement peak bytes.
    /// Ordinary CPU source/texture limits and Engine per-scene budgets still apply.
    pub const fn new(
        base_triangles: usize,
        detail_triangles: usize,
        submitted_triangles: usize,
        target_bytes: usize,
        peak_bytes: usize,
    ) -> Self {
        Self {
            triangles: [base_triangles, detail_triangles],
            submitted: submitted_triangles,
            target_bytes,
            peak_bytes,
        }
    }
    /// Per-layer source and Engine-submitted triangle maximum.
    pub const fn max_triangles(self, layer: RenderLayer3d) -> usize {
        self.triangles[layer.index()]
    }
    /// Aggregate admitted triangle submissions, including generated clipping and duplicate Base work.
    pub const fn max_submitted_triangles(self) -> usize {
        self.submitted
    }
    /// Nominal color/depth storage across all four targets.
    pub const fn max_target_bytes(self) -> usize {
        self.target_bytes
    }
    /// Nominal simultaneous previous/replacement attachment bytes.
    pub const fn max_peak_bytes(self) -> usize {
        self.peak_bytes
    }
}
/// Bounded, CPU-readable classification of expected layered presentation rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayeredRejection3d {
    /// An existing independent Logic 3D allowance was exceeded.
    Budget {
        /// Frozen resource category.
        resource: super::ThreeDLimitResource,
        /// Requested count or bytes.
        requested: u64,
        /// Frozen maximum.
        limit: u64,
    },
    /// Engine authoritative preflight exceeded exact submitted surface triangles.
    SurfaceTriangles {
        /// Requested count after Engine clipping/validation.
        requested: usize,
        /// Pass allowance.
        limit: usize,
    },
    /// Requested sizes do not preserve aspect or exceed the surface/detail extent.
    Resolution,
    /// A pass requested Native-only effects under StrictPortable policy.
    EffectsPolicy(RenderLayer3d),
    /// A visible object uses Blend or wireframe in the layered path.
    UnsupportedSurface {
        /// Rejected object's assigned layer.
        layer: RenderLayer3d,
        /// Managed source identity, including its World generation.
        source: LogicEntity,
    },
    /// Triangle source or aggregate submission allowance was exceeded.
    Triangles {
        /// None identifies 2*Base+Detail; Some identifies a single pass.
        layer: Option<RenderLayer3d>,
        /// Required triangle submissions, saturated on arithmetic overflow.
        requested: usize,
        /// Frozen inclusive allowance.
        limit: usize,
    },
    /// Target final/overlap allowance was rejected before allocation.
    TargetBudget {
        /// New four-target nominal storage.
        target_bytes: usize,
        /// Old-plus-new nominal storage.
        peak_bytes: usize,
    },
}
impl fmt::Display for LayeredRejection3d {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "layered presentation rejected: {self:?}")
    }
}
impl Error for LayeredRejection3d {}
/// Latched expected rejection tagged with the exact source frame and World.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayeredPresentationRejection {
    /// Logical frame whose 3D contribution was not presented.
    pub frame_index: u64,
    /// Source World generation, not the previously displayed World.
    pub generation: WorldGeneration,
    /// Structured rejection without text/log allocation.
    pub reason: LayeredRejection3d,
}
/// Register as an Application Resource before enabling ContinueUi. Read with AppRes.
/// Rejection is latched until a complete layered frame is presented; UI-only
/// presentation does not clear it. No behavior is fabricated by headless execution.
#[derive(Debug, Default)]
pub struct LayeredPresentationFeedback {
    last: Option<LayeredPresentationRejection>,
    rejected_frames: u64,
}
impl LayeredPresentationFeedback {
    /// Last failed layered contribution, or None after confirmed success.
    pub const fn last_rejection(&self) -> Option<LayeredPresentationRejection> {
        self.last
    }
    /// Lifetime count of expected rejected 3D contributions.
    pub const fn rejected_frames(&self) -> u64 {
        self.rejected_frames
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn reject(&mut self, rejection: LayeredPresentationRejection) {
        self.last = Some(rejection);
        self.rejected_frames = self.rejected_frames.saturating_add(1);
    }
    #[cfg(feature = "desktop")]
    pub(crate) fn presented(&mut self) {
        self.last = None;
    }
}
impl<'a> ThreeDSnapshot<'a> {
    /// Iterates visible cuboids assigned to one layer in stable extraction order.
    /// This borrowed partition does not allocate; ordinary rendering ignores it.
    pub fn layer_cuboids(
        self,
        layer: RenderLayer3d,
    ) -> impl Iterator<Item = &'a super::ResolvedCuboid3d> {
        self.cuboids()
            .iter()
            .filter(move |record| record.visual().render_layer() == layer)
    }
    /// Iterates visible meshes assigned to one layer without cloning source assets.
    pub fn layer_meshes(
        self,
        layer: RenderLayer3d,
    ) -> impl Iterator<Item = &'a super::ResolvedMesh3d> {
        self.meshes()
            .iter()
            .filter(move |record| record.visual().render_layer() == layer)
    }

    /// Performs CPU-only layered admission, without changing a published snapshot.
    /// Ordinary mode returns zero layer counts and ignores layer metadata.
    pub fn validate_layered(
        self,
        limits: LayeredRenderLimits3d,
    ) -> Result<[usize; 2], LayeredRejection3d> {
        let Some(settings) = self.view().layered() else {
            return Ok([0, 0]);
        };
        for layer in [RenderLayer3d::Base, RenderLayer3d::Detail] {
            let s = settings.layers[layer.index()];
            if s.policy == ThreeDSurfacePolicy::StrictPortable
                && (s.effects.affine_uv() || s.effects.vertex_snap_grid() != [0, 0])
            {
                return Err(LayeredRejection3d::EffectsPolicy(layer));
            }
        }
        let mut counts = [0usize; 2];
        for record in self.cuboids() {
            let v = record.visual();
            let layer = v.render_layer();
            if v.wireframe().is_some() {
                return Err(LayeredRejection3d::UnsupportedSurface {
                    layer,
                    source: record.source(),
                });
            }
            counts[layer.index()] = counts[layer.index()].saturating_add(12);
        }
        for record in self.meshes() {
            let v = record.visual();
            let layer = v.render_layer();
            if v.surface().alpha_mode() == SurfaceAlphaMode3d::Blend {
                return Err(LayeredRejection3d::UnsupportedSurface {
                    layer,
                    source: record.source(),
                });
            }
            counts[layer.index()] =
                counts[layer.index()].saturating_add(v.asset().mesh().triangle_count());
        }
        for layer in [RenderLayer3d::Base, RenderLayer3d::Detail] {
            let requested = counts[layer.index()];
            let limit = limits.max_triangles(layer);
            if requested > limit {
                return Err(LayeredRejection3d::Triangles {
                    layer: Some(layer),
                    requested,
                    limit,
                });
            }
        }
        let requested = counts[0].saturating_mul(2).saturating_add(counts[1]);
        if requested > limits.submitted {
            return Err(LayeredRejection3d::Triangles {
                layer: None,
                requested,
                limit: limits.submitted,
            });
        }
        Ok(counts)
    }
}

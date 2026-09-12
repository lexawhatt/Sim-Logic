//! Scene-owned texture patches and short-lived material bindings; no GPU history.

use super::{
    meshes::{MeshSlot, aggregate_peak_allowance},
    *,
};
use crate::{screen::ImageFilter, three_d::TextureVisual3d};
use sim_engine::{
    ImageBudget, ImageSampling, ImageTexelRect, Texture3d, Texture3dOptions, Texture3dUpdateBudget,
    TextureMaterial3d, TextureMipmaps3d,
};

pub(super) fn bind(
    renderer: &WgpuRenderer,
    scene: &Scene3d,
    slots: &[MeshSlot],
    mesh: &RetainedMesh3d,
    source: LogicEntity,
    visual: &TextureVisual3d,
) -> Result<RetainedMesh3d, DesktopThreeDError> {
    let shared = slots.iter().find(|slot| {
        slot.texture
            .as_ref()
            .is_some_and(|texture| same_texture(texture, visual))
    });
    let texture = if let Some(slot) = shared {
        scene
            .instance(slot.scene.id)
            .map_err(DesktopThreeDError::Scene)?
            .mesh()
            .material()
            .ok_or(DesktopThreeDError::InvalidCache)?
            .texture()
            .clone()
    } else {
        create(renderer, scene, source, visual)?
    };
    let sampling = match visual.filter() {
        ImageFilter::Nearest => ImageSampling::Nearest,
        ImageFilter::Linear => ImageSampling::Linear,
    };
    let material = TextureMaterial3d::with_alpha(&texture, sampling, visual.tint())
        .map_err(|error| DesktopThreeDError::Texture { source, error })?
        .with_uv_transform(visual.uv_transform())
        .with_address_mode(visual.address_mode());
    renderer
        .with_mesh3d_material(mesh, &material)
        .map_err(|error| DesktopThreeDError::Texture { source, error })
}

pub(super) fn update(
    renderer: &WgpuRenderer,
    scene: &mut Scene3d,
    slots: &mut [MeshSlot],
    index: usize,
    visual: &TextureVisual3d,
    updates: &mut DesktopThreeDUpdates,
) -> Result<(), DesktopThreeDError> {
    if slots[index].texture.as_ref() == Some(visual) {
        return Ok(());
    }
    let source = slots[index].scene.applied.source;
    let id = slots[index].scene.id;
    let already_uploaded = slots.iter().any(|slot| {
        slot.texture
            .as_ref()
            .is_some_and(|candidate| same_texture(candidate, visual))
    });
    if !already_uploaded
        && slots[index]
            .texture
            .as_ref()
            .is_some_and(|old| can_patch(old, visual))
    {
        let (region, stride, pixels) = patch_source(visual)?;
        let budget = patch_budget(scene, id)?;
        // Do not clone any Engine mesh/material/texture before this call:
        // ownership is the condition that permits in-place allocation reuse.
        let report = renderer
            .update_scene3d_texture_region(scene, id, region, pixels, stride, budget)
            .map_err(|error| DesktopThreeDError::TextureUpdate { source, error })?;
        updates.texture_updates = updates.texture_updates.saturating_add(1);
        updates.texture_upload_bytes = updates
            .texture_upload_bytes
            .saturating_add(report.uploaded_bytes());
        updates.texture_gpu_allocations = updates
            .texture_gpu_allocations
            .saturating_add(report.gpu_allocation_count());
        // Patch publication succeeded, but independent sampling might still be
        // old. A rejected later rebind must retry only the uncommitted fields.
        let previous = slots[index]
            .texture
            .take()
            .ok_or(DesktopThreeDError::InvalidCache)?;
        slots[index].texture = Some(previous.with_asset(visual.asset().clone()));
        if slots[index].texture.as_ref() == Some(visual) {
            return Ok(());
        }
    }
    let replacement = bind(
        renderer,
        scene,
        slots,
        scene
            .instance(id)
            .map_err(DesktopThreeDError::Scene)?
            .mesh(),
        source,
        visual,
    )?;
    scene
        .set_mesh(id, &replacement)
        .map_err(|error| DesktopThreeDError::Object { source, error })?;
    slots[index].texture = Some(visual.clone());
    Ok(())
}

fn same_texture(left: &TextureVisual3d, right: &TextureVisual3d) -> bool {
    left.asset().shares_storage(right.asset()) && left.mipmaps() == right.mipmaps()
}

fn can_patch(old: &TextureVisual3d, new: &TextureVisual3d) -> bool {
    new.asset().parent_key() == Some(old.asset().key()) && old.mipmaps() == new.mipmaps()
}

fn create(
    renderer: &WgpuRenderer,
    scene: &Scene3d,
    source: LogicEntity,
    visual: &TextureVisual3d,
) -> Result<Texture3d, DesktopThreeDError> {
    let limits = scene.budget();
    let statistics = scene.statistics();
    // Old/new resource overlap is finite even when material rebinding later
    // rejects final scene residency. Final accounting remains Engine-owned.
    let bytes = limits
        .max_texture_cpu_bytes()
        .saturating_mul(2)
        .saturating_sub(statistics.texture_cpu_bytes())
        .min(
            limits
                .max_texture_gpu_bytes()
                .saturating_mul(2)
                .saturating_sub(statistics.texture_gpu_bytes()),
        )
        .min(limits.max_texture_cpu_bytes())
        .min(limits.max_texture_gpu_bytes());
    let budget = ImageBudget::new(visual.asset().width(), visual.asset().height(), bytes).map_err(
        |error| DesktopThreeDError::Texture {
            source,
            error: Texture3dError::Image(error),
        },
    )?;
    let source_pixels = visual.asset().pixels();
    // Reject before a caller-sized copy, not only after Engine sees the Vec.
    let required = visual
        .texel_bytes()
        .map_err(|_| DesktopThreeDError::InvalidCache)?;
    if required > bytes {
        return Err(DesktopThreeDError::LimitExceeded {
            resource: ThreeDLimitResource::TextureGpuBytes,
            requested: required as u64,
            limit: bytes as u64,
        });
    }
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(source_pixels.len()).map_err(|_| {
        DesktopThreeDError::AllocationFailed {
            requested_bytes: source_pixels.len(),
        }
    })?;
    pixels.extend_from_slice(source_pixels);
    renderer
        .create_texture3d_rgba8_with_options(
            visual.asset().width(),
            visual.asset().height(),
            pixels,
            budget,
            Texture3dOptions::new()
                .with_alpha_preservation(true)
                .with_mipmaps(if visual.mipmaps() {
                    TextureMipmaps3d::Generate
                } else {
                    TextureMipmaps3d::None
                }),
        )
        .map_err(|error| DesktopThreeDError::Texture { source, error })
}

fn patch_source(
    visual: &TextureVisual3d,
) -> Result<(ImageTexelRect, usize, &[u8]), DesktopThreeDError> {
    let region = visual
        .asset()
        .updated_region()
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let stride = (visual.asset().width() as usize)
        .checked_mul(4)
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let start = (region.y() as usize)
        .checked_mul(stride)
        .and_then(|offset| offset.checked_add(region.x() as usize * 4))
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let length = (region.height() as usize - 1)
        .checked_mul(stride)
        .and_then(|length| length.checked_add(region.width() as usize * 4))
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let end = start
        .checked_add(length)
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let pixels = visual
        .asset()
        .pixels()
        .get(start..end)
        .ok_or(DesktopThreeDError::InvalidCache)?;
    let rect = ImageTexelRect::new(region.x(), region.y(), region.width(), region.height())
        .map_err(|_| DesktopThreeDError::InvalidCache)?;
    Ok((rect, stride, pixels))
}

fn patch_budget(
    scene: &Scene3d,
    id: Object3dId,
) -> Result<Texture3dUpdateBudget, DesktopThreeDError> {
    let old = scene
        .instance(id)
        .map_err(DesktopThreeDError::Scene)?
        .mesh()
        .material()
        .ok_or(DesktopThreeDError::InvalidCache)?
        .texture();
    let statistics = scene.statistics();
    let limits = scene.budget();
    let defaults = Texture3dUpdateBudget::default();
    Ok(Texture3dUpdateBudget::new(
        defaults
            .max_upload_bytes()
            .min(limits.max_texture_gpu_bytes()),
        defaults.max_staging_bytes(),
        aggregate_peak_allowance(
            limits.max_texture_cpu_bytes(),
            statistics.texture_cpu_bytes(),
            old.recovery_memory_bytes(),
        ),
        aggregate_peak_allowance(
            limits.max_texture_gpu_bytes(),
            statistics.texture_gpu_bytes(),
            old.gpu_allocation_bytes(),
        ),
        defaults
            .max_gpu_copy_bytes()
            .min(limits.max_texture_gpu_bytes()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{screen::ImageRegion, three_d::TextureAsset3d};

    fn texture() -> TextureVisual3d {
        TextureVisual3d::new(TextureAsset3d::rgba8(4, 3, vec![0; 48], 48).unwrap())
    }

    #[test]
    fn patches_require_immediate_parent_and_identical_mip_policy() {
        let original = texture();
        let region = ImageRegion::new(1, 1, 2, 2).unwrap();
        let first = original.clone().with_asset(
            original
                .asset()
                .with_region_update(region, 8, &[7; 16], 48)
                .unwrap(),
        );
        let second = first.clone().with_asset(
            first
                .asset()
                .with_region_update(region, 8, &[9; 16], 48)
                .unwrap(),
        );
        assert!(can_patch(&original, &first));
        assert!(can_patch(&first, &second));
        assert!(!can_patch(&original, &second));
        assert!(!can_patch(&original, &first.clone().with_mipmaps(true)));
        assert!(!can_patch(&first, &original));
        assert!(same_texture(
            &original,
            &original.clone().with_filter(ImageFilter::Linear)
        ));
        assert!(!same_texture(
            &original,
            &original.clone().with_mipmaps(true)
        ));
    }

    #[test]
    fn patch_view_includes_interrow_padding_but_not_final_row_padding() {
        let original = texture();
        let region = ImageRegion::new(1, 1, 2, 2).unwrap();
        let updated = original.clone().with_asset(
            original
                .asset()
                .with_region_update(region, 8, &[7; 16], 48)
                .unwrap(),
        );
        let (rect, stride, pixels) = patch_source(&updated).unwrap();
        assert_eq!(
            (rect.x(), rect.y(), rect.width(), rect.height()),
            (1, 1, 2, 2)
        );
        assert_eq!(stride, 16);
        assert_eq!(pixels.len(), 24);
        assert_eq!(&pixels[..8], &[7; 8]);
        assert_eq!(&pixels[8..16], &[0; 8]);
        assert_eq!(&pixels[16..], &[7; 8]);
    }
}

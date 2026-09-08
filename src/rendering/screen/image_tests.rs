use super::*;
use crate::{
    assets::{ImageAssetLimits, ImageAssetRegistry},
    identity::ApplicationId,
};

fn image(width: u32, height: u32) -> crate::LogicResult<ImageAssetId> {
    let pixels = vec![255; width as usize * height as usize * 4];
    let mut assets = ImageAssetRegistry::new(ApplicationId::issue()?, ImageAssetLimits::default());
    Ok(assets.register(width, height, &pixels)?)
}

fn visual() -> crate::LogicResult<ScreenImageVisual> {
    Ok(ScreenImageVisual::new(
        image(8, 6)?,
        LogicalScreenPosition::new(-0.0, -0.0),
        LogicalScreenVector::new(16.0, 12.0),
    )?)
}

fn bits(visual: ScreenImageVisual) -> [u32; 9] {
    let position = visual.position().to_vec2();
    let size = visual.size().to_vec2();
    [
        position.x(),
        position.y(),
        size.x(),
        size.y(),
        visual.tint().red(),
        visual.tint().green(),
        visual.tint().blue(),
        visual.tint().alpha(),
        visual.draw_order_depth(),
    ]
    .map(f32::to_bits)
}

#[test]
fn constructor_defaults_offscreen_geometry_and_no_transform_requirement() -> crate::LogicResult {
    let image = image(8, 6)?;
    let position = LogicalScreenPosition::new(-500.0, 9000.0);
    let size = LogicalScreenVector::new(40.0, 25.0);
    let visual = ScreenImageVisual::new(image, position, size)?;
    assert_eq!(visual.image(), image);
    assert_eq!(visual.source_region(), None);
    assert_eq!(visual.filter(), ImageFilter::Nearest);
    assert_eq!(ImageFilter::default(), ImageFilter::Nearest);
    assert_eq!(visual.tint(), Color::WHITE);
    assert_eq!(visual.layer(), Layer::DEFAULT);
    assert_eq!(visual.draw_order_depth(), 0.0);
    assert_eq!(visual.position(), position);
    assert_eq!(visual.size(), size);
    visual.validate()?;
    let mut world = bevy_ecs::world::World::new();
    let entity = world.spawn(visual);
    assert!(!entity.contains::<crate::visual::Transform2d>());
    assert!(!entity.contains::<ScreenRectangleVisual>());
    Ok(())
}

#[test]
fn source_regions_reject_empty_overflow_and_out_of_image_bounds() -> crate::LogicResult {
    for (x, y, width, height) in [
        (0, 0, 0, 1),
        (0, 0, 1, 0),
        (u32::MAX, 0, 1, 1),
        (0, u32::MAX, 1, 1),
    ] {
        assert!(matches!(
            ImageRegion::new(x, y, width, height),
            Err(ImageVisualError::InvalidRegion { .. })
        ));
    }
    let largest = ImageRegion::new(0, 0, u32::MAX, u32::MAX)?;
    assert_eq!(largest.width(), u32::MAX);
    let mut visual = visual()?;
    let exact = ImageRegion::new(2, 1, 6, 5)?;
    assert_eq!(
        (exact.x(), exact.y(), exact.width(), exact.height()),
        (2, 1, 6, 5)
    );
    visual.set_source_region(Some(exact))?;
    for region in [
        ImageRegion::new(2, 1, 7, 5)?,
        ImageRegion::new(2, 1, 6, 6)?,
        largest,
    ] {
        let before = visual;
        assert!(matches!(
            visual.set_source_region(Some(region)),
            Err(ImageVisualError::RegionOutOfBounds { .. })
        ));
        assert_eq!(visual, before);
    }
    visual.set_source_region(None)?;
    assert_eq!(visual.source_region(), None);
    Ok(())
}

#[test]
fn replacing_an_asset_preserves_a_valid_region_and_rejects_smaller_images_atomically()
-> crate::LogicResult {
    let mut visual = visual()?;
    visual.set_source_region(Some(ImageRegion::new(3, 2, 4, 3)?))?;
    visual.set_tint(Color::rgba(0.2, 0.4, 0.6, 0.8))?;
    visual.set_layer(Layer::new(-2));
    visual.set_draw_order_depth(-0.0)?;
    visual.set_filter(ImageFilter::Linear);
    let before = visual;
    let small = image(4, 4)?;
    assert!(matches!(
        visual.set_image(small),
        Err(ImageVisualError::RegionOutOfBounds { image, .. }) if image == small
    ));
    assert_eq!(visual, before);
    assert_eq!(bits(visual), bits(before));
    let large = image(16, 16)?;
    visual.set_image(large)?;
    assert_eq!(visual.image(), large);
    assert_eq!(visual.source_region(), before.source_region());
    assert_eq!(bits(visual), bits(before));
    visual.set_source_region(None)?;
    visual.set_image(small)?;
    assert_eq!(visual.image(), small);
    assert_eq!(visual.filter(), ImageFilter::Linear);
    Ok(())
}

#[test]
fn geometry_tint_and_order_delegate_with_atomic_bit_preservation() -> crate::LogicResult {
    let mut visual = visual()?;
    visual.set_tint(Color::rgba(-0.0, 0.25, 0.5, 1.0))?;
    visual.set_draw_order_depth(-0.0)?;
    visual.set_layer(Layer::new(7));
    visual.set_filter(ImageFilter::Linear);
    visual.set_source_region(Some(ImageRegion::new(0, 0, 8, 6)?))?;
    let before = visual;
    let before_bits = bits(before);
    assert!(
        visual
            .set_geometry(
                LogicalScreenPosition::new(1.0, 2.0),
                LogicalScreenVector::new(0.0, 4.0)
            )
            .is_err()
    );
    assert_eq!(bits(visual), before_bits);
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
        let error = visual
            .set_position(LogicalScreenPosition::new(value, 0.0))
            .expect_err("non-finite or unrepresentable geometry");
        assert!(error.source().is_some());
        assert_eq!(bits(visual), before_bits);
    }
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert!(
            visual
                .set_size(LogicalScreenVector::new(1.0, value))
                .is_err()
        );
        assert_eq!(bits(visual), before_bits);
    }
    for tint in [
        Color::rgb(2.0, 0.0, 0.0),
        Color::rgba(0.0, 0.0, 0.0, f32::NAN),
    ] {
        assert!(matches!(
            visual.set_tint(tint),
            Err(ImageVisualError::Screen(
                ScreenVisualError::InvalidColor { .. }
            ))
        ));
        assert_eq!(bits(visual), before_bits);
    }
    assert!(visual.set_draw_order_depth(f32::INFINITY).is_err());
    assert_eq!(visual, before);
    assert_eq!(bits(visual), before_bits);
    visual.set_geometry(
        LogicalScreenPosition::new(-40.0, 50.0),
        LogicalScreenVector::new(2.0, 3.0),
    )?;
    visual.set_position(LogicalScreenPosition::new(-50.0, 60.0))?;
    visual.set_size(LogicalScreenVector::new(4.0, 5.0))?;
    visual.set_tint(Color::TRANSPARENT)?;
    visual.set_draw_order_depth(-5.0)?;
    visual.validate()?;
    assert_eq!(visual.position(), LogicalScreenPosition::new(-50.0, 60.0));
    assert_eq!(visual.size(), LogicalScreenVector::new(4.0, 5.0));
    assert_eq!(visual.tint(), Color::TRANSPARENT);
    assert_eq!(visual.layer(), Layer::new(7));
    assert_eq!(visual.draw_order_depth(), -5.0);
    Ok(())
}

#[test]
fn corrupted_private_region_fails_validation_without_overflowing() -> crate::LogicResult {
    let mut visual = visual()?;
    visual.source_region = Some(ImageRegion {
        x: u32::MAX,
        y: 0,
        width: 1,
        height: 1,
    });
    assert!(matches!(
        visual.validate(),
        Err(ImageVisualError::InvalidRegion { .. })
    ));
    visual.source_region = Some(ImageRegion {
        x: 7,
        y: 0,
        width: 2,
        height: 1,
    });
    assert!(matches!(
        visual.validate(),
        Err(ImageVisualError::RegionOutOfBounds { .. })
    ));
    Ok(())
}

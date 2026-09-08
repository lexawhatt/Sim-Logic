use std::{collections::HashSet, error::Error};

use super::*;

fn registry(limits: ImageAssetLimits) -> ImageAssetRegistry {
    ImageAssetRegistry::new(ApplicationId::from_raw(1), limits)
}

#[test]
fn defaults_and_zero_limits_are_exact() {
    let defaults = ImageAssetLimits::default();
    assert_eq!(defaults.max_images(), 64);
    assert_eq!(defaults.max_width(), 4096);
    assert_eq!(defaults.max_height(), 4096);
    assert_eq!(defaults.max_pixel_bytes(), 64 * 1024 * 1024);
    let zero = ImageAssetLimits::new(0, 0, 0, 0);
    assert_eq!((zero.max_images(), zero.max_pixel_bytes()), (0, 0));
    assert_eq!((zero.max_width(), zero.max_height()), (0, 0));
    assert!(matches!(
        registry(ImageAssetLimits::new(0, 1, 1, 4)).register(1, 1, &[0; 4]),
        Err(ImageAssetError::ImageLimitExceeded { limit: 0 })
    ));
    assert!(matches!(
        registry(zero).register(1, 1, &[0; 4]),
        Err(ImageAssetError::DimensionsExceeded { .. })
    ));
    assert!(matches!(
        registry(ImageAssetLimits::new(1, 1, 1, 0)).register(1, 1, &[0; 4]),
        Err(ImageAssetError::PixelByteLimitExceeded { limit: 0, .. })
    ));
}

#[test]
fn registration_copies_exact_rows_without_retaining_caller_capacity() -> crate::LogicResult {
    let mut source = Vec::with_capacity(1024);
    source.extend_from_slice(&[255, 0, 0, 0, 0, 255, 0, 128]);
    let mut images = registry(ImageAssetLimits::new(2, 2, 1, 8));
    let id = images.register(2, 1, &source)?;
    source.fill(33);
    let image = images.get(id).ok_or("registered image missing")?;
    assert_eq!((id.width(), id.height()), (2, 1));
    assert_eq!((image.width(), image.height()), (2, 1));
    assert_eq!(image.pixels(), &[255, 0, 0, 0, 0, 255, 0, 128]);
    assert_eq!(images.len(), 1);
    assert_eq!(images.pixel_bytes(), 8);
    assert_eq!(images.pixel_bytes(), image.pixels.capacity());
    Ok(())
}

#[test]
fn identical_registration_is_distinct_and_foreign_or_corrupt_handles_fail_closed()
-> crate::LogicResult {
    let mut images = registry(ImageAssetLimits::new(2, 1, 1, 8));
    let first = images.register(1, 1, &[0; 4])?;
    let second = images.register(1, 1, &[0; 4])?;
    assert_ne!(first, second);
    assert_eq!(HashSet::from([first, second, first]).len(), 2);
    let mut foreign = ImageAssetRegistry::new(
        ApplicationId::from_raw(2),
        ImageAssetLimits::new(1, 1, 1, 4),
    );
    let foreign_id = foreign.register(1, 1, &[0; 4])?;
    assert_ne!(first, foreign_id);
    assert!(images.get(foreign_id).is_none());
    assert!(foreign.get(first).is_none());
    for forged in [
        ImageAssetId { slot: 2, ..first },
        ImageAssetId { width: 2, ..first },
        ImageAssetId { height: 2, ..first },
    ] {
        assert!(images.get(forged).is_none());
    }
    Ok(())
}

#[test]
fn dimensions_and_exact_lengths_are_checked_without_allocating() {
    let mut images = registry(ImageAssetLimits::new(1, 2, 3, 24));
    for (width, height) in [(0, 1), (1, 0), (0, 0)] {
        assert!(matches!(
            images.register(width, height, &[]),
            Err(ImageAssetError::InvalidDimensions { .. })
        ));
    }
    for (width, height) in [(3, 1), (1, 4)] {
        assert!(matches!(
            images.register(width, height, &[]),
            Err(ImageAssetError::DimensionsExceeded { .. })
        ));
    }
    for actual in [0, 3, 5, 25] {
        assert!(matches!(
            images.register(1, 1, &vec![0; actual]),
            Err(ImageAssetError::PixelLengthMismatch {
                expected: 4,
                actual: rejected,
            }) if rejected == actual
        ));
    }
    assert_eq!(images.len(), 0);
    assert_eq!(images.pixel_bytes(), 0);
    assert_eq!(images.images.capacity(), 0);
    let mut large = registry(ImageAssetLimits::new(1, u32::MAX, u32::MAX, usize::MAX));
    assert!(matches!(
        large.register(u32::MAX, u32::MAX, &[]),
        Err(ImageAssetError::PixelByteCountOverflow { .. })
    ));
    assert_eq!(large.images.capacity(), 0);
}

#[test]
fn rejected_count_and_pixel_limits_preserve_entries_accounting_and_next_slot() -> crate::LogicResult
{
    let mut images = registry(ImageAssetLimits::new(2, 2, 1, 8));
    let first = images.register(1, 1, &[1; 4])?;
    assert!(matches!(
        images.register(2, 1, &[2; 8]),
        Err(ImageAssetError::PixelByteLimitExceeded {
            limit: 8,
            retained: 4,
            incoming: 8,
        })
    ));
    assert_eq!(images.len(), 1);
    assert_eq!(images.pixel_bytes(), 4);
    assert_eq!(
        images.get(first).ok_or("first image lost")?.pixels(),
        &[1; 4]
    );
    let second = images.register(1, 1, &[3; 4])?;
    assert_eq!(second.slot, 1);
    assert_eq!(images.pixel_bytes(), 8);
    assert!(matches!(
        images.register(1, 1, &[4; 4]),
        Err(ImageAssetError::ImageLimitExceeded { limit: 2 })
    ));
    assert_eq!(images.len(), 2);
    assert_eq!(images.pixel_bytes(), 8);
    assert_eq!(
        images.get(second).ok_or("second image lost")?.pixels(),
        &[3; 4]
    );
    Ok(())
}

#[test]
fn retained_capacity_check_handles_overreservation_and_integer_extremes() -> crate::LogicResult {
    let mut images = registry(ImageAssetLimits::new(2, 1, 1, 8));
    images.register(1, 1, &[0; 4])?;
    assert!(images.check_pixel_capacity(4).is_ok());
    for incoming in [5, usize::MAX] {
        assert!(matches!(
            images.check_pixel_capacity(incoming),
            Err(ImageAssetError::PixelByteLimitExceeded { .. })
        ));
    }
    assert_eq!((images.len(), images.pixel_bytes()), (1, 4));
    Ok(())
}

#[test]
fn allocation_failure_retains_its_original_error_source() {
    let source = Vec::<u8>::new()
        .try_reserve(usize::MAX)
        .expect_err("capacity must overflow");
    let error = ImageAssetError::AllocationFailed { source };
    assert!(error.source().is_some());
    assert!(
        error
            .to_string()
            .contains("image asset storage allocation failed")
    );
}

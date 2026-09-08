use super::*;

#[test]
fn embedded_pixels_match_original_artwork() {
    let pixels = decode(PNG).unwrap();
    assert_eq!(pixels.len(), PIXEL_BYTES);
    // FNV-1a over independently decoded, straight-alpha RGBA8 source pixels.
    let hash = pixels.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    assert_eq!(hash, 0xf65351ec87b578df);
    assert_eq!(pixels[3], 0, "the corner must remain transparent");
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 255));
}

#[test]
fn malformed_or_truncated_png_returns_a_decode_error() {
    for bytes in [b"not a png".as_slice(), &PNG[..PNG.len() / 2]] {
        assert!(matches!(decode(bytes), Err(CrabDrawError::Decode(_))));
    }
}

#[test]
fn unexpected_artwork_layout_is_rejected_before_pixel_allocation() {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&[0; 4])
            .unwrap();
    }
    assert!(matches!(decode(&bytes), Err(CrabDrawError::InvalidArtwork)));
}

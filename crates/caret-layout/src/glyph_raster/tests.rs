//! 验证字形栅格化、颜色合成和设备像素相位。

#[cfg(target_os = "macos")]
use super::RasterizedGlyph;
use super::{GlyphRasterOptions, GlyphRasterizer, RasterizedGlyphKey};
use super::{colorize_alpha, premultiply_rgba, synthesis_skew};
use linebender_resource_handle::Blob;
use parley::FontData;

#[test]
fn invalid_raster_requests_are_rejected_without_allocating_bitmap_storage() {
    let font = FontData::new(Blob::from(vec![0_u8]), 0);
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY, 4097.0] {
        assert!(
            RasterizedGlyphKey::new(
                &font,
                1,
                GlyphRasterOptions {
                    pixels_per_em: size,
                    ..glyph_options()
                }
            )
            .is_none()
        );
    }
    for phase in [[-0.1, 0.0], [0.0, 1.0], [f32::NAN, 0.0]] {
        assert!(
            RasterizedGlyphKey::new(
                &font,
                1,
                GlyphRasterOptions {
                    pixel_phase: phase,
                    ..glyph_options()
                }
            )
            .is_none()
        );
    }
    assert!(RasterizedGlyphKey::new(&font, 65_536, glyph_options()).is_none());
    let mut rasterizer = GlyphRasterizer::default();
    assert!(rasterizer.rasterize(&font, 1, glyph_options()).is_none());
    assert_eq!(rasterizer.cache_bytes, 0);
    assert!(rasterizer.cache.is_empty());
}

#[test]
fn synthetic_italic_shears_x_without_changing_baseline() {
    let transform = synthesis_skew(12.0);
    assert!(transform.yx > 0.0);
    assert_eq!(transform.xy, 0.0);
    assert_eq!(transform.yy, 1.0);
}

#[test]
fn color_bitmap_pixels_are_normalized_to_premultiplied_alpha() {
    let mut pixels = [200, 100, 50, 128, 255, 32, 16, 0];
    premultiply_rgba(&mut pixels);
    assert_eq!(pixels, [100, 50, 25, 128, 0, 0, 0, 0]);
}

#[test]
fn alpha_mask_is_tinted_into_premultiplied_rgba() {
    let pixels = colorize_alpha(&[255, 128, 0], [200, 100, 50, 128])
        .unwrap_or_else(|| panic!("alpha mask should colorize"));
    assert_eq!(pixels, [100, 50, 25, 128, 50, 25, 13, 64, 0, 0, 0, 0]);
}

#[cfg(target_os = "macos")]
#[test]
fn ttc_faces_do_not_share_scaler_cache_entries() {
    let bytes = std::fs::read("/System/Library/Fonts/AppleSDGothicNeo.ttc")
        .unwrap_or_else(|error| panic!("macOS system TTC font is unavailable: {error}"));
    let data = Blob::from(bytes);
    let first_face = FontData::new(data.clone(), 0);
    let second_face = FontData::new(data, 1);
    let first_index = usize::try_from(first_face.index)
        .unwrap_or_else(|error| panic!("first TTC index is invalid: {error}"));
    let second_index = usize::try_from(second_face.index)
        .unwrap_or_else(|error| panic!("second TTC index is invalid: {error}"));
    let first_font = swash::FontRef::from_index(first_face.data.as_ref(), first_index)
        .unwrap_or_else(|| panic!("first TTC face is invalid"));
    let second_font = swash::FontRef::from_index(second_face.data.as_ref(), second_index)
        .unwrap_or_else(|| panic!("second TTC face is invalid"));
    let first_glyph = u32::from(first_font.charmap().map('A'));
    let second_glyph = u32::from(second_font.charmap().map('A'));
    let mut reused = GlyphRasterizer::default();
    let _ = reused
        .rasterize(&first_face, first_glyph, glyph_options())
        .unwrap_or_else(|| panic!("first TTC face should rasterize"));
    let reused_second = reused
        .rasterize(&second_face, second_glyph, glyph_options())
        .unwrap_or_else(|| panic!("second TTC face should rasterize"));
    let fresh_second = GlyphRasterizer::default()
        .rasterize(&second_face, second_glyph, glyph_options())
        .unwrap_or_else(|| panic!("second TTC face should rasterize independently"));
    assert_eq!(reused_second, fresh_second);
}

#[cfg(target_os = "macos")]
#[test]
fn color_bitmap_glyph_preserves_fractional_placement_phase() {
    let bytes = std::fs::read("/System/Library/Fonts/Apple Color Emoji.ttc")
        .unwrap_or_else(|error| panic!("macOS color emoji font is unavailable: {error}"));
    let font = FontData::new(Blob::from(bytes), 0);
    let face = swash::FontRef::from_index(font.data.as_ref(), 0)
        .unwrap_or_else(|| panic!("color emoji face is invalid"));
    let glyph_id = u32::from(face.charmap().map('😀'));
    let mut rasterizer = GlyphRasterizer::default();
    let integral = rasterizer
        .rasterize(&font, glyph_id, glyph_options())
        .unwrap_or_else(|| panic!("color emoji should rasterize"));
    let fractional = rasterizer
        .rasterize(
            &font,
            glyph_id,
            GlyphRasterOptions {
                pixel_phase: [0.5, 0.5],
                ..glyph_options()
            },
        )
        .unwrap_or_else(|| panic!("fractional color emoji should rasterize"));
    assert!(!integral.pixel_phase_applied);
    assert!(!fractional.pixel_phase_applied);
    assert_eq!(integral.pixels, fractional.pixels);
}

fn glyph_options() -> GlyphRasterOptions<'static> {
    GlyphRasterOptions {
        pixels_per_em: 16.0,
        pixel_phase: [0.0, 0.0],
        normalized_coords: &[],
        synthesis: Default::default(),
        foreground: [0, 0, 0, 255],
    }
}

#[cfg(target_os = "macos")]
#[test]
fn outline_pixel_phase_moves_down_in_device_coordinates() {
    let bytes = std::fs::read("/System/Library/Fonts/Apple Symbols.ttf")
        .unwrap_or_else(|error| panic!("macOS outline font is unavailable: {error}"));
    let font = FontData::new(Blob::from(bytes), 0);
    let face = swash::FontRef::from_index(font.data.as_ref(), 0)
        .unwrap_or_else(|| panic!("outline font face is invalid"));
    let glyph_id = u32::from(face.charmap().map('H'));
    let mut rasterizer = GlyphRasterizer::default();
    let integral = rasterizer
        .rasterize(&font, glyph_id, glyph_options())
        .unwrap_or_else(|| panic!("outline glyph should rasterize"));
    let fractional = rasterizer
        .rasterize(
            &font,
            glyph_id,
            GlyphRasterOptions {
                pixel_phase: [0.0, 0.25],
                ..glyph_options()
            },
        )
        .unwrap_or_else(|| panic!("fractional outline glyph should rasterize"));
    assert!(integral.pixel_phase_applied);
    assert!(fractional.pixel_phase_applied);
    let movement = device_y_centroid(&fractional) - device_y_centroid(&integral);
    assert!((movement - 0.25).abs() < 0.05, "movement was {movement}");
}

#[cfg(target_os = "macos")]
fn device_y_centroid(glyph: &RasterizedGlyph) -> f64 {
    let width = usize::try_from(glyph.width)
        .unwrap_or_else(|error| panic!("glyph width is invalid: {error}"));
    let mut alpha_sum = 0.0;
    let mut y_sum = 0.0;
    for (index, pixel) in glyph.pixels.as_chunks::<4>().0.iter().enumerate() {
        let y = index / width;
        let y = u32::try_from(y).unwrap_or_else(|error| panic!("glyph row is invalid: {error}"));
        let alpha = f64::from(pixel[3]);
        alpha_sum += alpha;
        y_sum += f64::from(y) * alpha;
    }
    assert!(alpha_sum > 0.0);
    -f64::from(glyph.top) + y_sum / alpha_sum
}

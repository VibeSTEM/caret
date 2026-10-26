//! 使用 Parley 已解析的字体面和 glyph ID 栅格化单个字形。

use parley::FontData;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use swash::{
    FontRef,
    scale::{Render, ScaleContext, Source, StrikeWith},
    zeno::{Angle, Transform, Vector},
};

/// 已栅格化字形；位置是相对基线的像素偏移，像素大小由请求方决定。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RasterizedGlyph {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
    /// false 表示字体位图源未烘焙相位，绘制时须保留请求的小数放置偏移。
    pub pixel_phase_applied: bool,
    pub pixels: Vec<u8>,
}

/// 一次字形栅格化的像素尺寸、亚像素相位、字体实例和前景色。
#[derive(Debug, Clone, Copy)]
pub struct GlyphRasterOptions<'a> {
    /// 每 em 的目标像素数，有效范围为 `(0, 4096]`。
    pub pixels_per_em: f32,
    /// Qt 设备像素坐标的小数相位，每轴必须位于 `[0, 1)`；y 轴向下。
    pub pixel_phase: [f32; 2],
    /// Parley 字体实例的 F2Dot14 规范化坐标原始位值。
    pub normalized_coords: &'a [i16],
    /// Parley 为当前字体面选择的合成提示。
    pub synthesis: fontique::Synthesis,
    /// 用于 COLR 前景层和普通轮廓字形的 RGBA 前景色。
    pub foreground: [u8; 4],
}

/// 复用 Swash 的字体缩放和临时缓冲区处理 Parley 输出的字形。
#[derive(Default)]
pub struct GlyphRasterizer {
    context: ScaleContext,
    cache: HashMap<GlyphRasterKey, CachedGlyph>,
    insertion_order: VecDeque<GlyphRasterKey>,
    cache_bytes: usize,
}

const MAX_GLYPH_CACHE_BYTES: usize = 16 * 1024 * 1024;
const MAX_GLYPH_CACHE_ENTRIES: usize = 8192;
const MAX_PIXELS_PER_EM: f32 = 4096.0;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct GlyphRasterKey {
    font_resource_id: u64,
    font_index: u32,
    glyph_id: u16,
    pixels_per_em: u32,
    pixel_phase: [u32; 2],
    normalized_coords: Vec<i16>,
    foreground: [u8; 4],
    embolden: bool,
    skew_degrees: Option<u32>,
}

/// 字形栅格身份；调用方可在一页范围内按此键去重，不依赖缓存驻留状态。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RasterizedGlyphKey(GlyphRasterKey);

impl RasterizedGlyphKey {
    /// 为准确字体实例和渲染参数构造稳定栅格键；参数非法时返回 `None`。
    pub fn new(font: &FontData, glyph_id: u32, options: GlyphRasterOptions<'_>) -> Option<Self> {
        if !options.pixels_per_em.is_finite()
            || options.pixels_per_em <= 0.0
            || options.pixels_per_em > MAX_PIXELS_PER_EM
            || options
                .pixel_phase
                .iter()
                .any(|phase| !phase.is_finite() || !(0.0..1.0).contains(phase))
        {
            return None;
        }
        Some(Self(GlyphRasterKey {
            font_resource_id: font.data.id(),
            font_index: font.index,
            glyph_id: u16::try_from(glyph_id).ok()?,
            pixels_per_em: options.pixels_per_em.to_bits(),
            pixel_phase: options.pixel_phase.map(f32::to_bits),
            normalized_coords: options.normalized_coords.to_vec(),
            foreground: options.foreground,
            embolden: options.synthesis.embolden(),
            skew_degrees: options.synthesis.skew().map(f32::to_bits),
        }))
    }
}

struct CachedGlyph {
    image: Arc<RasterizedGlyph>,
}

impl GlyphRasterizer {
    /// 按准确字体集合索引、可变轴坐标和 Parley 合成提示栅格化 glyph ID。
    ///
    /// 无效字体索引、glyph ID、尺寸/相位或不受支持的字形来源返回 `None`；相同键的
    /// 图片以共享 `Arc` 返回，缓存总像素数据不超过 16 MiB。
    pub fn rasterize(
        &mut self,
        font: &FontData,
        glyph_id: u32,
        options: GlyphRasterOptions<'_>,
    ) -> Option<Arc<RasterizedGlyph>> {
        self.rasterize_with_key(font, glyph_id, options)
            .map(|(_, image)| image)
    }

    /// 返回与实际栅格缓存相同的稳定键和图片，供页面快照跨缓存淘汰去重。
    pub fn rasterize_with_key(
        &mut self,
        font: &FontData,
        glyph_id: u32,
        options: GlyphRasterOptions<'_>,
    ) -> Option<(RasterizedGlyphKey, Arc<RasterizedGlyph>)> {
        let key = RasterizedGlyphKey::new(font, glyph_id, options)?;
        let GlyphRasterOptions {
            pixels_per_em,
            pixel_phase,
            normalized_coords,
            synthesis,
            foreground,
        } = options;
        let glyph_id = u16::try_from(glyph_id).ok()?;
        let bytes = font.data.as_ref();
        if let Some(cached) = self.cache.get(&key.0) {
            return Some((key, Arc::clone(&cached.image)));
        }
        let index = usize::try_from(font.index).ok()?;
        let font_ref = FontRef::from_index(bytes, index)?;
        let identity = font.data.id();
        let mut scaler = self
            .context
            .builder_with_id(font_ref, [identity, u64::from(font.index)])
            .size(pixels_per_em)
            .normalized_coords(normalized_coords.iter().copied())
            .build();

        let mut render = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
            Source::Bitmap(StrikeWith::ExactSize),
        ]);
        render.default_color(foreground);
        if synthesis.embolden() {
            render.embolden(pixels_per_em * 0.03);
        }
        render.offset(Vector::new(pixel_phase[0], -pixel_phase[1]));
        if let Some(skew_degrees) = synthesis.skew() {
            render.transform(Some(synthesis_skew(skew_degrees)));
        }
        let image = render.render(&mut scaler, glyph_id)?;
        let source = image.source;
        let pixel_phase_applied = !matches!(source, Source::ColorBitmap(_) | Source::Bitmap(_));
        let channels = match image.content {
            swash::scale::image::Content::Mask => 1usize,
            swash::scale::image::Content::Color => 4usize,
            swash::scale::image::Content::SubpixelMask => return None,
        };
        let expected_size = usize::try_from(image.placement.width)
            .ok()?
            .checked_mul(usize::try_from(image.placement.height).ok()?)?
            .checked_mul(channels)?;
        if expected_size != image.data.len() {
            return None;
        }
        let pixels = match image.content {
            swash::scale::image::Content::Mask => colorize_alpha(&image.data, foreground)?,
            swash::scale::image::Content::Color => {
                let mut pixels = image.data;
                if matches!(source, Source::ColorBitmap(_)) {
                    premultiply_rgba(&mut pixels);
                }
                pixels
            }
            swash::scale::image::Content::SubpixelMask => return None,
        };
        let image = Arc::new(RasterizedGlyph {
            width: image.placement.width,
            height: image.placement.height,
            left: image.placement.left,
            top: image.placement.top,
            pixel_phase_applied,
            pixels,
        });
        if image.pixels.len() <= MAX_GLYPH_CACHE_BYTES {
            while self.cache_bytes.saturating_add(image.pixels.len()) > MAX_GLYPH_CACHE_BYTES
                || self.cache.len() >= MAX_GLYPH_CACHE_ENTRIES
            {
                let Some(oldest) = self.insertion_order.pop_front() else {
                    break;
                };
                if let Some(removed) = self.cache.remove(&oldest) {
                    self.cache_bytes = self.cache_bytes.saturating_sub(removed.image.pixels.len());
                }
            }
            self.cache_bytes = self.cache_bytes.saturating_add(image.pixels.len());
            self.insertion_order.push_back(key.0.clone());
            self.cache.insert(
                key.0.clone(),
                CachedGlyph {
                    image: Arc::clone(&image),
                },
            );
        }
        Some((key, image))
    }
}

fn premultiply_rgba(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = u8::try_from((u16::from(*channel) * alpha + 127) / 255).unwrap_or(0);
        }
    }
}

fn colorize_alpha(coverage: &[u8], foreground: [u8; 4]) -> Option<Vec<u8>> {
    let mut pixels = Vec::with_capacity(coverage.len().checked_mul(4)?);
    for coverage in coverage {
        let alpha = (u16::from(*coverage) * u16::from(foreground[3]) + 127) / 255;
        for channel in &foreground[..3] {
            pixels.push(u8::try_from((u16::from(*channel) * alpha + 127) / 255).ok()?);
        }
        pixels.push(u8::try_from(alpha).ok()?);
    }
    Some(pixels)
}

fn synthesis_skew(degrees: f32) -> Transform {
    Transform::skew(Angle::from_degrees(degrees), Angle::ZERO)
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::{GlyphRasterOptions, GlyphRasterizer, RasterizedGlyph};
    use super::{colorize_alpha, premultiply_rgba, synthesis_skew};
    #[cfg(target_os = "macos")]
    use linebender_resource_handle::Blob;
    #[cfg(target_os = "macos")]
    use parley::FontData;

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

    #[cfg(target_os = "macos")]
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
            let y =
                u32::try_from(y).unwrap_or_else(|error| panic!("glyph row is invalid: {error}"));
            let alpha = f64::from(pixel[3]);
            alpha_sum += alpha;
            y_sum += f64::from(y) * alpha;
        }
        assert!(alpha_sum > 0.0);
        -f64::from(glyph.top) + y_sum / alpha_sum
    }
}

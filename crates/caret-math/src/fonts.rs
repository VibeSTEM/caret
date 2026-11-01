//! 读取内置 STIX Two Math 字体的 OpenType MATH 常量、字形度量与伸展构造。
//!
//! MATH 表经 read-fonts 0.45 的 `tables::math` 读取；字形边界、字符映射和步进经 skrifa
//! 读取，CFF 轮廓的边界由 skrifa 从轮廓计算。所有返回值使用字体设计单位，调用方
//! 按字号换算。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Mutex, OnceLock};

use read_fonts::TableProvider;
use read_fonts::tables::math::{Math, MathConstant, MathKernCorner, StretchAxis};
use read_fonts::types::GlyphId;
use skrifa::MetadataProvider;
use skrifa::instance::{LocationRef, Size};

/// 数学字体角色使用的内置字体族名。
pub const BUILTIN_FONT_FAMILY: &str = "STIX Two Math";

/// 内置数学字体字节，许可证见仓库公共字体资源目录中的 `OFL.txt`。
pub const STIX_TWO_MATH: &[u8] =
    include_bytes!("../../../resources/fonts/math/stix-two-math/STIXTwoMath-Regular.otf");

/// 伸展方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    X,
    Y,
}

/// 字形四角之一，用于 MATH 角落字距。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    /// 对角，上下标与基底的字距按对角相加。
    pub(crate) fn inv(self) -> Self {
        match self {
            Self::TopLeft => Self::BottomRight,
            Self::TopRight => Self::BottomLeft,
            Self::BottomLeft => Self::TopRight,
            Self::BottomRight => Self::TopLeft,
        }
    }
}

/// 字形墨迹边界，设计单位，y 向上。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct GlyphBounds {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

/// 组装件的一段。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct AssemblyPart {
    pub glyph: u16,
    pub start_connector: f64,
    pub end_connector: f64,
    pub full_advance: f64,
    pub extender: bool,
}

/// 字形组装：按 OpenType 顺序（自左向右或自下而上）排列的片段。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Assembly {
    pub italics_correction: f64,
    pub parts: Vec<AssemblyPart>,
}

/// 某一方向上的伸展构造：预制变体按尺寸递增，以及可选组装。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Construction {
    pub variants: Vec<(u16, f64)>,
    pub assembly: Option<Assembly>,
}

/// 每个字形的缓存度量，避免同一公式反复解析字体表。
#[derive(Debug, Clone, Copy)]
struct GlyphMetrics {
    advance: f64,
    bounds: Option<GlyphBounds>,
}

/// 只读数学字体；可在多线程间共享。
pub struct MathFont {
    data: &'static [u8],
    upem: f64,
    constants: [i32; 56],
    min_connector_overlap: f64,
    identity: u64,
    metrics: Mutex<HashMap<u16, GlyphMetrics>>,
}

impl std::fmt::Debug for MathFont {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("MathFont")
            .field("upem", &self.upem)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// 字体无法作为数学字体使用的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathFontError {
    InvalidFont,
    MissingMathTable,
}

impl std::fmt::Display for MathFontError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidFont => "math.font.invalid",
            Self::MissingMathTable => "math.font.missing_math_table",
        })
    }
}

impl std::error::Error for MathFontError {}

impl MathFont {
    /// 进程内共享的内置 STIX Two Math 实例。
    pub fn builtin() -> Result<&'static MathFont, MathFontError> {
        static FONT: OnceLock<Result<MathFont, MathFontError>> = OnceLock::new();
        FONT.get_or_init(|| MathFont::from_static(STIX_TWO_MATH))
            .as_ref()
            .map_err(|error| *error)
    }

    /// 从静态字节建立数学字体；字体必须带 MATH 表。
    pub fn from_static(data: &'static [u8]) -> Result<Self, MathFontError> {
        let font = read_fonts::FontRef::new(data).map_err(|_| MathFontError::InvalidFont)?;
        let upem = f64::from(
            font.head()
                .map_err(|_| MathFontError::InvalidFont)?
                .units_per_em(),
        );
        let math = font.math().map_err(|_| MathFontError::MissingMathTable)?;
        let table = math
            .math_constants()
            .map_err(|_| MathFontError::MissingMathTable)?;
        let mut constants = [0; 56];
        for constant in MathConstant::ALL {
            if let Some(slot) = constants.get_mut(*constant as usize) {
                *slot = table.constant(*constant);
            }
        }
        let min_connector_overlap = math
            .math_variants()
            .map(|variants| f64::from(variants.min_connector_overlap().to_u16()))
            .unwrap_or(0.0);
        let mut hasher = DefaultHasher::new();
        data.hash(&mut hasher);
        Ok(Self {
            data,
            upem,
            constants,
            min_connector_overlap,
            identity: hasher.finish(),
            metrics: Mutex::new(HashMap::new()),
        })
    }

    /// 字体原始字节，供绘制层按同一 Blob 栅格化。
    pub fn data(&self) -> &'static [u8] {
        self.data
    }

    /// 字体身份，参与布局缓存键。
    pub fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn units_per_em(&self) -> f64 {
        self.upem
    }

    fn math(&self) -> Option<Math<'static>> {
        read_fonts::FontRef::new(self.data).ok()?.math().ok()
    }

    /// MATH 常量，设计单位；百分比常量返回原始百分数。
    pub(crate) fn constant(&self, constant: MathConstant) -> f64 {
        f64::from(
            self.constants
                .get(constant as usize)
                .copied()
                .unwrap_or_default(),
        )
    }

    pub(crate) fn min_connector_overlap(&self) -> f64 {
        self.min_connector_overlap
    }

    /// 字符映射；字体不含该字符时返回 `None`。
    pub(crate) fn glyph(&self, character: char) -> Option<u16> {
        let font = skrifa::FontRef::new(self.data).ok()?;
        let glyph = font.charmap().map(character)?;
        u16::try_from(glyph.to_u32()).ok().filter(|id| *id != 0)
    }

    fn metrics(&self, glyph: u16) -> GlyphMetrics {
        if let Ok(cache) = self.metrics.lock()
            && let Some(metrics) = cache.get(&glyph)
        {
            return *metrics;
        }
        let metrics = skrifa::FontRef::new(self.data)
            .ok()
            .map(|font| {
                let glyph_metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
                let id = skrifa::GlyphId::new(u32::from(glyph));
                GlyphMetrics {
                    advance: f64::from(glyph_metrics.advance_width(id).unwrap_or_default()),
                    bounds: glyph_metrics.bounds(id).map(|bounds| GlyphBounds {
                        x_min: f64::from(bounds.x_min),
                        y_min: f64::from(bounds.y_min),
                        x_max: f64::from(bounds.x_max),
                        y_max: f64::from(bounds.y_max),
                    }),
                }
            })
            .unwrap_or(GlyphMetrics {
                advance: 0.0,
                bounds: None,
            });
        if let Ok(mut cache) = self.metrics.lock() {
            cache.insert(glyph, metrics);
        }
        metrics
    }

    pub(crate) fn advance(&self, glyph: u16) -> f64 {
        self.metrics(glyph).advance
    }

    pub(crate) fn bounds(&self, glyph: u16) -> Option<GlyphBounds> {
        self.metrics(glyph).bounds
    }

    pub(crate) fn italics_correction(&self, glyph: u16) -> Option<f64> {
        let info = self.math()?.math_glyph_info().ok()?;
        let corrections = info.math_italics_correction_info()?.ok()?;
        corrections
            .correction(GlyphId::new(u32::from(glyph)))
            .map(f64::from)
    }

    pub(crate) fn top_accent_attachment(&self, glyph: u16) -> Option<f64> {
        let info = self.math()?.math_glyph_info().ok()?;
        let attachments = info.math_top_accent_attachment()?.ok()?;
        attachments
            .attachment(GlyphId::new(u32::from(glyph)))
            .map(f64::from)
    }

    pub(crate) fn is_extended_shape(&self, glyph: u16) -> bool {
        self.math()
            .and_then(|math| math.math_glyph_info().ok())
            .is_some_and(|info| info.is_extended_shape(GlyphId::new(u32::from(glyph))))
    }

    /// 指定高度处的角落字距，设计单位。
    pub(crate) fn kern_at_height(&self, glyph: u16, corner: Corner, height: f64) -> Option<f64> {
        let info = self.math()?.math_glyph_info().ok()?;
        let kerns = info.math_kern_info()?.ok()?;
        let corner = match corner {
            Corner::TopLeft => MathKernCorner::TopLeft,
            Corner::TopRight => MathKernCorner::TopRight,
            Corner::BottomLeft => MathKernCorner::BottomLeft,
            Corner::BottomRight => MathKernCorner::BottomRight,
        };
        let kern = kerns.kern(GlyphId::new(u32::from(glyph)), corner)?;
        kern.kerning(height.round() as i32).map(f64::from)
    }

    /// 字形在某方向上是否有伸展构造。
    pub(crate) fn is_stretchable(&self, glyph: u16, axis: Axis) -> bool {
        self.construction(glyph, axis).is_some()
    }

    pub(crate) fn construction(&self, glyph: u16, axis: Axis) -> Option<Construction> {
        let variants = self.math()?.math_variants().ok()?;
        let axis = match axis {
            Axis::X => StretchAxis::Horizontal,
            Axis::Y => StretchAxis::Vertical,
        };
        let construction = variants.glyph_construction(GlyphId::new(u32::from(glyph)), axis)?;
        let variant_list = construction
            .math_glyph_variant_records()
            .iter()
            .map(|record| {
                (
                    record.variant_glyph().to_u16(),
                    f64::from(record.advance_measurement().to_u16()),
                )
            })
            .collect();
        let assembly = construction
            .glyph_assembly()
            .and_then(Result::ok)
            .map(|assembly| Assembly {
                italics_correction: f64::from(assembly.italics_correction().value().to_i16()),
                parts: assembly
                    .part_records()
                    .iter()
                    .map(|part| AssemblyPart {
                        glyph: part.glyph_id().to_u16(),
                        start_connector: f64::from(part.start_connector_length().to_u16()),
                        end_connector: f64::from(part.end_connector_length().to_u16()),
                        full_advance: f64::from(part.full_advance().to_u16()),
                        extender: part
                            .part_flags()
                            .contains(read_fonts::tables::math::PartFlags::EXTENDER_FLAG),
                    })
                    .collect(),
            });
        Some(Construction {
            variants: variant_list,
            assembly,
        })
    }
}

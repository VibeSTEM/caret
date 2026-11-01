//! 公式布局：把文档数学树排成以 pt 为单位、基线为 y=0（向下为正）的绘制项列表。
//!
//! 排版算法移植自 Typst（见各子模块文件头），字体度量全部来自 OpenType MATH 表。
//! [`MathLayoutEngine`] 驻留字体并按（内容哈希、字号、显示/行内、字体标识）缓存结果。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod accent;
mod alphabet;
mod context;
mod convert;
mod fenced;
mod font;
mod fraction;
mod frame;
mod glyph;
mod radical;
mod run;
mod scripts;
mod table;

use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex};

use caret_doc::{DocumentUnits, MathNode, NodeId};

pub use context::{MathDiagnostic, MathSize};
pub use font::{MathFont, MathFontError, STIX_TWO_MATH};
pub use frame::MathColor;

use context::{Context, Style};
use convert::Converter;
use frame::Item;

/// 公式的排版样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MathStyle {
    /// 行内公式（TeX text style）。
    Inline,
    /// 显示公式（TeX display style）。
    Display,
}

/// 数学排版输入错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathLayoutError {
    InvalidSize,
}

impl Display for MathLayoutError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSize => formatter.write_str("math.layout.invalid_size"),
        }
    }
}

impl std::error::Error for MathLayoutError {}

/// 布局输出的绘制项；坐标以公式左端基线为原点，y 向下。
#[derive(Debug, Clone, PartialEq)]
pub enum MathItem {
    /// 字形；`(x, y)` 为字形原点（基线左端），`size` 为字号（pt）。
    Glyph {
        glyph: u16,
        x: f64,
        y: f64,
        size: f64,
        color: Option<MathColor>,
    },
    /// 实心矩形（分数线、根号顶线、上下横线、表格线），`(x, y)` 为左上角。
    Rule {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        color: Option<MathColor>,
    },
    /// 线段（删除线），从 `(x, y)` 到 `(x + dx, y + dy)`。
    Line {
        x: f64,
        y: f64,
        dx: f64,
        dy: f64,
        thickness: f64,
        color: Option<MathColor>,
    },
    /// 矩形背景或边框（`\colorbox`、`\boxed`、占位框），`(x, y)` 为左上角。
    Rectangle {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill: Option<MathColor>,
        stroke: Option<MathColor>,
        stroke_width: f64,
    },
}

/// 某个数学节点在公式中的范围，供命中测试、光标与选区使用。
#[derive(Debug, Clone, PartialEq)]
pub struct MathNodeBox {
    pub id: NodeId,
    pub x: f64,
    /// 顶边（相对基线，向下为正，通常为负）。
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// 节点自身基线相对公式基线的偏移。
    pub baseline: f64,
}

/// 排版结果。颜色为 `None` 的项使用正文颜色。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathLayout {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub items: Vec<MathItem>,
    /// 按前序遍历排列；外层节点在内层之前。
    pub nodes: Vec<MathNodeBox>,
    pub diagnostics: Vec<MathDiagnostic>,
}

/// 以给定字体与字号（pt）排版一棵数学树。
pub fn layout_math(
    font: &MathFont,
    root: &MathNode,
    size: f64,
    style: MathStyle,
) -> Result<MathLayout, MathLayoutError> {
    validate_size(size)?;
    let mut converter = Converter {
        ctx: Context::new(font, size),
    };
    let fragment = converter.node(root, Style::root(style == MathStyle::Display));
    let frame = fragment.frame;
    let baseline = frame.baseline;
    let mut layout = MathLayout {
        width: frame.width,
        ascent: frame.ascent(),
        descent: frame.descent(),
        items: Vec::with_capacity(frame.items.len()),
        nodes: Vec::new(),
        diagnostics: converter.ctx.diagnostics,
    };
    for (point, item) in frame.items {
        let (x, y) = (point.x, point.y - baseline);
        match item {
            Item::Glyph { glyph, size, color } => layout.items.push(MathItem::Glyph {
                glyph,
                x,
                y,
                size,
                color,
            }),
            Item::Rule {
                width,
                height,
                color,
            } => layout.items.push(MathItem::Rule {
                x,
                y,
                width,
                height,
                color,
            }),
            Item::Line {
                dx,
                dy,
                thickness,
                color,
            } => layout.items.push(MathItem::Line {
                x,
                y,
                dx,
                dy,
                thickness,
                color,
            }),
            Item::Rect {
                width,
                height,
                fill,
                stroke,
            } => layout.items.push(MathItem::Rectangle {
                x,
                y,
                width,
                height,
                fill,
                stroke: stroke.and_then(|(color, _)| color),
                stroke_width: stroke.map_or(0.0, |(_, width)| width),
            }),
            Item::Node {
                id,
                width,
                height,
                baseline: node_baseline,
            } => layout.nodes.push(MathNodeBox {
                id,
                x,
                y,
                width,
                height,
                baseline: y + node_baseline,
            }),
        }
    }
    Ok(layout)
}

fn validate_size(size: f64) -> Result<(), MathLayoutError> {
    if !size.is_finite() || size <= 0.0 || size > DocumentUnits::max_font_size_points() {
        return Err(MathLayoutError::InvalidSize);
    }
    Ok(())
}

/// 缓存键：内容哈希（含节点 ID）、字号位模式、样式、字体标识。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    content: u64,
    size_bits: u64,
    style: MathStyle,
    font: u64,
}

/// 缓存条目上限；超过时整体清空，避免无界增长。
const CACHE_LIMIT: usize = 4096;

/// 驻留字体与按公式缓存的布局引擎。
#[derive(Debug)]
pub struct MathLayoutEngine {
    font: &'static MathFont,
    cache: Mutex<HashMap<CacheKey, Arc<MathLayout>>>,
}

impl MathLayoutEngine {
    /// 使用内置 STIX Two Math 字体。
    pub fn new() -> Result<Self, MathFontError> {
        Ok(Self::with_font(MathFont::builtin()?))
    }

    pub fn with_font(font: &'static MathFont) -> Self {
        Self {
            font,
            cache: Mutex::new(HashMap::new()),
        }
    }

    pub fn font(&self) -> &'static MathFont {
        self.font
    }

    /// 排版并缓存；相同内容、字号、样式与字体的重复请求直接返回缓存结果。
    pub fn layout(
        &self,
        root: &MathNode,
        size: f64,
        style: MathStyle,
    ) -> Result<Arc<MathLayout>, MathLayoutError> {
        validate_size(size)?;
        let key = CacheKey {
            content: content_hash(root),
            size_bits: size.to_bits(),
            style,
            font: self.font.identity(),
        };
        if let Some(hit) = self.lock().get(&key) {
            return Ok(Arc::clone(hit));
        }
        let layout = Arc::new(layout_math(self.font, root, size, style)?);
        let mut cache = self.lock();
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, Arc::clone(&layout));
        Ok(layout)
    }

    /// 当前缓存条目数。
    pub fn cached_len(&self) -> usize {
        self.lock().len()
    }

    pub fn clear_cache(&self) {
        self.lock().clear();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CacheKey, Arc<MathLayout>>> {
        // 缓存内容在锁中毒后仍然一致（只有插入与清空），直接取回。
        self.cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// 节点内容哈希：ID、类别、文本、属性与子节点；未知扩展字段不影响布局，不参与哈希。
fn content_hash(root: &MathNode) -> u64 {
    fn feed(node: &MathNode, hasher: &mut DefaultHasher) {
        node.id.hash(hasher);
        format!("{:?}", node.kind).hash(hasher);
        node.text.hash(hasher);
        node.attributes.hash(hasher);
        node.children.len().hash(hasher);
        for child in &node.children {
            feed(child, hasher);
        }
    }
    let mut hasher = DefaultHasher::new();
    feed(root, &mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests;

/// 浮点判零容差（单位与被比较量相同）。
pub(crate) const FLOAT_EPSILON: f64 = 1e-9;

/// 判断浮点值是否在容差内为零。
pub(crate) fn is_zero(value: f64) -> bool {
    value.abs() < FLOAT_EPSILON
}

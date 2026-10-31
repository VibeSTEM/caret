//! 数学布局的中间帧：左上角原点、y 向下、带基线的矩形，推入子帧时立即展平。
//!
//! 帧模型与 Typst 的 `Frame` 一致（尺寸 + 基线 + 定位项），便于逐行对照移植的排版算法；
//! 片段在帧之外携带数学专有属性（类别、斜体校正、重音附着点、角落字距来源）。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件改写自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-layout/src/math/fragment/mod.rs
//   提交：84cca406aafe7f830ed3f37f1cc6f0c8b6b65a03（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：片段合并为单一结构体；帧推入子帧时立即展平；绘制项改为 Caret 的字形、矩形、
//   线段与节点记录；颜色使用 Caret 的 `MathColor`。

use caret_doc::NodeId;

use crate::symbols::AtomClass;

/// 帧内坐标点，单位 pt。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub(crate) fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// RGBA 颜色；`None` 表示继承正文前景色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MathColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl MathColor {
    /// 解析 `#rrggbb`。
    pub fn parse_hex(value: &str) -> Option<Self> {
        let hex = value.strip_prefix('#').unwrap_or(value);
        if hex.len() != 6 || !hex.is_ascii() {
            return None;
        }
        let channel = |index: usize| u8::from_str_radix(hex.get(index..index + 2)?, 16).ok();
        Some(Self {
            red: channel(0)?,
            green: channel(2)?,
            blue: channel(4)?,
            alpha: 255,
        })
    }
}

/// 帧内的绘制项；位置由所在的 `(Point, Item)` 给出。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Item {
    /// 字形，位置是字形原点（基线左端）。
    Glyph {
        glyph: u16,
        size: f64,
        color: Option<MathColor>,
    },
    /// 实心矩形，位置是左上角。
    Rule {
        width: f64,
        height: f64,
        color: Option<MathColor>,
    },
    /// 线段，位置是起点；终点为起点加 `(dx, dy)`。
    Line {
        dx: f64,
        dy: f64,
        thickness: f64,
        color: Option<MathColor>,
    },
    /// 矩形填充或描边，位置是左上角。
    Rect {
        width: f64,
        height: f64,
        fill: Option<MathColor>,
        stroke: Option<(Option<MathColor>, f64)>,
    },
    /// 节点占位范围，供命中测试与选区使用，不绘制。位置是左上角。
    Node {
        id: NodeId,
        width: f64,
        height: f64,
        baseline: f64,
    },
}

/// 左上角原点的帧。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Frame {
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
    pub items: Vec<(Point, Item)>,
}

impl Frame {
    pub(crate) fn new(width: f64, height: f64, baseline: f64) -> Self {
        Self {
            width,
            height,
            baseline,
            items: Vec::new(),
        }
    }

    pub(crate) fn ascent(&self) -> f64 {
        self.baseline
    }

    pub(crate) fn descent(&self) -> f64 {
        self.height - self.baseline
    }

    pub(crate) fn push(&mut self, position: Point, item: Item) {
        self.items.push((position, item));
    }

    /// 把子帧平移后并入当前帧。
    pub(crate) fn push_frame(&mut self, position: Point, frame: Frame) {
        self.items.extend(
            frame.items.into_iter().map(|(point, item)| {
                (Point::new(point.x + position.x, point.y + position.y), item)
            }),
        );
    }

    /// 移除绘制项，只保留节点范围（`\phantom`）。
    pub(crate) fn hide(&mut self) {
        self.items
            .retain(|(_, item)| matches!(item, Item::Node { .. }));
    }
}

/// 字形来源，供上下标查询 MATH 角落字距。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GlyphSource {
    pub glyph: u16,
    pub size: f64,
}

/// 排版片段：帧加数学属性。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fragment {
    pub frame: Frame,
    pub class: AtomClass,
    /// 是否是空白片段（显式间距），空白不参与原子间距。
    pub space: bool,
    pub italics_correction: f64,
    /// 上、下重音附着点，自左边缘起量。
    pub accent_attach: (f64, f64),
    pub base_ascent: f64,
    pub base_descent: f64,
    pub text_like: bool,
    /// 显示样式下上下限放在正上下方。
    pub limits: bool,
    /// 是否是大运算符（`\sum`、`\int` 等），影响间距与上下标。
    pub large: bool,
    /// 是否处于上下标字号，上下标字号不加运算符间距。
    pub script: bool,
    pub glyph: Option<GlyphSource>,
    /// 可水平伸展的单字形（如箭头），上下限按伸展栈排版。
    pub stretchy_x: bool,
}

impl Fragment {
    pub(crate) fn new(frame: Frame, class: AtomClass) -> Self {
        let attach = frame.width / 2.0;
        Self {
            base_ascent: frame.ascent(),
            base_descent: frame.descent(),
            frame,
            class,
            space: false,
            italics_correction: 0.0,
            accent_attach: (attach, attach),
            text_like: false,
            limits: false,
            large: false,
            script: false,
            glyph: None,
            stretchy_x: false,
        }
    }

    /// 宽度为 `width` 的空白。
    pub(crate) fn spacing(width: f64) -> Self {
        let mut fragment = Self::new(Frame::new(width, 0.0, 0.0), AtomClass::Ord);
        fragment.space = true;
        fragment
    }

    pub(crate) fn width(&self) -> f64 {
        self.frame.width
    }

    pub(crate) fn height(&self) -> f64 {
        self.frame.height
    }

    pub(crate) fn ascent(&self) -> f64 {
        self.frame.ascent()
    }

    pub(crate) fn descent(&self) -> f64 {
        self.frame.descent()
    }
}

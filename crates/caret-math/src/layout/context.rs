//! 布局上下文：字体、数学样式（字号级别、紧缩、颜色、字母表变体）与诊断收集。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use read_fonts::tables::math::MathConstant;

use super::font::MathFont;
use super::frame::MathColor;

/// TeX 的四级数学字号。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MathSize {
    ScriptScript,
    Script,
    Text,
    Display,
}

/// 数学字母表变体，决定字母与数字映射到哪个 Unicode 数学字母块。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    /// 默认：拉丁字母与小写希腊字母为斜体。
    Normal,
    Roman,
    Bold,
    Italic,
    BoldItalic,
    SansSerif,
    Monospace,
    DoubleStruck,
    Script,
    Fraktur,
}

/// 子树继承的样式。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Style {
    pub size: MathSize,
    pub cramped: bool,
    pub color: Option<MathColor>,
    pub variant: Variant,
    /// 文本模式（`\text{}`）：字符按原样直立显示，空格保留。
    pub text_mode: bool,
}

impl Style {
    pub(crate) fn root(display: bool) -> Self {
        Self {
            size: if display {
                MathSize::Display
            } else {
                MathSize::Text
            },
            cramped: false,
            color: None,
            variant: Variant::Normal,
            text_mode: false,
        }
    }

    /// 上标样式（TeXbook 规则 18a）。
    pub(crate) fn superscript(self) -> Self {
        Self {
            size: match self.size {
                MathSize::Display | MathSize::Text => MathSize::Script,
                MathSize::Script | MathSize::ScriptScript => MathSize::ScriptScript,
            },
            ..self
        }
    }

    /// 下标样式：上标样式并紧缩。
    pub(crate) fn subscript(self) -> Self {
        Self {
            cramped: true,
            ..self.superscript()
        }
    }

    /// 分子样式。
    pub(crate) fn numerator(self) -> Self {
        Self {
            size: match self.size {
                MathSize::Display => MathSize::Text,
                MathSize::Text => MathSize::Script,
                MathSize::Script | MathSize::ScriptScript => MathSize::ScriptScript,
            },
            ..self
        }
    }

    /// 分母样式：分子样式并紧缩。
    pub(crate) fn denominator(self) -> Self {
        Self {
            cramped: true,
            ..self.numerator()
        }
    }

    pub(crate) fn cramped(self) -> Self {
        Self {
            cramped: true,
            ..self
        }
    }

    pub(crate) fn with_size(self, size: MathSize) -> Self {
        Self { size, ..self }
    }

    pub(crate) fn is_script(self) -> bool {
        self.size <= MathSize::Script
    }
}

/// 布局诊断，键名稳定，供界面本地化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathDiagnostic {
    pub code: &'static str,
    pub detail: String,
}

pub(crate) struct Context<'a> {
    pub font: &'a MathFont,
    /// 正文字号（pt）。
    pub base_size: f64,
    pub diagnostics: Vec<MathDiagnostic>,
}

impl<'a> Context<'a> {
    pub(crate) fn new(font: &'a MathFont, base_size: f64) -> Self {
        Self {
            font,
            base_size,
            diagnostics: Vec::new(),
        }
    }

    /// 样式对应的实际字号（pt）。
    pub(crate) fn font_size(&self, style: Style) -> f64 {
        let percent = match style.size {
            MathSize::Display | MathSize::Text => 100.0,
            MathSize::Script => self.font.constant(MathConstant::ScriptPercentScaleDown),
            MathSize::ScriptScript => self
                .font
                .constant(MathConstant::ScriptScriptPercentScaleDown),
        };
        let percent = if percent > 0.0 { percent } else { 100.0 };
        self.base_size * percent / 100.0
    }

    /// 设计单位换算为 pt。
    pub(crate) fn units(&self, value: f64, size: f64) -> f64 {
        value * size / self.font.units_per_em()
    }

    /// 某字号下的 MATH 常量（pt）。
    pub(crate) fn constant(&self, constant: MathConstant, size: f64) -> f64 {
        self.units(self.font.constant(constant), size)
    }

    /// 样式字号下的 MATH 常量（pt）。
    pub(crate) fn styled(&self, constant: MathConstant, style: Style) -> f64 {
        self.constant(constant, self.font_size(style))
    }

    pub(crate) fn diagnostic(&mut self, code: &'static str, detail: impl Into<String>) {
        let detail = detail.into();
        if !self
            .diagnostics
            .iter()
            .any(|existing| existing.code == code && existing.detail == detail)
        {
            self.diagnostics.push(MathDiagnostic { code, detail });
        }
    }
}

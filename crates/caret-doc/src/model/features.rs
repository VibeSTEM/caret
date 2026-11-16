//! 根据文档内容推导所需功能标识。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;
use std::collections::BTreeSet;

impl Document {
    // `requiredFeatures` 的数学能力标识只在 `Document` 上登记一份；类型推导、raw
    // 声明扫描与已知能力清单都引用这里，避免同一字面量在两条路径各写一遍。
    pub(crate) const MATH_FEATURE_ALIGN: &'static str = "caret.math.align";
    pub(crate) const MATH_FEATURE_CONTENT_FLOW_V2: &'static str = "caret.math.content-flow.v2";
    pub(crate) const MATH_FEATURE_EQUATION: &'static str = "caret.math.equation";
    pub(crate) const MATH_FEATURE_INLINE: &'static str = "caret.math.inline";
    pub(crate) const MATH_FEATURE_SYNTAX_V1: &'static str = "caret.math.syntax.v1";

    /// 读取器要求在 `requiredFeatures` 中显式声明、否则按格式错误拒绝的数学能力。
    const REQUIRED_EXPLICIT_MATH_FEATURES: &'static [&'static str] = &[
        Self::MATH_FEATURE_ALIGN,
        Self::MATH_FEATURE_CONTENT_FLOW_V2,
        Self::MATH_FEATURE_EQUATION,
        Self::MATH_FEATURE_INLINE,
        Self::MATH_FEATURE_SYNTAX_V1,
    ];

    /// 判定数学能力是否必须由 envelope 显式声明。
    pub(crate) fn requires_explicit_math_declaration(feature: &str) -> bool {
        Self::REQUIRED_EXPLICIT_MATH_FEATURES.contains(&feature)
    }

    pub fn required_features(&self) -> Vec<String> {
        let mut features = BTreeSet::new();
        features.insert("caret.paragraph.layout-settings".to_owned());
        features.insert("caret.paragraph.content-flow.v1".to_owned());
        features.insert("caret.text.default-font-size".to_owned());
        if self.global_settings.default_language.is_some()
            || self.body.iter().any(block_has_language)
        {
            features.insert("caret.text.language-tags".to_owned());
        }
        if self.global_settings.theme.is_some() {
            features.insert("caret.theme.snapshot".to_owned());
        }
        if has_custom_family(self.global_settings.default_font_family.as_deref())
            || has_custom_family(self.global_settings.default_cjk_font_family.as_deref())
            || self
                .global_settings
                .default_math_font_family
                .as_deref()
                .is_some_and(|family| family != DEFAULT_FONT_FAMILY)
            || self.global_settings.default_code_font_family != DEFAULT_FONT_FAMILY
            || !self.global_settings.script_font_families.is_empty()
            || !self
                .global_settings
                .language_script_font_families
                .is_empty()
        {
            features.insert("caret.font.family-preferences".to_owned());
        }
        if self
            .global_settings
            .theme
            .as_ref()
            .is_some_and(|theme| !theme.environment_templates.is_empty())
        {
            features.insert("caret.env.templates".to_owned());
        }
        if self
            .global_settings
            .theme
            .as_ref()
            .is_some_and(|theme| !theme.default_inline_environment_templates.is_empty())
        {
            features.insert("caret.env.inline-text-templates.v1".to_owned());
        }
        for block in &self.body {
            collect_block_features(block, &mut features);
        }
        features.into_iter().collect()
    }
}

fn has_custom_family(family: Option<&str>) -> bool {
    family.is_some_and(|family| family != DEFAULT_FONT_FAMILY)
}

/// 判定块子树中是否存在语言标注。
///
/// 语言标注只挂在段落和文本 Run 上；数学树与行内环境只作为容器展开。
fn block_has_language(block: &Block) -> bool {
    walk_block(block, &mut LanguageProbe)
}

/// 把块子树涉及的功能标识并入 `features`。
///
/// 功能收集必须完整遍历整棵子树，收集器从不短路，骨架返回值恒为 `false`。
fn collect_block_features(block: &Block, features: &mut BTreeSet<String>) {
    walk_block(block, &mut FeatureCollector { features });
}

/// 文档树与数学树共用的遍历访问器。
///
/// 遍历骨架固定访问顺序并展开子树；访问器只实现与自身目的相关的节点处理。
/// 钩子返回 `true` 表示访问器已得出结论，骨架按存储顺序立即短路；返回 `false`
/// 表示继续遍历。默认实现不处理对应节点，供只关心少数节点的访问器复用。
trait FeatureVisitor {
    /// 段落自身；在遍历其行内内容之前访问。块环境标题复用同一钩子。
    fn visit_paragraph(&mut self, _paragraph: &Paragraph) -> bool {
        false
    }

    /// 块环境自身；在遍历其标题与子块之前访问。
    fn visit_block_environment(&mut self, _environment: &BlockEnvironment) -> bool {
        false
    }

    /// 行内环境自身；在遍历其子内容之前访问。
    fn visit_inline_environment<Child>(&mut self, _environment: &InlineEnvironment<Child>) -> bool {
        false
    }

    /// 文本流自身；其 Run 由访问器按需展开。
    fn visit_text_flow(&mut self, _text: &TextFlow) -> bool {
        false
    }

    /// 行内公式自身；在遍历其数学树之前访问。
    fn visit_inline_math(&mut self, _math: &InlineMath) -> bool {
        false
    }

    /// 数学节点自身；在遍历其操作数之前访问。
    fn visit_math_node(&mut self, _node: &MathNode) -> bool {
        false
    }

    /// 公式块自身；在遍历其数学树之前访问。
    fn visit_equation(&mut self, _equation: &Equation) -> bool {
        false
    }

    /// 对齐公式块自身；在遍历其单元格之前访问。
    fn visit_align(&mut self, _align: &Align) -> bool {
        false
    }
}

/// 块级遍历骨架：按块类型展开段落、公式块、对齐公式与块环境子树。
fn walk_block(block: &Block, visitor: &mut impl FeatureVisitor) -> bool {
    match block {
        Block::Paragraph(paragraph) => walk_paragraph(paragraph, visitor),
        Block::Equation(equation) => {
            visitor.visit_equation(equation) || walk_math_flow(&equation.math_tree.root, visitor)
        }
        Block::Align(align) => {
            visitor.visit_align(align)
                || align
                    .rows
                    .iter()
                    .flat_map(|row| &row.cells)
                    .any(|tree| walk_math_flow(&tree.root, visitor))
        }
        Block::Environment(environment) => {
            visitor.visit_block_environment(environment)
                || environment
                    .title
                    .as_ref()
                    .is_some_and(|title| walk_paragraph(title, visitor))
                || environment
                    .children
                    .iter()
                    .any(|child| walk_block(child, visitor))
        }
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    }
}

/// 段落遍历骨架：先访问段落自身，再按存储顺序遍历行内内容。
fn walk_paragraph(paragraph: &Paragraph, visitor: &mut impl FeatureVisitor) -> bool {
    visitor.visit_paragraph(paragraph) || walk_flows(&paragraph.inlines, visitor)
}

/// 内容流遍历骨架：按存储顺序遍历，任一内容项得出结论即短路。
fn walk_flows<T: MathPayload>(
    contents: &[FlowContent<T>],
    visitor: &mut impl FeatureVisitor,
) -> bool {
    contents.iter().any(|content| walk_flow(content, visitor))
}

/// 单个内容项遍历骨架：文本、数学载荷与行内环境各按自身规则展开。
fn walk_flow<T: MathPayload>(content: &FlowContent<T>, visitor: &mut impl FeatureVisitor) -> bool {
    match content {
        FlowContent::Text(text) => visitor.visit_text_flow(text),
        FlowContent::Math(math) => math.walk_math(visitor),
        FlowContent::Environment(environment) => {
            visitor.visit_inline_environment(environment)
                || walk_flows(&environment.children, visitor)
        }
        FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => false,
    }
}

/// 数学流遍历骨架：按存储顺序遍历其中的内容项。
fn walk_math_flow(flow: &MathFlow, visitor: &mut impl FeatureVisitor) -> bool {
    walk_flows(&flow.content, visitor)
}

/// 内容流携带的数学载荷。
///
/// 正文内容流与数学树内容流只在数学分支上不同：行内公式展开整棵数学树，
/// 数学节点展开其操作数；遍历顺序由各自类型决定。
trait MathPayload {
    /// 访问载荷自身，再按存储顺序展开其子数学流；返回 `true` 表示已得出结论。
    fn walk_math(&self, visitor: &mut impl FeatureVisitor) -> bool;
}

impl MathPayload for InlineMath {
    fn walk_math(&self, visitor: &mut impl FeatureVisitor) -> bool {
        visitor.visit_inline_math(self) || walk_math_flow(&self.math_tree.root, visitor)
    }
}

impl MathPayload for MathNode {
    fn walk_math(&self, visitor: &mut impl FeatureVisitor) -> bool {
        visitor.visit_math_node(self)
            || self
                .children
                .iter()
                .any(|child| walk_math_flow(child, visitor))
    }
}

/// 只判定是否存在语言标注：命中段落语言或文本 Run 语言即短路。
struct LanguageProbe;

impl FeatureVisitor for LanguageProbe {
    fn visit_paragraph(&mut self, paragraph: &Paragraph) -> bool {
        paragraph.language.is_some()
    }

    fn visit_text_flow(&mut self, text: &TextFlow) -> bool {
        text.runs.iter().any(|run| run.language.is_some())
    }
}

/// 收集文档所需功能：遍历全部节点，把每类节点对应的功能标识并入集合。
struct FeatureCollector<'a> {
    features: &'a mut BTreeSet<String>,
}

impl FeatureVisitor for FeatureCollector<'_> {
    fn visit_paragraph(&mut self, paragraph: &Paragraph) -> bool {
        if paragraph.language.is_some() {
            self.features.insert("caret.text.language-tags".to_owned());
        }
        false
    }

    fn visit_block_environment(&mut self, environment: &BlockEnvironment) -> bool {
        self.features.insert(
            match environment.kind {
                BlockEnvironmentKind::Section => "caret.env.section",
                BlockEnvironmentKind::Custom => "caret.env.custom",
            }
            .to_owned(),
        );
        if environment.definition_id.is_some() {
            self.features.insert("caret.env.templates".to_owned());
        }
        false
    }

    fn visit_inline_environment<Child>(&mut self, environment: &InlineEnvironment<Child>) -> bool {
        collect_environment_features(environment, self.features);
        false
    }

    fn visit_text_flow(&mut self, text: &TextFlow) -> bool {
        collect_text_flow_features(text, self.features);
        false
    }

    fn visit_inline_math(&mut self, _math: &InlineMath) -> bool {
        self.features
            .insert(Document::MATH_FEATURE_INLINE.to_owned());
        self.features
            .insert(Document::MATH_FEATURE_CONTENT_FLOW_V2.to_owned());
        false
    }

    fn visit_math_node(&mut self, node: &MathNode) -> bool {
        if math_node_requires_syntax(node.kind, !node.attributes.is_empty()) {
            self.features
                .insert(Document::MATH_FEATURE_SYNTAX_V1.to_owned());
        }
        false
    }

    fn visit_equation(&mut self, _equation: &Equation) -> bool {
        self.features
            .insert(Document::MATH_FEATURE_EQUATION.to_owned());
        self.features
            .insert(Document::MATH_FEATURE_CONTENT_FLOW_V2.to_owned());
        false
    }

    fn visit_align(&mut self, _align: &Align) -> bool {
        self.features
            .insert(Document::MATH_FEATURE_ALIGN.to_owned());
        self.features
            .insert(Document::MATH_FEATURE_CONTENT_FLOW_V2.to_owned());
        false
    }
}

/// 判断数学节点是否依赖当前版本显式登记的语法/属性契约。
pub(crate) fn math_node_requires_syntax(kind: MathNodeKind, has_attributes: bool) -> bool {
    has_attributes
        || matches!(
            kind,
            MathNodeKind::SizedDelimiter
                | MathNodeKind::Matrix
                | MathNodeKind::Substack
                | MathNodeKind::Intertext
                | MathNodeKind::Hline
                | MathNodeKind::Sum
                | MathNodeKind::Integral
                | MathNodeKind::Product
                | MathNodeKind::Limit
                | MathNodeKind::OverUnder
                | MathNodeKind::CancelTo
                | MathNodeKind::Color
                | MathNodeKind::TextColor
                | MathNodeKind::ColorBox
                | MathNodeKind::FColorBox
                | MathNodeKind::Strut
                | MathNodeKind::Phantom
        )
}

fn collect_text_flow_features(text: &TextFlow, features: &mut BTreeSet<String>) {
    for run in &text.runs {
        if run.language.is_some() {
            features.insert("caret.text.language-tags".to_owned());
        }
        if let Some(value) = run.props.get("underline") {
            features.insert(
                if value.is_boolean() {
                    "caret.text.underline"
                } else {
                    "caret.text.underline.extended"
                }
                .to_owned(),
            );
        }
    }
}

fn collect_environment_features(
    environment: &InlineEnvironment<impl Sized>,
    features: &mut BTreeSet<String>,
) {
    features.insert(environment.kind.required_feature().to_owned());
    if environment.definition_id.is_some() {
        features.insert("caret.env.templates".to_owned());
        features.insert("caret.env.inline-text-templates.v1".to_owned());
    }
    if environment.kind.supports_enabled() && environment.props.contains_key("enabled") {
        features.insert("caret.env.explicit-format-state".to_owned());
    }
}

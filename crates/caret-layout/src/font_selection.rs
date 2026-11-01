//! 编译正文、数学与代码字体偏好，并按角色和实际脚本生成候选字体族。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::collections::{BTreeMap, BTreeSet};

use crate::fonts::{code, math};
use caret_doc::DocumentSettings;
use icu_locale_core::{Locale, subtags::Script};
use icu_properties::{
    PropertyNamesLong, PropertyNamesShort, PropertyParser, props::Script as UnicodeScript,
};

/// 候选字体族来自哪一层文档偏好；实际字体覆盖由字体后端检查。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontPreferenceSource {
    Explicit,
    Code,
    Math,
    LanguageScript,
    Script,
    Cjk,
    Default,
}

/// 按优先级排列的字体族引用；借用偏好规则或调用者的显式族名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FontFamilyCandidate<'a> {
    pub family: &'a str,
    pub source: FontPreferenceSource,
}

/// 返回 ICU 当前 Unicode 数据中的真实单一脚本短标签。
pub fn unicode_script_tags() -> Vec<String> {
    let names = PropertyNamesShort::<UnicodeScript>::new();
    #[allow(deprecated)]
    UnicodeScript::ALL_VALUES
        .iter()
        .filter_map(|script| names.get_locale_script(*script))
        .map(|script| script.as_str().to_owned())
        .filter(|tag| {
            !matches!(
                tag.as_str(),
                "Zyyy" | "Zinh" | "Zzzz" | "Jpan" | "Kore" | "Hanb"
            )
        })
        .collect()
}

/// 返回 Unicode 单一脚本的规范短标签与英文名称，供脚本选择列表直接展示。
pub fn unicode_script_choices() -> Vec<(String, String)> {
    let short_names = PropertyNamesShort::<UnicodeScript>::new();
    let long_names = PropertyNamesLong::<UnicodeScript>::new();
    #[allow(deprecated)]
    UnicodeScript::ALL_VALUES
        .iter()
        .filter_map(|script| {
            let locale_script = short_names.get_locale_script(*script)?;
            let short = locale_script.as_str();
            if matches!(short, "Zyyy" | "Zinh" | "Zzzz" | "Jpan" | "Kore" | "Hanb") {
                return None;
            }
            let long = long_names.get(*script)?.to_owned();
            Some((short.to_owned(), long))
        })
        .collect()
}

/// 已完成文字分析后的字体选择上下文；不在这里重新分类字符。
#[derive(Debug, Clone, Copy)]
pub struct FontSelectionContext<'a> {
    /// Parley 等分析器解析出的真实脚本，不能由文档语言替代。
    pub script: Script,
    /// Run/Paragraph/文档继承后得到的 BCP 47 语言。
    pub language: Option<&'a str>,
    /// 数学公式使用独立字体角色，不继承正文 Script 字体偏好。
    pub math_context: bool,
    /// Font EnvironmentNode 等局部显式请求，优先于所有文档默认值。
    pub explicit_family: Option<&'a str>,
    /// 代码文本使用文档代码字体和等宽通用族，不继承正文脚本字体。
    pub code: bool,
}

/// 偏好键规范化失败；不因本机缺少某字体而返回此错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontPreferenceError {
    InvalidScriptTag(String),
    EmptyFamily,
    DuplicateScriptTag(String),
    DuplicateLanguageTag(String),
}

impl std::fmt::Display for FontPreferenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidScriptTag(tag) => write!(formatter, "font.invalid_script_tag: {tag}"),
            Self::EmptyFamily => formatter.write_str("font.empty_family"),
            Self::DuplicateScriptTag(tag) => write!(formatter, "font.duplicate_script_tag: {tag}"),
            Self::DuplicateLanguageTag(tag) => {
                write!(formatter, "font.duplicate_language_tag: {tag}")
            }
        }
    }
}

impl std::error::Error for FontPreferenceError {}

/// 按文档样式代际编译一次的字体偏好；候选查询不扫描字体目录。
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FontPreferenceRules {
    default_family: Option<String>,
    cjk_family: Option<String>,
    math_family: Option<String>,
    code_family: Option<String>,
    scripts: BTreeMap<Script, String>,
    language_scripts: BTreeMap<Script, BTreeMap<String, String>>,
}

/// 为文档设置代际编译一次的不可变 shaping 字体和语言上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDocumentTextSettings {
    default_language: Option<String>,
    font_preferences: FontPreferenceRules,
}

impl ResolvedDocumentTextSettings {
    /// 在打开文档或全局字体/语言设置变化时解析一次，供该代际的段落共享。
    pub fn from_document_settings(
        settings: &DocumentSettings,
    ) -> Result<Self, FontPreferenceError> {
        Ok(Self {
            default_language: settings.default_language.clone(),
            font_preferences: FontPreferenceRules::from_document_settings(settings)?,
        })
    }

    pub fn default_language(&self) -> Option<&str> {
        self.default_language.as_deref()
    }

    pub fn font_preferences(&self) -> &FontPreferenceRules {
        &self.font_preferences
    }
}

impl FontPreferenceRules {
    /// 使用 ICU4X 解析脚本键并规范化语言键，保留原始设置中的请求族名。
    ///
    /// 新的、形状有效的 ISO 15924 标签可编译；不把脚本注册表硬编码进文档模型。
    pub fn from_document_settings(
        settings: &DocumentSettings,
    ) -> Result<Self, FontPreferenceError> {
        let code_family = checked_family(Some(&settings.default_code_font_family))?;
        let mut result = Self {
            default_family: checked_family(settings.default_font_family.as_deref())?,
            cjk_family: checked_family(settings.default_cjk_font_family.as_deref())?,
            math_family: checked_family(settings.default_math_font_family.as_deref())?,
            code_family,
            ..Self::default()
        };
        let mut seen_scripts = BTreeSet::new();
        for (raw_script, family) in &settings.script_font_families {
            let Some(script) = parse_script(raw_script)? else {
                continue;
            };
            if !seen_scripts.insert(script) {
                return Err(FontPreferenceError::DuplicateScriptTag(raw_script.clone()));
            }
            if let Some(family) = checked_family(Some(family))? {
                result.scripts.insert(script, family);
            }
        }
        let mut seen_language_scripts = BTreeSet::new();
        for (raw_script, languages) in &settings.language_script_font_families {
            let Some(script) = parse_script(raw_script)? else {
                continue;
            };
            if !seen_language_scripts.insert(script) {
                return Err(FontPreferenceError::DuplicateScriptTag(raw_script.clone()));
            }
            let mut normalized = BTreeMap::new();
            let mut seen_languages = BTreeSet::new();
            for (language, family) in languages {
                let key = language_key(language);
                if !seen_languages.insert(key.clone()) {
                    return Err(FontPreferenceError::DuplicateLanguageTag(language.clone()));
                }
                if let Some(family) = checked_family(Some(family))? {
                    normalized.insert(key, family);
                }
            }
            if !normalized.is_empty() {
                result.language_scripts.insert(script, normalized);
            }
        }
        Ok(result)
    }

    /// 返回字体请求优先级；调用者仍须检查完整 shaping cluster 的 glyph 覆盖。
    ///
    /// 无候选时由宿主字体后端选择系统默认值。字体不存在不改写文档中的请求名。
    /// 候选可以直接交给 Fontique `Query::set_families`，原有 `FallbackKey` 继续负责
    /// 系统回退；这里不建立第二套字体查询。偏好匹配使用完整 BCP 47 标签；当前 Parley
    /// shaping 只接受其可表示的语言前缀，不能保证 variants/extensions 都传给 OpenType `locl`。
    pub fn family_candidates<'a>(
        &'a self,
        context: FontSelectionContext<'a>,
    ) -> Vec<FontFamilyCandidate<'a>> {
        let mut result = Vec::with_capacity(6);
        push_candidate(
            &mut result,
            context.explicit_family,
            FontPreferenceSource::Explicit,
        );
        if context.code {
            push_candidate(
                &mut result,
                self.code_family.as_deref(),
                FontPreferenceSource::Code,
            );
            push_candidate(
                &mut result,
                Some(code::default_family()),
                FontPreferenceSource::Code,
            );
            return result;
        }
        if context.math_context {
            push_candidate(
                &mut result,
                self.math_family.as_deref(),
                FontPreferenceSource::Math,
            );
            push_candidate(
                &mut result,
                Some(math::default_family()),
                FontPreferenceSource::Math,
            );
            return result;
        }
        if let Some(language) = context.language
            && let Some(languages) = self.language_scripts.get(&context.script)
        {
            for key in language_lookup_chain(language) {
                push_candidate(
                    &mut result,
                    languages.get(&key).map(String::as_str),
                    FontPreferenceSource::LanguageScript,
                );
            }
        }
        push_candidate(
            &mut result,
            self.scripts.get(&context.script).map(String::as_str),
            FontPreferenceSource::Script,
        );
        if is_cjk_script(context.script) {
            push_candidate(
                &mut result,
                self.cjk_family.as_deref(),
                FontPreferenceSource::Cjk,
            );
        }
        push_candidate(
            &mut result,
            self.default_family.as_deref(),
            FontPreferenceSource::Default,
        );
        result
    }
}

fn checked_family(family: Option<&str>) -> Result<Option<String>, FontPreferenceError> {
    match crate::fonts::requested_family(family) {
        None => Ok(None),
        Some(family) if family.trim().is_empty() => Err(FontPreferenceError::EmptyFamily),
        Some(family) => Ok(Some(family.to_owned())),
    }
}

fn parse_script(tag: &str) -> Result<Option<Script>, FontPreferenceError> {
    if tag.len() != 4 || !tag.bytes().all(|byte| byte.is_ascii_alphabetic()) {
        return Err(FontPreferenceError::InvalidScriptTag(tag.to_owned()));
    }
    let Some(unicode_script) = PropertyParser::<UnicodeScript>::new().get_loose(tag) else {
        return Ok(None);
    };
    let names = PropertyNamesShort::<UnicodeScript>::new();
    let Some(script) = names.get_locale_script(unicode_script) else {
        return Ok(None);
    };
    if !script.as_str().eq_ignore_ascii_case(tag) {
        return Ok(None);
    }
    Ok(Some(script))
}

fn language_key(tag: &str) -> String {
    // ICU 的 locale 类型覆盖结构化 BCP 47 locale；原模型允许的 grandfathered/private-use
    // 标签仍保留并可作精确匹配，不在这里重写完整的 BCP 47 解析器。
    Locale::normalize(tag).map_or_else(
        |_| tag.to_ascii_lowercase(),
        |normalized| normalized.to_ascii_lowercase(),
    )
}

fn language_lookup_chain(tag: &str) -> Vec<String> {
    let Ok(locale) = tag.parse::<Locale>() else {
        return vec![language_key(tag)];
    };
    // locale_core 没有 RFC 4647 lookup helper。解析由 ICU 完成，这里只实现 lookup 的
    // 右侧逐级删减策略；单字符 extension singleton 与其余部分一并删除。
    let mut current = locale.to_string().to_ascii_lowercase();
    let mut result = Vec::new();
    loop {
        result.push(current.clone());
        let Some(index) = current.rfind('-') else {
            break;
        };
        current.truncate(index);
        if current
            .rsplit('-')
            .next()
            .is_some_and(|part| part.len() == 1)
        {
            if let Some(index) = current.rfind('-') {
                current.truncate(index);
            } else {
                break;
            }
        }
    }
    result
}

fn is_cjk_script(script: Script) -> bool {
    matches!(script.as_str(), "Hani" | "Hira" | "Kana" | "Hang" | "Bopo")
}

fn push_candidate<'a>(
    result: &mut Vec<FontFamilyCandidate<'a>>,
    family: Option<&'a str>,
    source: FontPreferenceSource,
) {
    let Some(family) =
        crate::fonts::requested_family(family).filter(|family| !family.trim().is_empty())
    else {
        return;
    };
    if !result
        .iter()
        .any(|candidate| candidate.family.to_lowercase() == family.to_lowercase())
    {
        result.push(FontFamilyCandidate { family, source });
    }
}

#[cfg(test)]
mod tests;

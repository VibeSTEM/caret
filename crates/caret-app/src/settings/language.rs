//! 应用语言标识、支持目录与当前选择；不依赖 Qt 翻译器或资源系统。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use serde::Serialize;
use std::fmt;

/// 新增支持语言时扩展此目录。locale 标识采用规范写法；QM 文件名和资源
/// 路径由 C++/Qt 运行时根据构建约定解析，不在领域层保存。
const SUPPORTED_LANGUAGES: &[LanguageDefinition] = &[
    LanguageDefinition {
        locale: "en",
        display_name: "English",
    },
    LanguageDefinition {
        locale: "zh-Hans",
        display_name: "简体中文",
    },
    LanguageDefinition {
        locale: "zh-Hant",
        display_name: "繁體中文",
    },
    LanguageDefinition {
        locale: "ja",
        display_name: "日本語",
    },
];
const SUPPORTED_LOCALES: &[&str] = &["en", "zh-Hans", "zh-Hant", "ja"];
const DEFAULT_LOCALE: &str = "en";

/// 应用语言目录中的稳定标识和原生显示名称。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguageDefinition {
    pub locale: &'static str,
    pub display_name: &'static str,
}

/// 经支持目录校验的 locale 标识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleId(String);

impl LocaleId {
    /// 接受规范 locale；旧版 zh-CN 配置迁移为通用简体脚本 zh-Hans。
    pub fn parse(value: &str) -> Result<Self, LocaleError> {
        if value == "zh-CN" {
            Ok(Self("zh-Hans".to_owned()))
        } else if SUPPORTED_LOCALES.contains(&value) {
            Ok(Self(value.to_owned()))
        } else {
            Err(LocaleError::Unsupported(value.to_owned()))
        }
    }

    /// 返回通过校验的规范 locale 标识。
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for LocaleId {
    fn default() -> Self {
        debug_assert!(SUPPORTED_LOCALES.contains(&DEFAULT_LOCALE));
        Self(DEFAULT_LOCALE.to_owned())
    }
}

/// 不支持的 locale，保留原始输入以供边界层诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocaleError {
    Unsupported(String),
}

impl fmt::Display for LocaleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(locale) => {
                write!(formatter, "language.unsupported_locale: {locale}")
            }
        }
    }
}

impl std::error::Error for LocaleError {}

/// 持有当前应用 locale；切换流程先校验，再由 Qt 成功加载字典后提交。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LanguageService {
    current: LocaleId,
}

impl LanguageService {
    /// 返回当前应用构建支持的 locale 目录。
    pub fn supported_locales() -> &'static [&'static str] {
        SUPPORTED_LOCALES
    }

    /// 返回用于界面选择器的语言元数据。
    pub fn supported_languages() -> &'static [LanguageDefinition] {
        SUPPORTED_LANGUAGES
    }

    /// 返回序列化的语言目录，供 Qt 选择器显示，不由界面推导语言名称。
    pub fn catalog_json() -> Result<String, serde_json::Error> {
        serde_json::to_string(SUPPORTED_LANGUAGES)
    }

    /// 返回当前已提交的 locale。
    pub fn current(&self) -> &LocaleId {
        &self.current
    }

    /// 校验候选 locale，不提前改变当前状态。
    pub fn validate(&self, candidate: &str) -> Result<LocaleId, LocaleError> {
        LocaleId::parse(candidate)
    }

    /// 提交已成功加载翻译目录的 locale。
    pub fn commit(&mut self, candidate: LocaleId) {
        self.current = candidate;
    }
}

#[cfg(test)]
mod tests {
    use super::{LanguageService, LocaleError, LocaleId};

    #[test]
    fn default_locale_is_supported_and_catalog_has_no_duplicates() {
        let service = LanguageService::default();
        let supported = LanguageService::supported_languages();

        assert!(
            supported
                .iter()
                .any(|language| language.locale == service.current().as_str())
        );
        for (index, locale) in supported.iter().enumerate() {
            assert_eq!(
                LocaleId::parse(locale.locale)
                    .as_ref()
                    .map(LocaleId::as_str),
                Ok(locale.locale)
            );
            assert!(
                !supported[..index]
                    .iter()
                    .any(|previous| previous.locale == locale.locale),
                "duplicate locale: {}",
                locale.locale
            );
        }
    }

    #[test]
    fn supported_locale_metadata_matches_the_built_in_translation_files() {
        use caret_dsl_core::{Status, parse};

        fn placeholders(value: &str) -> Vec<String> {
            let characters: Vec<char> = value.chars().collect();
            let mut found = Vec::new();
            let mut index = 0;
            while index < characters.len() {
                if characters[index] == '%' && index + 1 < characters.len() {
                    if characters[index + 1] == 'n' {
                        found.push("%n".to_owned());
                        index += 2;
                        continue;
                    }
                    if characters[index + 1].is_ascii_digit() {
                        let start = index;
                        index += 1;
                        while index < characters.len() && characters[index].is_ascii_digit() {
                            index += 1;
                        }
                        found.push(characters[start..index].iter().collect());
                        continue;
                    }
                }
                index += 1;
            }
            found.sort();
            found
        }

        let source = [
            include_str!("../../../../resources/i18n/caret-en.pa"),
            include_str!("../../../../resources/i18n/caret-zh-Hans.pa"),
            include_str!("../../../../resources/i18n/caret-zh-Hant.pa"),
            include_str!("../../../../resources/i18n/caret-ja.pa"),
        ];
        let locales = LanguageService::supported_locales();

        assert_eq!(source.len(), locales.len());
        assert_eq!(
            LanguageService::supported_languages()
                .iter()
                .map(|language| language.locale)
                .collect::<Vec<_>>(),
            locales
        );
        for (locale, catalog) in locales.iter().zip(source) {
            assert!(catalog.contains(&format!("language: {locale}\n")));
            assert!(catalog.contains("kind: language\n"));
        }
        let parsed: Vec<_> = source
            .iter()
            .map(|catalog| match parse(catalog) {
                Ok(document) => document,
                Err(error) => panic!("built-in translation source must parse: {error}"),
            })
            .collect();
        let english = &parsed[0];
        for (locale, document) in locales.iter().zip(parsed.iter()) {
            assert_eq!(document.language.as_deref(), Some(*locale));
            assert_eq!(document.messages.len(), english.messages.len());
            for (id, source_message) in &english.messages {
                let translated = document
                    .messages
                    .get(id)
                    .unwrap_or_else(|| panic!("{locale} is missing message {id}"));
                assert_eq!(translated.context, source_message.context);
                assert_eq!(translated.source, source_message.source);
                assert_eq!(translated.status, Some(Status::Finished));
                let translation = translated
                    .translations
                    .get(*locale)
                    .and_then(|value| value.forms.first())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| panic!("{locale} has no finished translation for {id}"));
                assert_eq!(
                    placeholders(&translated.source),
                    placeholders(translation),
                    "placeholder mismatch in {locale}/{id}"
                );
            }
        }
        let json = match LanguageService::catalog_json() {
            Ok(json) => json,
            Err(error) => panic!("static language catalog must serialize: {error}"),
        };
        for language in LanguageService::supported_languages() {
            assert!(json.contains(&format!("\"locale\":\"{}\"", language.locale)));
            assert!(!language.display_name.is_empty());
        }
    }

    #[test]
    fn validation_does_not_change_current_locale_until_commit() {
        let mut service = LanguageService::default();
        let original = service.current().clone();
        let candidate = match service.validate("zh-Hans") {
            Ok(locale) => locale,
            Err(error) => panic!("supported locale was rejected: {error}"),
        };

        assert_eq!(service.current(), &original);
        service.commit(candidate);
        assert_eq!(service.current().as_str(), "zh-Hans");
    }

    #[test]
    fn unsupported_locale_preserves_current_state_and_input() {
        let mut service = LanguageService::default();
        let candidate = match service.validate("zh-Hans") {
            Ok(locale) => locale,
            Err(error) => panic!("supported locale was rejected: {error}"),
        };
        service.commit(candidate);

        for candidate in ["", "zh_CN", "en-US", "fr"] {
            assert_eq!(
                service.validate(candidate),
                Err(LocaleError::Unsupported(candidate.to_owned()))
            );
            assert_eq!(service.current().as_str(), "zh-Hans");
        }
    }

    #[test]
    fn legacy_mainland_locale_is_normalized_to_the_script_identifier() {
        let locale = match LocaleId::parse("zh-CN") {
            Ok(locale) => locale,
            Err(error) => panic!("legacy locale must be accepted: {error}"),
        };
        assert_eq!(locale.as_str(), "zh-Hans");
        assert!(LanguageService::supported_locales().contains(&"zh-Hans"));
    }
}

//! 应用语言标识、支持目录与当前选择；不依赖 Qt 翻译器或资源系统。

use std::fmt;

/// 新增支持语言时扩展此目录。locale 标识采用规范写法；QM 文件名和资源
/// 路径由 C++/Qt 运行时根据构建约定解析，不在领域层保存。
const SUPPORTED_LOCALES: &[&str] = &["en", "zh-CN"];
const DEFAULT_LOCALE: &str = "en";

/// 经支持目录校验的 locale 标识。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocaleId(String);

impl LocaleId {
    /// 只接受支持目录中的规范 locale 标识；不做隐式大小写或分隔符转换。
    pub fn parse(value: &str) -> Result<Self, LocaleError> {
        if SUPPORTED_LOCALES.contains(&value) {
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
        let supported = LanguageService::supported_locales();

        assert!(supported.contains(&service.current().as_str()));
        for (index, locale) in supported.iter().enumerate() {
            assert_eq!(
                LocaleId::parse(locale).as_ref().map(LocaleId::as_str),
                Ok(*locale)
            );
            assert!(
                !supported[..index].contains(locale),
                "duplicate locale: {locale}"
            );
        }
    }

    #[test]
    fn validation_does_not_change_current_locale_until_commit() {
        let mut service = LanguageService::default();
        let original = service.current().clone();
        let candidate = match service.validate("zh-CN") {
            Ok(locale) => locale,
            Err(error) => panic!("supported locale was rejected: {error}"),
        };

        assert_eq!(service.current(), &original);
        service.commit(candidate);
        assert_eq!(service.current().as_str(), "zh-CN");
    }

    #[test]
    fn unsupported_locale_preserves_current_state_and_input() {
        let mut service = LanguageService::default();
        let candidate = match service.validate("zh-CN") {
            Ok(locale) => locale,
            Err(error) => panic!("supported locale was rejected: {error}"),
        };
        service.commit(candidate);

        for candidate in ["", "zh_CN", "en-US", "fr"] {
            assert_eq!(
                service.validate(candidate),
                Err(LocaleError::Unsupported(candidate.to_owned()))
            );
            assert_eq!(service.current().as_str(), "zh-CN");
        }
    }
}

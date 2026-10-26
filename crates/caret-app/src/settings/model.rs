//! Settings 的持久化快照、版本与校验错误。

use super::language::LanguageService;
use super::preferences::{PreferencesError, UserPreferences};
use super::theme::DEFAULT_THEME_ID;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

/// 可以在设置文件和以后的界面边界之间传递的完整用户偏好快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserSettings {
    pub schema_version: u32,
    pub theme_id: String,
    pub language: String,
    #[serde(flatten)]
    pub preferences: UserPreferences,
    #[serde(flatten, default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(super) unknown_fields: BTreeMap<String, serde_json::Value>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_id: DEFAULT_THEME_ID.to_owned(),
            language: LanguageService::default().current().as_str().to_owned(),
            preferences: UserPreferences::default(),
            unknown_fields: BTreeMap::new(),
        }
    }
}

/// 偏好快照中不符合当前 schema 的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    UnsupportedVersion(u32),
    InvalidThemeId(String),
    InvalidLanguage(String),
    InvalidPreferences(PreferencesError),
    UnknownFields(Vec<String>),
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedVersion(version) => {
                write!(formatter, "settings.unsupported_version: {version}")
            }
            Self::InvalidThemeId(id) => write!(formatter, "settings.invalid_theme_id: {id}"),
            Self::InvalidLanguage(language) => {
                write!(formatter, "settings.invalid_language: {language}")
            }
            Self::InvalidPreferences(error) => write!(formatter, "{error}"),
            Self::UnknownFields(fields) => {
                write!(formatter, "settings.unknown_fields: {fields:?}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

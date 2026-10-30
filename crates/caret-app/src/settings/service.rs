//! Settings 分类服务的组合、候选校验与原子状态发布。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::language::LanguageService;
use super::model::{SETTINGS_SCHEMA_VERSION, SettingsError, UserSettings};
use super::preferences::PreferencesService;
use super::storage::{SettingsSaveOutcome, SettingsUpdateError};
use super::theme::{self, ThemeService};
use std::collections::BTreeMap;
use std::path::Path;

/// 组合分类服务；候选校验与发布都在 Rust 内完成。
#[derive(Debug, Clone)]
pub struct SettingsService {
    schema_version: u32,
    preferences: PreferencesService,
    theme: ThemeService,
    language: LanguageService,
}

impl SettingsService {
    pub fn new(theme: ThemeService, language: LanguageService) -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            preferences: PreferencesService::default(),
            theme,
            language,
        }
    }

    pub fn builtin() -> Result<Self, theme::ThemeError> {
        Ok(Self::new(
            theme::builtin_theme_service()?,
            LanguageService::default(),
        ))
    }

    /// 从三个领域服务拼装设置快照，不在总服务里保存并行字段副本。
    pub fn current(&self) -> UserSettings {
        UserSettings {
            schema_version: self.schema_version,
            theme_id: self.theme.current().id.clone(),
            language: self.language.current().as_str().to_owned(),
            preferences: self.preferences.current().clone(),
            unknown_fields: BTreeMap::new(),
        }
    }

    /// 从同一份活动 ThemeService 派生供 ViewModel 使用的主题目录快照。
    pub fn theme_catalog_json(&self) -> Result<String, theme::ThemeError> {
        self.theme.bridge_json()
    }

    pub fn validate(&self, candidate: UserSettings) -> Result<UserSettings, SettingsError> {
        if candidate.schema_version != SETTINGS_SCHEMA_VERSION {
            return Err(SettingsError::UnsupportedVersion(candidate.schema_version));
        }
        if !candidate.unknown_fields.is_empty() {
            return Err(SettingsError::UnknownFields(
                candidate.unknown_fields.keys().cloned().collect(),
            ));
        }
        if !self.theme.contains(&candidate.theme_id) {
            return Err(SettingsError::InvalidThemeId(candidate.theme_id.clone()));
        }
        self.language
            .validate(&candidate.language)
            .map_err(|_| SettingsError::InvalidLanguage(candidate.language.clone()))?;
        self.preferences
            .validate(candidate.preferences.clone())
            .map_err(SettingsError::InvalidPreferences)?;
        Ok(candidate)
    }

    pub fn commit(&mut self, candidate: UserSettings) -> Result<(), SettingsError> {
        let candidate = self.validate(candidate)?;
        let locale = self
            .language
            .validate(&candidate.language)
            .map_err(|_| SettingsError::InvalidLanguage(candidate.language.clone()))?;
        self.preferences
            .commit(candidate.preferences)
            .map_err(SettingsError::InvalidPreferences)?;
        self.theme
            .select(&candidate.theme_id)
            .map_err(|_| SettingsError::InvalidThemeId(candidate.theme_id.clone()))?;
        self.language.commit(locale);
        self.schema_version = candidate.schema_version;
        Ok(())
    }

    /// 临时文件替换目标后发布新状态；提交前失败时保留当前快照。
    /// 替换后的目录同步失败只产生持久性警告，不把已提交的结果报告为失败。
    pub fn commit_and_save(
        &mut self,
        candidate: UserSettings,
        path: &Path,
    ) -> Result<SettingsSaveOutcome, SettingsUpdateError> {
        let mut next = self.clone();
        next.commit(candidate)
            .map_err(SettingsUpdateError::Validation)?;
        let outcome = next
            .save_file(path)
            .map_err(SettingsUpdateError::Persistence)?;
        *self = next;
        Ok(outcome)
    }

    pub fn candidate_recent_project(&self, path: &str) -> Result<UserSettings, SettingsError> {
        let preferences = self
            .preferences
            .candidate_recent_project(path)
            .map_err(SettingsError::InvalidPreferences)?;
        let mut candidate = self.current();
        candidate.preferences = preferences;
        self.validate(candidate)
    }
}

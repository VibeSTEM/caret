//! 用户可调偏好及其领域校验；应用设置总协调由 `settings` 模块负责。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub const DEFAULT_WINDOW_WIDTH: u32 = 1280;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 720;
pub const MINIMUM_WINDOW_WIDTH: u32 = 880;
pub const MINIMUM_WINDOW_HEIGHT: u32 = 560;
pub const DEFAULT_CARET_BLINK_ENABLED: bool = true;
pub const DEFAULT_COMMAND_SUGGESTIONS_ENABLED: bool = true;
pub const DEFAULT_AUTO_SAVE_ENABLED: bool = false;
pub const MAX_WINDOW_DIMENSION: u32 = 16_384;
pub const MAX_RECENT_PROJECTS: usize = 10;
const MAX_PATH_BYTES: usize = 32 * 1024;

/// 持久化的用户偏好；不包含主题定义或应用语言服务状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserPreferences {
    pub window_width: u32,
    pub window_height: u32,
    pub window_maximized: bool,
    /// 插入点获得焦点时是否跟随操作系统闪烁周期。
    #[serde(default = "default_caret_blink_enabled")]
    pub caret_blink_enabled: bool,
    /// 是否显示反斜线命令的候选补全列表。
    #[serde(default = "default_command_suggestions_enabled")]
    pub command_suggestions_enabled: bool,
    /// 是否在命名文档编辑后自动保存。
    #[serde(default = "default_auto_save_enabled")]
    pub auto_save_enabled: bool,
    pub recent_project_paths: Vec<String>,
}

const fn default_caret_blink_enabled() -> bool {
    DEFAULT_CARET_BLINK_ENABLED
}

const fn default_command_suggestions_enabled() -> bool {
    DEFAULT_COMMAND_SUGGESTIONS_ENABLED
}

const fn default_auto_save_enabled() -> bool {
    DEFAULT_AUTO_SAVE_ENABLED
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            window_maximized: true,
            caret_blink_enabled: DEFAULT_CARET_BLINK_ENABLED,
            command_suggestions_enabled: DEFAULT_COMMAND_SUGGESTIONS_ENABLED,
            auto_save_enabled: DEFAULT_AUTO_SAVE_ENABLED,
            recent_project_paths: Vec::new(),
        }
    }
}

/// 用户偏好候选不符合当前领域约束。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferencesError {
    InvalidWindowWidth(u32),
    InvalidWindowHeight(u32),
    TooManyRecentProjects(usize),
    InvalidRecentProjectPath(String),
}

impl fmt::Display for PreferencesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWindowWidth(width) => {
                write!(formatter, "preferences.invalid_window_width: {width}")
            }
            Self::InvalidWindowHeight(height) => {
                write!(formatter, "preferences.invalid_window_height: {height}")
            }
            Self::TooManyRecentProjects(count) => {
                write!(formatter, "preferences.too_many_recent_projects: {count}")
            }
            Self::InvalidRecentProjectPath(path) => {
                write!(formatter, "preferences.invalid_recent_project_path: {path}")
            }
        }
    }
}

impl std::error::Error for PreferencesError {}

/// 持有经验证的用户偏好，不承担主题、语言或文件系统职责。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreferencesService {
    current: UserPreferences,
}

impl PreferencesService {
    pub fn current(&self) -> &UserPreferences {
        &self.current
    }

    pub fn validate(
        &self,
        candidate: UserPreferences,
    ) -> Result<UserPreferences, PreferencesError> {
        validate_preferences(candidate)
    }

    pub fn commit(
        &mut self,
        candidate: UserPreferences,
    ) -> Result<&UserPreferences, PreferencesError> {
        self.current = self.validate(candidate)?;
        Ok(&self.current)
    }

    pub fn candidate_recent_project(
        &self,
        path: &str,
    ) -> Result<UserPreferences, PreferencesError> {
        let mut candidate = self.current.clone();
        validate_recent_project_path(path)?;
        candidate
            .recent_project_paths
            .retain(|existing| existing != path);
        candidate.recent_project_paths.insert(0, path.to_owned());
        candidate.recent_project_paths.truncate(MAX_RECENT_PROJECTS);
        self.validate(candidate)
    }
}

fn validate_preferences(candidate: UserPreferences) -> Result<UserPreferences, PreferencesError> {
    if !(MINIMUM_WINDOW_WIDTH..=MAX_WINDOW_DIMENSION).contains(&candidate.window_width) {
        return Err(PreferencesError::InvalidWindowWidth(candidate.window_width));
    }
    if !(MINIMUM_WINDOW_HEIGHT..=MAX_WINDOW_DIMENSION).contains(&candidate.window_height) {
        return Err(PreferencesError::InvalidWindowHeight(
            candidate.window_height,
        ));
    }
    if candidate.recent_project_paths.len() > MAX_RECENT_PROJECTS {
        return Err(PreferencesError::TooManyRecentProjects(
            candidate.recent_project_paths.len(),
        ));
    }
    for (index, path) in candidate.recent_project_paths.iter().enumerate() {
        validate_recent_project_path(path)?;
        if candidate.recent_project_paths[..index].contains(path) {
            return Err(PreferencesError::InvalidRecentProjectPath(path.clone()));
        }
    }
    Ok(candidate)
}

fn validate_recent_project_path(value: &str) -> Result<(), PreferencesError> {
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.contains('\0')
        || !Path::new(value).is_absolute()
    {
        return Err(PreferencesError::InvalidRecentProjectPath(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_AUTO_SAVE_ENABLED, DEFAULT_CARET_BLINK_ENABLED, DEFAULT_WINDOW_HEIGHT,
        DEFAULT_WINDOW_WIDTH, MAX_RECENT_PROJECTS, MAX_WINDOW_DIMENSION, MINIMUM_WINDOW_WIDTH,
        PreferencesError, PreferencesService, UserPreferences,
    };

    fn absolute_path(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn defaults_are_valid_and_only_contain_user_preferences() {
        let service = PreferencesService::default();
        assert_eq!(service.current().window_width, DEFAULT_WINDOW_WIDTH);
        assert_eq!(service.current().window_height, DEFAULT_WINDOW_HEIGHT);
        assert_eq!(
            service.current().caret_blink_enabled,
            DEFAULT_CARET_BLINK_ENABLED
        );
        assert_eq!(
            service.current().auto_save_enabled,
            DEFAULT_AUTO_SAVE_ENABLED
        );
        assert!(service.validate(service.current().clone()).is_ok());
    }

    #[test]
    fn invalid_window_dimensions_do_not_replace_current_preferences() {
        let mut service = PreferencesService::default();
        let original = service.current().clone();
        for candidate in [
            UserPreferences {
                window_width: MINIMUM_WINDOW_WIDTH - 1,
                ..original.clone()
            },
            UserPreferences {
                window_height: MAX_WINDOW_DIMENSION + 1,
                ..original.clone()
            },
        ] {
            assert!(service.commit(candidate).is_err());
            assert_eq!(service.current(), &original);
        }
    }

    #[test]
    fn recent_projects_are_deduplicated_ordered_and_bounded() -> Result<(), PreferencesError> {
        let paths: Vec<_> = (0..MAX_RECENT_PROJECTS)
            .map(|index| absolute_path(&format!("{index}.cdoc")))
            .collect();
        let mut service = PreferencesService::default();
        let mut candidate = service.current().clone();
        candidate.recent_project_paths = paths.clone();
        service.commit(candidate)?;

        let promoted = service.candidate_recent_project(&absolute_path("new.cdoc"))?;
        assert_eq!(promoted.recent_project_paths.len(), MAX_RECENT_PROJECTS);
        assert!(
            !promoted
                .recent_project_paths
                .contains(&paths[MAX_RECENT_PROJECTS - 1])
        );
        assert_eq!(
            service.candidate_recent_project("relative.cdoc"),
            Err(PreferencesError::InvalidRecentProjectPath(
                "relative.cdoc".into()
            ))
        );
        Ok(())
    }
}

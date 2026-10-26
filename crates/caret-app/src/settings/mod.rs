//! Settings 总协调入口，组合 Preferences、Theme、Language 并持久化兼容快照。

pub mod language;
pub mod preferences;
pub mod theme;

use self::language::LanguageService;
use self::preferences::{PreferencesError, PreferencesService, UserPreferences};
pub use self::theme::DEFAULT_THEME_ID;
use self::theme::ThemeService;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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
    unknown_fields: BTreeMap<String, serde_json::Value>,
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

/// 先验证完整候选，再由 Qt adapter 持久化并提交，避免失败写入改变当前状态。
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

    /// 校验和写盘均成功后才发布新状态；失败时保留当前服务快照。
    pub fn commit_and_save(
        &mut self,
        candidate: UserSettings,
        path: &Path,
    ) -> Result<(), SettingsUpdateError> {
        let mut next = self.clone();
        next.commit(candidate)
            .map_err(SettingsUpdateError::Validation)?;
        next.save_file(path)
            .map_err(SettingsUpdateError::Persistence)?;
        *self = next;
        Ok(())
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

    /// 从文件载入快照。文件不存在时保留内存中的当前快照。
    /// 内容无法解码或校验失败时，把原字节隔离到旁路文件，不改内存。
    pub fn load_file(&mut self, path: &Path) -> Result<(), SettingsFileError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(SettingsFileError::Io(format!(
                    "settings.storage_access_error: {error}"
                )));
            }
        };
        let candidate = match serde_json::from_slice::<UserSettings>(&bytes) {
            Ok(candidate) => candidate,
            Err(error) => {
                let quarantined = quarantine_settings_file(path, &bytes)?;
                return Err(SettingsFileError::Isolated {
                    quarantined,
                    reason: error.to_string(),
                });
            }
        };
        if let Err(error) = self.commit(candidate) {
            let quarantined = quarantine_settings_file(path, &bytes)?;
            return Err(SettingsFileError::Isolated {
                quarantined,
                reason: error.to_string(),
            });
        }
        Ok(())
    }

    /// 把当前快照写到 `path`。先写同目录临时文件并 fsync，再改名。
    /// 失败时保留调用方原来的目标文件。
    pub fn save_file(&self, path: &Path) -> Result<(), SettingsFileError> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        let Some(parent) = parent else {
            return Err(SettingsFileError::Io(
                "settings.storage_access_error: missing parent".to_owned(),
            ));
        };
        fs::create_dir_all(parent).map_err(|error| {
            SettingsFileError::Io(format!("settings.storage_access_error: {error}"))
        })?;
        let Some(file_name) = path.file_name() else {
            return Err(SettingsFileError::Io(format!(
                "settings.storage_access_error: {}",
                path.display()
            )));
        };
        let payload = serde_json::to_vec_pretty(&self.current()).map_err(|error| {
            SettingsFileError::Io(format!("settings.storage_format_error: {error}"))
        })?;
        let temporary = unique_temp_path(parent, file_name);
        if let Err(error) = write_synced(&temporary, &payload) {
            let _ = fs::remove_file(&temporary);
            return Err(SettingsFileError::Io(format!(
                "settings.storage_access_error: {error}"
            )));
        }
        if let Err(error) = fs::rename(&temporary, path) {
            let _ = fs::remove_file(&temporary);
            return Err(SettingsFileError::Io(format!(
                "settings.storage_access_error: {error}"
            )));
        }
        sync_parent(parent)?;
        Ok(())
    }
}

/// 桌面/CLI 共用的 Settings 生命周期状态；前端只传入宿主解析后的配置路径。
pub struct SettingsRuntime {
    service: Option<SettingsService>,
    settings_path: PathBuf,
    writable: bool,
    load_warning: Option<String>,
    initialization_error: Option<String>,
}

impl SettingsRuntime {
    pub fn open(settings_path: PathBuf) -> Self {
        let (service, initialization_error) = match SettingsService::builtin() {
            Ok(service) => (Some(service), None),
            Err(error) => (None, Some(error.to_string())),
        };
        let mut runtime = Self {
            service,
            settings_path,
            writable: true,
            load_warning: None,
            initialization_error,
        };
        if let Some(service) = &mut runtime.service {
            if let Err(error) = service.load_file(&runtime.settings_path) {
                match error {
                    SettingsFileError::Isolated { .. } => {
                        runtime.load_warning = Some(error.to_string());
                    }
                    SettingsFileError::Io(_) => {
                        runtime.writable = false;
                        runtime.load_warning = Some(error.to_string());
                    }
                }
            }
        }
        runtime
    }

    pub fn snapshot(&self) -> Result<UserSettings, String> {
        self.service
            .as_ref()
            .map(SettingsService::current)
            .ok_or_else(|| {
                self.initialization_error
                    .clone()
                    .unwrap_or_else(|| "settings.initialize: unavailable".to_owned())
            })
    }

    pub fn theme_catalog_json(&self) -> Result<String, String> {
        self.service
            .as_ref()
            .ok_or_else(|| self.initialization_error.clone().unwrap_or_default())?
            .theme_catalog_json()
            .map_err(|error| error.to_string())
    }

    pub fn load_warning(&self) -> Option<&str> {
        self.load_warning
            .as_deref()
            .or(self.initialization_error.as_deref())
    }

    pub fn set_caret_blink_enabled(&mut self, enabled: bool) -> Result<UserSettings, String> {
        if !self.writable {
            return Err(format!(
                "settings.read_only_after_load_failure: {}",
                self.load_warning.as_deref().unwrap_or_default()
            ));
        }
        let service = self.service.as_mut().ok_or_else(|| {
            self.initialization_error
                .clone()
                .unwrap_or_else(|| "settings.initialize: unavailable".to_owned())
        })?;
        let mut candidate = service.current();
        candidate.preferences.caret_blink_enabled = enabled;
        service
            .commit_and_save(candidate, &self.settings_path)
            .map_err(|error| error.to_string())?;
        self.load_warning = None;
        self.snapshot()
    }
}

/// 设置文件没有被接受。隔离成功时内存快照保持载入前的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsFileError {
    Io(String),
    Isolated {
        quarantined: PathBuf,
        reason: String,
    },
}

impl fmt::Display for SettingsFileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(formatter, "{message}"),
            Self::Isolated {
                quarantined,
                reason,
            } => {
                write!(
                    formatter,
                    "settings.isolated: {}: {reason}",
                    quarantined.display()
                )
            }
        }
    }
}

impl std::error::Error for SettingsFileError {}

/// 设置提交的候选校验错误或持久化错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsUpdateError {
    Validation(SettingsError),
    Persistence(SettingsFileError),
}

impl fmt::Display for SettingsUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(formatter, "{error}"),
            Self::Persistence(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SettingsUpdateError {}

fn quarantine_settings_file(path: &Path, bytes: &[u8]) -> Result<PathBuf, SettingsFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            SettingsFileError::Io("settings.storage_access_error: missing parent".to_owned())
        })?;
    let Some(file_name) = path.file_name() else {
        return Err(SettingsFileError::Io(format!(
            "settings.storage_access_error: {}",
            path.display()
        )));
    };
    for index in 0..1000 {
        let mut suffix = file_name.to_os_string();
        suffix.push(if index == 0 {
            ".isolated".to_owned()
        } else {
            format!(".isolated-{index}")
        });
        let destination = parent.join(suffix);
        match write_synced(&destination, bytes) {
            Ok(()) => match fs::remove_file(path) {
                Ok(()) => return Ok(destination),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(destination);
                }
                Err(error) => {
                    return Err(SettingsFileError::Io(format!(
                        "settings.storage_access_error: {error}"
                    )));
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(SettingsFileError::Io(format!(
                    "settings.storage_access_error: {error}"
                )));
            }
        }
    }
    Err(SettingsFileError::Io(
        "settings.storage_access_error: isolation names exhausted".to_owned(),
    ))
}

fn unique_temp_path(parent: &Path, file_name: &OsStr) -> PathBuf {
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let mut name = OsString::from(".");
    name.push(file_name);
    name.push(format!(".{}.{token}.tmp", std::process::id()));
    parent.join(name)
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn sync_parent(parent: &Path) -> Result<(), SettingsFileError> {
    #[cfg(unix)]
    {
        let file = fs::File::open(parent).map_err(|error| {
            SettingsFileError::Io(format!("settings.storage_access_error: {error}"))
        })?;
        file.sync_all().map_err(|error| {
            SettingsFileError::Io(format!("settings.storage_access_error: {error}"))
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_THEME_ID, SETTINGS_SCHEMA_VERSION, SettingsError, SettingsService, UserSettings,
    };
    use crate::settings::preferences::{
        MAX_RECENT_PROJECTS, MINIMUM_WINDOW_WIDTH, PreferencesError,
    };

    fn service() -> SettingsService {
        SettingsService::builtin().unwrap_or_else(|error| panic!("{error}"))
    }

    fn absolute_path(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn settings_default_composes_theme_language_and_preferences() {
        let service = service();
        let current = service.current();
        assert_eq!(current.theme_id, DEFAULT_THEME_ID);
        assert_eq!(current.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(current.language, "en");
        assert!(current.preferences.window_maximized);
        assert!(current.preferences.caret_blink_enabled);
        assert_eq!(service.validate(current.clone()), Ok(current));
    }

    #[test]
    fn invalid_subservice_candidates_preserve_the_complete_settings_snapshot() {
        let mut service = service();
        let original = service.current();
        let mut invalid_preferences = original.preferences.clone();
        invalid_preferences.window_width = MINIMUM_WINDOW_WIDTH - 1;
        let invalid_candidates = [
            UserSettings {
                schema_version: 99,
                ..original.clone()
            },
            UserSettings {
                theme_id: "bad id".to_owned(),
                ..original.clone()
            },
            UserSettings {
                language: "fr".to_owned(),
                ..original.clone()
            },
            UserSettings {
                preferences: invalid_preferences,
                ..original.clone()
            },
        ];
        for candidate in invalid_candidates {
            assert!(service.commit(candidate).is_err());
            assert_eq!(service.current(), original);
        }
    }

    #[test]
    fn recent_projects_are_deduplicated_ordered_and_bounded() -> Result<(), PreferencesError> {
        let paths: Vec<_> = (0..MAX_RECENT_PROJECTS)
            .map(|index| absolute_path(&format!("{index}.cdoc")))
            .collect();
        let mut service_runtime = service();
        let mut candidate = service_runtime.current();
        candidate.preferences.recent_project_paths = paths.clone();
        service_runtime
            .commit(candidate)
            .map_err(|error| match error {
                SettingsError::InvalidPreferences(error) => error,
                _ => PreferencesError::InvalidRecentProjectPath("unexpected settings error".into()),
            })?;

        let promoted = service_runtime
            .candidate_recent_project(&absolute_path("new.cdoc"))
            .map_err(|error| match error {
                SettingsError::InvalidPreferences(error) => error,
                _ => PreferencesError::InvalidRecentProjectPath("unexpected settings error".into()),
            })?;
        assert_eq!(
            promoted.preferences.recent_project_paths.len(),
            MAX_RECENT_PROJECTS
        );
        assert_eq!(
            promoted.preferences.recent_project_paths[0],
            absolute_path("new.cdoc")
        );
        assert!(
            !promoted
                .preferences
                .recent_project_paths
                .contains(&paths[MAX_RECENT_PROJECTS - 1])
        );
        Ok(())
    }

    struct TempDir {
        root: std::path::PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> std::io::Result<Self> {
            let base = std::fs::canonicalize(std::env::temp_dir())
                .unwrap_or_else(|_| std::env::temp_dir());
            let root = base.join(format!("caret-settings-{}-{label}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root)?;
            Ok(Self { root })
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn settings_file_round_trips_all_services() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("roundtrip")?;
        let path = dir.root.join("settings.json");
        let mut service_runtime = service();
        let mut candidate = service_runtime.current();
        candidate.language = "zh-CN".to_owned();
        candidate.theme_id = "dark".to_owned();
        candidate.preferences.window_maximized = false;
        candidate.preferences.caret_blink_enabled = false;
        service_runtime.commit_and_save(candidate.clone(), &path)?;

        let mut loaded = service();
        loaded.load_file(&path)?;
        assert_eq!(loaded.current(), candidate);
        let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
        assert!(saved.get("preferences").is_none());
        assert_eq!(saved["caretBlinkEnabled"], false);
        Ok(())
    }

    #[test]
    fn old_flat_settings_without_blink_flag_use_the_default()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("legacy-caret-blink")?;
        let path = dir.root.join("settings.json");
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"themeId":"light","language":"en","windowWidth":1280,"windowHeight":720,"windowMaximized":true,"recentProjectPaths":[]}"#,
        )?;
        let mut service = service();
        service.load_file(&path)?;
        assert!(service.current().preferences.caret_blink_enabled);
        Ok(())
    }

    #[test]
    fn failed_persistence_does_not_publish_the_candidate() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = TempDir::new("commit-failure")?;
        let mut service = service();
        let original = service.current();
        let mut candidate = original.clone();
        candidate.preferences.caret_blink_enabled = false;

        assert!(service.commit_and_save(candidate, &dir.root).is_err());
        assert_eq!(service.current(), original);
        Ok(())
    }

    #[test]
    fn unknown_fields_are_rejected_and_quarantined() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("unknown-field")?;
        let path = dir.root.join("settings.json");
        std::fs::write(
            &path,
            r#"{"schemaVersion":1,"themeId":"light","language":"en","windowWidth":1280,"windowHeight":720,"windowMaximized":true,"recentProjectPaths":[],"futureSetting":true}"#,
        )?;
        let mut service = service();
        let original = service.current();
        assert!(matches!(
            service.load_file(&path),
            Err(super::SettingsFileError::Isolated { .. })
        ));
        assert_eq!(service.current(), original);
        assert!(dir.root.join("settings.json.isolated").exists());
        Ok(())
    }
}

//! 版本化的用户偏好快照。校验在内存中完成。文件往返由调用方传入路径，
//! 不发现平台目录，也不依赖 Qt。

use crate::language::LanguageService;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const SETTINGS_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_THEME_ID: &str = "light";
const SUPPORTED_THEME_IDS: [&str; 2] = ["light", "dark"];
pub const DEFAULT_WINDOW_WIDTH: u32 = 1280;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 720;
pub const MAX_WINDOW_DIMENSION: u32 = 16_384;
pub const MAX_RECENT_PROJECTS: usize = 10;
const MAX_PATH_BYTES: usize = 32 * 1024;

/// 可以在设置文件和以后的界面边界之间传递的完整用户偏好快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserSettings {
    pub schema_version: u32,
    pub theme_id: String,
    pub language: String,
    pub window_width: u32,
    pub window_height: u32,
    pub window_maximized: bool,
    pub recent_project_paths: Vec<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            theme_id: DEFAULT_THEME_ID.to_owned(),
            language: LanguageService::default().current().as_str().to_owned(),
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
            window_maximized: true,
            recent_project_paths: Vec::new(),
        }
    }
}

/// 偏好快照中不符合当前 schema 的字段。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsError {
    UnsupportedVersion(u32),
    InvalidThemeId(String),
    InvalidLanguage(String),
    InvalidWindowWidth(u32),
    InvalidWindowHeight(u32),
    TooManyRecentProjects(usize),
    InvalidRecentProjectPath(String),
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
            Self::InvalidWindowWidth(width) => {
                write!(formatter, "settings.invalid_window_width: {width}")
            }
            Self::InvalidWindowHeight(height) => {
                write!(formatter, "settings.invalid_window_height: {height}")
            }
            Self::TooManyRecentProjects(count) => {
                write!(formatter, "settings.too_many_recent_projects: {count}")
            }
            Self::InvalidRecentProjectPath(path) => {
                write!(formatter, "settings.invalid_recent_project_path: {path}")
            }
        }
    }
}

impl std::error::Error for SettingsError {}

/// 先验证完整候选，再由 Qt adapter 持久化并提交，避免失败写入改变当前状态。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsService {
    current: UserSettings,
    language: LanguageService,
}

impl SettingsService {
    pub fn current(&self) -> &UserSettings {
        &self.current
    }

    pub fn validate(&self, candidate: UserSettings) -> Result<UserSettings, SettingsError> {
        validate_snapshot(candidate, &self.language)
    }

    pub fn commit(&mut self, candidate: UserSettings) -> Result<&UserSettings, SettingsError> {
        let candidate = self.validate(candidate)?;
        let locale = self
            .language
            .validate(&candidate.language)
            .map_err(|_| SettingsError::InvalidLanguage(candidate.language.clone()))?;
        self.current = candidate;
        self.language.commit(locale);
        Ok(&self.current)
    }

    pub fn candidate_recent_project(&self, path: &str) -> Result<UserSettings, SettingsError> {
        let mut candidate = self.current.clone();
        validate_recent_project_path(path)?;
        candidate
            .recent_project_paths
            .retain(|existing| existing != path);
        candidate.recent_project_paths.insert(0, path.to_owned());
        candidate.recent_project_paths.truncate(MAX_RECENT_PROJECTS);
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
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(SettingsFileError::Io(format!(
                "settings.storage_access_error: {}",
                path.display()
            )));
        };
        let payload = serde_json::to_vec_pretty(&self.current).map_err(|error| {
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

fn quarantine_settings_file(path: &Path, bytes: &[u8]) -> Result<PathBuf, SettingsFileError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            SettingsFileError::Io("settings.storage_access_error: missing parent".to_owned())
        })?;
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Err(SettingsFileError::Io(format!(
            "settings.storage_access_error: {}",
            path.display()
        )));
    };
    for index in 0..1000 {
        let suffix = if index == 0 {
            format!("{file_name}.isolated")
        } else {
            format!("{file_name}.isolated-{index}")
        };
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

fn unique_temp_path(parent: &Path, file_name: &str) -> PathBuf {
    let token = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    parent.join(format!(".{file_name}.{}.{token}.tmp", std::process::id()))
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

fn validate_snapshot(
    candidate: UserSettings,
    language: &LanguageService,
) -> Result<UserSettings, SettingsError> {
    if candidate.schema_version != SETTINGS_SCHEMA_VERSION {
        return Err(SettingsError::UnsupportedVersion(candidate.schema_version));
    }
    if !SUPPORTED_THEME_IDS.contains(&candidate.theme_id.as_str()) {
        return Err(SettingsError::InvalidThemeId(candidate.theme_id));
    }
    if language.validate(&candidate.language).is_err() {
        return Err(SettingsError::InvalidLanguage(candidate.language));
    }
    if !(DEFAULT_WINDOW_WIDTH..=MAX_WINDOW_DIMENSION).contains(&candidate.window_width) {
        return Err(SettingsError::InvalidWindowWidth(candidate.window_width));
    }
    if !(DEFAULT_WINDOW_HEIGHT..=MAX_WINDOW_DIMENSION).contains(&candidate.window_height) {
        return Err(SettingsError::InvalidWindowHeight(candidate.window_height));
    }
    if candidate.recent_project_paths.len() > MAX_RECENT_PROJECTS {
        return Err(SettingsError::TooManyRecentProjects(
            candidate.recent_project_paths.len(),
        ));
    }
    for (index, path) in candidate.recent_project_paths.iter().enumerate() {
        validate_recent_project_path(path)?;
        if candidate.recent_project_paths[..index].contains(path) {
            return Err(SettingsError::InvalidRecentProjectPath(path.clone()));
        }
    }
    Ok(candidate)
}

fn validate_recent_project_path(value: &str) -> Result<(), SettingsError> {
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.contains('\0')
        || !Path::new(value).is_absolute()
    {
        return Err(SettingsError::InvalidRecentProjectPath(value.to_owned()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_THEME_ID, MAX_RECENT_PROJECTS, MAX_WINDOW_DIMENSION, SETTINGS_SCHEMA_VERSION,
        SettingsError, SettingsService, UserSettings,
    };
    fn absolute_path(name: &str) -> String {
        std::env::temp_dir()
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn defaults_are_valid_and_include_supported_locale() {
        let service = SettingsService::default();
        assert_eq!(service.current().theme_id, DEFAULT_THEME_ID);
        assert_eq!(service.current().schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(service.current().language, "en");
        assert!(service.current().window_maximized);
        assert!(service.current().recent_project_paths.is_empty());
        assert_eq!(
            service.validate(service.current().clone()),
            Ok(service.current().clone())
        );
    }

    #[test]
    fn invalid_loaded_snapshots_are_rejected_without_replacing_current_state() {
        let mut service = SettingsService::default();
        let original = service.current().clone();
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
                language: "zh_CN".to_owned(),
                ..original.clone()
            },
            UserSettings {
                window_width: 1279,
                ..original.clone()
            },
            UserSettings {
                window_height: MAX_WINDOW_DIMENSION + 1,
                ..original.clone()
            },
            UserSettings {
                recent_project_paths: vec!["relative.txt".to_owned()],
                ..original.clone()
            },
        ];

        for candidate in invalid_candidates {
            assert!(service.commit(candidate).is_err());
            assert_eq!(service.current(), &original);
        }
    }

    #[test]
    fn validated_preferences_commit_as_one_snapshot() -> Result<(), SettingsError> {
        let mut service = SettingsService::default();
        let mut candidate = service.current().clone();
        candidate.language = "zh-CN".to_owned();
        candidate.theme_id = "dark".to_owned();
        candidate.window_width = 1440;
        candidate.window_height = 900;
        candidate.window_maximized = false;
        candidate
            .recent_project_paths
            .push(absolute_path("one.txt"));
        service.commit(candidate.clone())?;
        assert_eq!(service.current(), &candidate);
        let mut invalid = candidate.clone();
        invalid.language = "fr".to_owned();
        assert_eq!(
            service.validate(invalid),
            Err(SettingsError::InvalidLanguage("fr".into()))
        );
        assert_eq!(service.current(), &candidate);
        Ok(())
    }

    #[test]
    fn recent_projects_are_deduplicated_ordered_and_bounded() -> Result<(), SettingsError> {
        let service = SettingsService::default();
        let paths: Vec<_> = (0..MAX_RECENT_PROJECTS)
            .map(|index| absolute_path(&format!("{index}.txt")))
            .collect();
        let mut candidate = service.current().clone();
        candidate.recent_project_paths = paths.clone();
        let mut service = SettingsService::default();
        service.commit(candidate)?;

        let new_path = absolute_path("new.txt");
        let promoted = service.candidate_recent_project(&new_path)?;
        assert_eq!(promoted.recent_project_paths.len(), MAX_RECENT_PROJECTS);
        assert_eq!(promoted.recent_project_paths[0], new_path);
        assert!(!promoted.recent_project_paths.contains(&paths[9]));
        assert_eq!(
            service.candidate_recent_project("relative.txt"),
            Err(SettingsError::InvalidRecentProjectPath(
                "relative.txt".into()
            ))
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
    fn settings_file_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("roundtrip")?;
        let path = dir.root.join("settings.json");
        let mut service = SettingsService::default();
        let mut candidate = service.current().clone();
        candidate.language = "zh-CN".to_owned();
        candidate.theme_id = "dark".to_owned();
        candidate.window_maximized = false;
        candidate
            .recent_project_paths
            .push(absolute_path("note.txt"));
        service.commit(candidate.clone())?;
        service.save_file(&path)?;

        let mut loaded = SettingsService::default();
        loaded.load_file(&path)?;
        assert_eq!(loaded.current(), &candidate);
        Ok(())
    }

    #[test]
    fn missing_settings_file_keeps_the_current_snapshot() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = TempDir::new("missing")?;
        let mut service = SettingsService::default();
        let original = service.current().clone();
        service.load_file(&dir.root.join("settings.json"))?;
        assert_eq!(service.current(), &original);
        assert!(!dir.root.join("settings.json").exists());
        Ok(())
    }

    #[test]
    fn corrupt_settings_file_is_isolated_without_replacing_memory()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("corrupt")?;
        let path = dir.root.join("settings.json");
        let corrupt = b"{not-json";
        std::fs::write(&path, corrupt)?;
        let mut service = SettingsService::default();
        let mut candidate = service.current().clone();
        candidate.theme_id = "dark".to_owned();
        candidate.language = "zh-CN".to_owned();
        service.commit(candidate.clone())?;

        assert!(matches!(
            service.load_file(&path),
            Err(super::SettingsFileError::Isolated { .. })
        ));
        assert_eq!(service.current(), &candidate);
        assert!(!path.exists());
        let isolated = dir.root.join("settings.json.isolated");
        assert_eq!(std::fs::read(&isolated)?, corrupt);

        service.save_file(&path)?;
        assert_eq!(std::fs::read(&isolated)?, corrupt);
        let mut restored = SettingsService::default();
        restored.load_file(&path)?;
        assert_eq!(restored.current(), &candidate);
        Ok(())
    }

    #[test]
    fn invalid_snapshot_is_isolated_and_keeps_the_previous_snapshot()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = TempDir::new("invalid")?;
        let path = dir.root.join("settings.json");
        let mut service = SettingsService::default();
        service.save_file(&path)?;
        let original = service.current().clone();
        let mut raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
        raw["themeId"] = serde_json::Value::String("nope".to_owned());
        let damaged = serde_json::to_vec_pretty(&raw)?;
        std::fs::write(&path, &damaged)?;

        assert!(matches!(
            service.load_file(&path),
            Err(super::SettingsFileError::Isolated { .. })
        ));
        assert_eq!(service.current(), &original);
        assert_eq!(
            std::fs::read(dir.root.join("settings.json.isolated"))?,
            damaged
        );
        Ok(())
    }
}

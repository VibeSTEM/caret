//! Settings 聚合、磁盘兼容与失败原子性的行为测试。

use super::{
    DEFAULT_THEME_ID, SETTINGS_SCHEMA_VERSION, SettingsError, SettingsService, UserSettings,
};
use crate::settings::preferences::{MAX_RECENT_PROJECTS, MINIMUM_WINDOW_WIDTH, PreferencesError};

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
    assert_eq!(current, UserSettings::default());
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
        let base =
            std::fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
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
fn old_flat_settings_without_blink_flag_use_the_default() -> Result<(), Box<dyn std::error::Error>>
{
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
fn failed_persistence_does_not_publish_the_candidate() -> Result<(), Box<dyn std::error::Error>> {
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

#[test]
fn invalid_settings_isolation_keeps_existing_quarantines() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = TempDir::new("isolation-collision")?;
    let path = dir.root.join("settings.json");
    let existing = dir.root.join("settings.json.isolated");
    std::fs::write(&existing, b"previous invalid settings")?;
    std::fs::write(&path, b"new invalid settings")?;
    let mut service = service();
    let before = service.current();

    let Err(error) = service.load_file(&path) else {
        panic!("invalid file should not be accepted");
    };
    let super::SettingsFileError::Isolated { quarantined, .. } = &error else {
        panic!("invalid file should have been isolated: {error}");
    };
    assert_eq!(quarantined, &dir.root.join("settings.json.isolated-1"));
    assert_eq!(std::fs::read(existing)?, b"previous invalid settings");
    assert_eq!(std::fs::read(quarantined)?, b"new invalid settings");
    assert!(!path.exists());
    assert_eq!(service.current(), before);
    assert!(error.to_string().starts_with("settings.isolated:"));
    Ok(())
}

#[test]
fn invalid_candidate_is_rejected_before_creating_a_settings_file()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("invalid-candidate")?;
    let path = dir.root.join("settings.json");
    let mut service = service();
    let before = service.current();
    let candidate = UserSettings {
        schema_version: SETTINGS_SCHEMA_VERSION + 1,
        ..before.clone()
    };

    let Err(error) = service.commit_and_save(candidate, &path) else {
        panic!("invalid candidate should not be saved");
    };
    assert!(matches!(error, super::SettingsUpdateError::Validation(_)));
    assert!(
        error
            .to_string()
            .starts_with("settings.unsupported_version:")
    );
    assert_eq!(service.current(), before);
    assert!(!path.exists());
    Ok(())
}

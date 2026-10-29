//! Settings 的显式可用状态、加载恢复与用户意图提交，不依赖 Qt。

use super::{SettingsFileError, SettingsService, UserSettings};
use std::path::PathBuf;

/// 可供界面或命令行展示的设置能力，不包含服务和配置路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsStatus {
    Ready,
    ReadOnly,
    Unavailable,
}

enum SettingsState {
    Ready {
        service: SettingsService,
        load_warning: Option<String>,
    },
    ReadOnly {
        service: SettingsService,
        diagnostic: String,
    },
    Unavailable {
        diagnostic: String,
    },
}

/// 桌面和 CLI 共用的设置运行时；宿主仅提供平台解析后的配置路径。
pub struct SettingsRuntime {
    state: SettingsState,
    settings_path: PathBuf,
    persistence_warning: Option<String>,
}

impl SettingsRuntime {
    /// 已隔离的无效文件允许使用默认值继续写入；访问失败保留只读有效快照。
    pub fn open(settings_path: PathBuf) -> Self {
        let state = match SettingsService::builtin() {
            Ok(mut service) => match service.load_file(&settings_path) {
                Ok(()) => SettingsState::Ready {
                    service,
                    load_warning: None,
                },
                Err(error @ SettingsFileError::Isolated { .. }) => SettingsState::Ready {
                    service,
                    load_warning: Some(error.to_string()),
                },
                Err(error @ SettingsFileError::Io(_)) => SettingsState::ReadOnly {
                    service,
                    diagnostic: error.to_string(),
                },
            },
            Err(error) => SettingsState::Unavailable {
                diagnostic: error.to_string(),
            },
        };
        Self {
            state,
            settings_path,
            persistence_warning: None,
        }
    }

    pub fn status(&self) -> SettingsStatus {
        match self.state {
            SettingsState::Ready { .. } => SettingsStatus::Ready,
            SettingsState::ReadOnly { .. } => SettingsStatus::ReadOnly,
            SettingsState::Unavailable { .. } => SettingsStatus::Unavailable,
        }
    }

    pub fn snapshot(&self) -> Result<UserSettings, String> {
        self.service().map(SettingsService::current)
    }

    pub fn theme_catalog_json(&self) -> Result<String, String> {
        self.service()?
            .theme_catalog_json()
            .map_err(|error| error.to_string())
    }

    pub fn language_catalog_json(&self) -> Result<String, String> {
        super::language::LanguageService::catalog_json().map_err(|error| error.to_string())
    }

    /// 只报告初始化/加载诊断；保存后的崩溃持久性警告单独读取。
    pub fn load_warning(&self) -> Option<&str> {
        match &self.state {
            SettingsState::Ready { load_warning, .. } => load_warning.as_deref(),
            SettingsState::ReadOnly { diagnostic, .. }
            | SettingsState::Unavailable { diagnostic } => Some(diagnostic),
        }
    }

    pub fn persistence_warning(&self) -> Option<&str> {
        self.persistence_warning.as_deref()
    }

    /// 同步提交单项意图；失败时保留有效快照，已替换目标的保存仍视为成功。
    pub fn set_caret_blink_enabled(&mut self, enabled: bool) -> Result<UserSettings, String> {
        match &mut self.state {
            SettingsState::Ready {
                service,
                load_warning,
            } => {
                let mut candidate = service.current();
                candidate.preferences.caret_blink_enabled = enabled;
                let outcome = service
                    .commit_and_save(candidate, &self.settings_path)
                    .map_err(|error| error.to_string())?;
                self.persistence_warning = outcome.durability_warning;
                *load_warning = None;
                Ok(service.current())
            }
            SettingsState::ReadOnly { diagnostic, .. } => Err(format!(
                "settings.read_only_after_load_failure: {diagnostic}"
            )),
            SettingsState::Unavailable { diagnostic } => Err(diagnostic.clone()),
        }
    }

    /// 持久化 AutoSave 全局开关；失败时保留最后有效设置快照。
    pub fn set_auto_save_enabled(&mut self, enabled: bool) -> Result<UserSettings, String> {
        match &mut self.state {
            SettingsState::Ready {
                service,
                load_warning,
            } => {
                let mut candidate = service.current();
                candidate.preferences.auto_save_enabled = enabled;
                let outcome = service
                    .commit_and_save(candidate, &self.settings_path)
                    .map_err(|error| error.to_string())?;
                self.persistence_warning = outcome.durability_warning;
                *load_warning = None;
                Ok(service.current())
            }
            SettingsState::ReadOnly { diagnostic, .. } => Err(format!(
                "settings.read_only_after_load_failure: {diagnostic}"
            )),
            SettingsState::Unavailable { diagnostic } => Err(diagnostic.clone()),
        }
    }

    /// 原子提交主题与界面语言选择；持久化失败时保留原设置快照。
    pub fn set_ui_selection(
        &mut self,
        theme_id: &str,
        language: &str,
        caret_blink_enabled: bool,
        command_suggestions_enabled: bool,
    ) -> Result<UserSettings, String> {
        match &mut self.state {
            SettingsState::Ready {
                service,
                load_warning,
            } => {
                let mut candidate = service.current();
                candidate.theme_id = theme_id.to_owned();
                candidate.language = language.to_owned();
                candidate.preferences.caret_blink_enabled = caret_blink_enabled;
                candidate.preferences.command_suggestions_enabled = command_suggestions_enabled;
                let outcome = service
                    .commit_and_save(candidate, &self.settings_path)
                    .map_err(|error| error.to_string())?;
                self.persistence_warning = outcome.durability_warning;
                *load_warning = None;
                Ok(service.current())
            }
            SettingsState::ReadOnly { diagnostic, .. } => Err(format!(
                "settings.read_only_after_load_failure: {diagnostic}"
            )),
            SettingsState::Unavailable { diagnostic } => Err(diagnostic.clone()),
        }
    }

    fn service(&self) -> Result<&SettingsService, String> {
        match &self.state {
            SettingsState::Ready { service, .. } | SettingsState::ReadOnly { service, .. } => {
                Ok(service)
            }
            SettingsState::Unavailable { diagnostic } => Err(diagnostic.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SettingsRuntime, SettingsState, SettingsStatus};
    use std::fs;

    #[test]
    fn isolated_settings_remain_writable_and_publish_a_saved_preference()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "caret-settings-runtime-isolated-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        let path = directory.join("settings.json");
        fs::write(&path, b"invalid settings")?;
        let mut runtime = SettingsRuntime::open(path.clone());
        assert_eq!(runtime.status(), SettingsStatus::Ready);
        assert!(runtime.load_warning().is_some());
        assert!(directory.join("settings.json.isolated").exists());

        let snapshot = runtime.set_caret_blink_enabled(false)?;
        assert!(!snapshot.preferences.caret_blink_enabled);
        assert!(runtime.load_warning().is_none());
        let snapshot = runtime.set_auto_save_enabled(true)?;
        assert!(snapshot.preferences.auto_save_enabled);
        let reopened = SettingsRuntime::open(path);
        assert!(!reopened.snapshot()?.preferences.caret_blink_enabled);
        assert!(reopened.snapshot()?.preferences.auto_save_enabled);
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn inaccessible_configuration_preserves_defaults_and_rejects_updates()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "caret-settings-runtime-readonly-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        // 目录不能作为配置文件读取，此错误不能通过覆盖目标来“恢复”。
        let mut runtime = SettingsRuntime::open(directory.clone());
        assert_eq!(runtime.status(), SettingsStatus::ReadOnly);
        let original = runtime.snapshot()?;
        assert!(runtime.set_caret_blink_enabled(false).is_err());
        assert_eq!(runtime.snapshot()?, original);
        assert!(directory.is_dir());
        fs::remove_dir_all(directory)?;
        Ok(())
    }

    #[test]
    fn unavailable_runtime_reports_initialization_failure_for_all_operations() {
        let diagnostic = "settings.test_initialization_failed";
        let mut runtime = SettingsRuntime {
            state: SettingsState::Unavailable {
                diagnostic: diagnostic.to_owned(),
            },
            settings_path: std::env::temp_dir().join("caret-unavailable-settings.json"),
            persistence_warning: None,
        };

        assert_eq!(runtime.status(), SettingsStatus::Unavailable);
        assert_eq!(runtime.snapshot(), Err(diagnostic.to_owned()));
        assert_eq!(runtime.theme_catalog_json(), Err(diagnostic.to_owned()));
        assert_eq!(runtime.load_warning(), Some(diagnostic));
        assert_eq!(runtime.persistence_warning(), None);
        assert_eq!(
            runtime.set_caret_blink_enabled(false),
            Err(diagnostic.to_owned())
        );
    }

    #[test]
    fn ui_selection_commits_theme_and_language_together_and_persists()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = std::env::temp_dir().join(format!(
            "caret-settings-ui-selection-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory)?;
        let path = directory.join("settings.json");
        let mut runtime = SettingsRuntime::open(path.clone());

        let selected = runtime.set_ui_selection("dark", "ja", false, false)?;
        assert_eq!(selected.theme_id, "dark");
        assert_eq!(selected.language, "ja");
        assert!(!selected.preferences.caret_blink_enabled);
        let reopened = SettingsRuntime::open(path);
        assert_eq!(reopened.snapshot()?.theme_id, "dark");
        assert_eq!(reopened.snapshot()?.language, "ja");

        let original = runtime.snapshot()?;
        assert!(
            runtime
                .set_ui_selection("missing", "zh-Hant", true, true)
                .is_err()
        );
        assert_eq!(runtime.snapshot()?, original);
        fs::remove_dir_all(directory)?;
        Ok(())
    }
}

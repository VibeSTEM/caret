//! Settings 总入口，组合偏好、主题、语言与持久化运行时。

pub mod language;
mod model;
pub mod preferences;
mod runtime;
mod service;
mod storage;
pub mod theme;

pub use model::{SETTINGS_SCHEMA_VERSION, SettingsError, UserSettings};
pub use runtime::{SettingsRuntime, SettingsStatus};
pub use service::SettingsService;
pub use storage::{SettingsFileError, SettingsSaveOutcome, SettingsUpdateError};
pub use theme::DEFAULT_THEME_ID;

#[cfg(test)]
mod tests;

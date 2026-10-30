//! Settings 运行时句柄、偏好快照与设置意图的边界转换。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::generated::bridge;

/// Settings 组合服务及其宿主注入的持久化路径。
pub struct SettingsRuntime {
    runtime: caret_app::settings::SettingsRuntime,
}

/// 成功时是 JSON。失败时以 `error:` 开头，调用方不要把它当成快照。
pub(crate) fn builtin_theme_catalog() -> String {
    match caret_app::settings::theme::builtin_theme_catalog() {
        Ok(json) => json,
        Err(error) => format!("error:{error}"),
    }
}

pub(crate) fn new_settings_runtime(settings_path: String) -> Box<SettingsRuntime> {
    Box::new(SettingsRuntime {
        runtime: caret_app::settings::SettingsRuntime::open(std::path::PathBuf::from(
            settings_path,
        )),
    })
}

pub(crate) fn settings_snapshot(runtime: &mut SettingsRuntime) -> bridge::SettingsSnapshot {
    match runtime.runtime.snapshot() {
        Ok(snapshot) => bridge::SettingsSnapshot {
            caret_blink_enabled: snapshot.preferences.caret_blink_enabled,
            command_suggestions_enabled: snapshot.preferences.command_suggestions_enabled,
            auto_save_enabled: snapshot.preferences.auto_save_enabled,
            window_width: snapshot.preferences.window_width,
            window_height: snapshot.preferences.window_height,
            theme_id: snapshot.theme_id,
            language: snapshot.language,
            error: String::new(),
            warning: runtime
                .runtime
                .persistence_warning()
                .unwrap_or_default()
                .to_owned(),
        },
        Err(error) => bridge::SettingsSnapshot {
            caret_blink_enabled: true,
            command_suggestions_enabled: true,
            auto_save_enabled: false,
            window_width: 0,
            window_height: 0,
            theme_id: String::new(),
            language: String::new(),
            error: format!("settings.initialize: {error}"),
            warning: String::new(),
        },
    }
}

pub(crate) fn settings_theme_catalog(runtime: &mut SettingsRuntime) -> String {
    match runtime.runtime.theme_catalog_json() {
        Ok(json) => json,
        Err(error) => format!("error:{error}"),
    }
}

pub(crate) fn settings_language_catalog(runtime: &mut SettingsRuntime) -> String {
    runtime
        .runtime
        .language_catalog_json()
        .unwrap_or_else(|error| format!("error:{error}"))
}

pub(crate) fn settings_constraints() -> bridge::SettingsConstraints {
    bridge::SettingsConstraints {
        minimum_window_width: caret_app::settings::preferences::MINIMUM_WINDOW_WIDTH,
        minimum_window_height: caret_app::settings::preferences::MINIMUM_WINDOW_HEIGHT,
    }
}

pub(crate) fn settings_load_warning(runtime: &mut SettingsRuntime) -> String {
    runtime
        .runtime
        .load_warning()
        .unwrap_or_default()
        .to_owned()
}

pub(crate) fn set_caret_blink_enabled(
    runtime: &mut SettingsRuntime,
    enabled: bool,
) -> bridge::SettingsSnapshot {
    if let Err(error) = runtime.runtime.set_caret_blink_enabled(enabled) {
        let mut snapshot = settings_snapshot(runtime);
        snapshot.error = error;
        return snapshot;
    }
    settings_snapshot(runtime)
}

pub(crate) fn set_auto_save_enabled(
    runtime: &mut SettingsRuntime,
    enabled: bool,
) -> bridge::SettingsSnapshot {
    if let Err(error) = runtime.runtime.set_auto_save_enabled(enabled) {
        let mut snapshot = settings_snapshot(runtime);
        snapshot.error = error;
        return snapshot;
    }
    settings_snapshot(runtime)
}

pub(crate) fn set_ui_selection(
    runtime: &mut SettingsRuntime,
    theme_id: String,
    language: String,
    caret_blink_enabled: bool,
    command_suggestions_enabled: bool,
) -> bridge::SettingsSnapshot {
    if let Err(error) = runtime.runtime.set_ui_selection(
        &theme_id,
        &language,
        caret_blink_enabled,
        command_suggestions_enabled,
    ) {
        let mut snapshot = settings_snapshot(runtime);
        snapshot.error = error;
        return snapshot;
    }
    settings_snapshot(runtime)
}

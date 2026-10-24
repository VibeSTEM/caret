//! 锁定 Qt 6.11.2 与 Linux ICU。归档表在 `native/cmake/qt-provision.cmake`，
//! 安装根是 `.toolchain/qt`，不写入 `target/`。

use std::path::{Path, PathBuf};
use std::process::Command;

/// 供给 Qt 并返回 `staging` 目录。已校验的安装树会直接复用。
pub fn ensure_qt() -> Result<PathBuf, String> {
    let toolchain = crate::toolchain_root()?;
    let provision = toolchain.join("qt");
    let staging = provision.join("staging");
    let script =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../native/cmake/qt-provision.cmake");
    if !script.is_file() {
        return Err(format!(
            "Qt 供给脚本不存在：{}；见 ai-docs/standards/dependency-acquisition.md",
            script.display()
        ));
    }
    let cmake = crate::resolve_cmake()?;
    let mut command = Command::new(&cmake);
    command
        .arg(format!("-DQT_PROVISION_DIR={}", provision.display()))
        .arg("-P")
        .arg(&script);
    let status = command
        .status()
        .map_err(|error| format!("无法启动 Qt 供给：{error}"))?;
    if !status.success() {
        return Err(match status.code() {
            Some(code) => format!("Qt 供给失败（退出码 {code}）"),
            None => "Qt 供给被信号终止".to_owned(),
        });
    }
    let marker = staging.join("lib/cmake/Qt6/Qt6Config.cmake");
    if !marker.is_file() {
        return Err(format!("Qt 供给后缺少 {}", marker.display()));
    }
    Ok(staging)
}

/// 供测试确认供给目录落在 toolchain 根下，而不是 `target/`。
pub fn qt_provision_dir(toolchain: &Path) -> PathBuf {
    toolchain.join("qt")
}

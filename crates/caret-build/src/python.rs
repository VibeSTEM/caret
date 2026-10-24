//! Python 仅服务开发工具；解释器、环境和缓存都位于 `.toolchain/tools/python`。
use std::path::Path;
use std::process::Command;

use crate::assets::Tool;
use crate::install::ensure_tool;
use crate::toolchain_root;

pub use crate::assets::UV_VERSION;

pub const PYTHON_VERSION: &str = "3.14.7";

/// uv 自身按 SHA256 安装；其固定版本携带 Python 下载清单及校验和，禁止选择系统 Python。
pub fn command(project: &Path, tool: &str) -> Result<Command, String> {
    let uv = ensure_tool(Tool::Uv, None)?;
    let root = toolchain_root()?.join("tools").join("python");
    let mut command = Command::new(uv);
    command
        .current_dir(project)
        .env("UV_CACHE_DIR", root.join("cache"))
        .env("UV_PYTHON_INSTALL_DIR", root.join("interpreters"))
        .env("UV_PYTHON_BIN_DIR", root.join("bin"))
        .env("UV_PROJECT_ENVIRONMENT", root.join("venv"))
        .env("UV_TOOL_DIR", root.join("tools"))
        .args([
            "run",
            "--locked",
            "--managed-python",
            "--python",
            PYTHON_VERSION,
            "--no-build",
            "--project",
        ])
        .arg(project)
        .arg(tool);
    Ok(command)
}

//! 托管构建支持公共门面：固定资产、安装事务、编译器与平台环境各归其模块。
//! 下载只在消费者请求工具路径时发生；Cargo 与 CMake 共用已校验的安装树。

use std::path::PathBuf;

mod archive;
mod assets;
mod atomic_directory;
mod compiler;
mod environment;
mod install;
mod paths;

pub mod database;
pub mod python;

pub use assets::{HYPERFINE_VERSION, LLVM_VERSION};
pub use atomic_directory::replace_directory_with_files;
pub use compiler::{
    LlvmCompilers, compiler_rt_dll_dir, resolve_cmake, resolve_llvm_compilers, resolve_ninja,
};
pub use environment::{
    MACOS_DEPLOYMENT_TARGET, macos_sdk, native_test_env, prepend_path, target_root,
    use_system_tools, windows_sdk_env,
};
pub use install::{install_directory, install_lock};
pub use paths::{exe_name, toolchain_root};

/// 解析按需安装并经 SHA256 校验的 hyperfine 命令。安装根是 `.toolchain/`。
pub fn resolve_hyperfine() -> Result<PathBuf, String> {
    install::ensure_tool(assets::Tool::Hyperfine, None)
}

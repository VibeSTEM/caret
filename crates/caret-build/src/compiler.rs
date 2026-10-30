//! 构建与质量工具的入口定位；托管安装和显式系统旁路共用公开契约。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::assets::{LLVM_VERSION, Tool};
use crate::environment::use_system_tools;
use crate::install::ensure_tool;
use crate::paths::{find_binary, find_on_path, set_executable};

/// 两条 C++ 构建链必须消费同一份 LLVM 目录中的工具。
#[derive(Clone, Debug)]
pub struct LlvmCompilers {
    pub root: PathBuf,
    pub clang: PathBuf,
    pub clangxx: PathBuf,
    pub clang_cl: Option<PathBuf>,
    pub clang_format: PathBuf,
}

/// 解析 Cargo CXX 与 CMake 共用的固定 LLVM 工具链。
pub fn resolve_llvm_compilers() -> Result<LlvmCompilers, String> {
    if use_system_tools() {
        return resolve_system_llvm();
    }

    let primary = ensure_tool(Tool::Llvm, None)?;
    let bin_dir = primary
        .parent()
        .ok_or_else(|| format!("LLVM 编译器路径没有父目录：{}", primary.display()))?;
    let root = bin_dir
        .parent()
        .ok_or_else(|| format!("LLVM bin 目录没有安装根：{}", bin_dir.display()))?
        .to_path_buf();
    let clang = find_binary(&root, "clang")
        .ok_or_else(|| format!("LLVM {} 缺少 clang：{}", LLVM_VERSION, root.display()))?;
    let clangxx = find_binary(&root, "clang++")
        .ok_or_else(|| format!("LLVM {} 缺少 clang++：{}", LLVM_VERSION, root.display()))?;
    let clang_format = find_binary(&root, "clang-format").ok_or_else(|| {
        format!(
            "LLVM {} 缺少 clang-format：{}",
            LLVM_VERSION,
            root.display()
        )
    })?;
    let clang_cl = find_binary(&root, "clang-cl");
    for path in [&clang, &clangxx, &clang_format] {
        set_executable(path)?;
    }
    if let Some(path) = &clang_cl {
        set_executable(path)?;
    }
    Ok(LlvmCompilers {
        root,
        clang,
        clangxx,
        clang_cl,
        clang_format,
    })
}

fn resolve_system_llvm() -> Result<LlvmCompilers, String> {
    let cxx_name = if cfg!(windows) { "clang-cl" } else { "clang++" };
    let c_name = if cfg!(windows) { "clang-cl" } else { "clang" };
    let clangxx = env_or_path("CXX", cxx_name)?;
    let clang = env_or_path("CC", c_name)?;
    let clang_format = env_or_path("CLANG_FORMAT", "clang-format")?;
    let root = clangxx
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let clang_cl = cfg!(windows).then_some(clangxx.clone());
    Ok(LlvmCompilers {
        root,
        clang,
        clangxx,
        clang_cl,
        clang_format,
    })
}

fn env_or_path(variable: &str, name: &str) -> Result<PathBuf, String> {
    if let Some(value) = std::env::var_os(variable) {
        let path = PathBuf::from(value);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "{variable} 环境变量指向的编译器不存在：{}",
            path.display()
        ));
    }
    find_on_path(name).ok_or_else(|| format!("系统工具旁路需要 {variable} 或 PATH 中的 {name}"))
}

/// 解析 CMake：仅在显式开启系统工具旁路后读取 `CMAKE`/PATH。
pub fn resolve_cmake() -> Result<PathBuf, String> {
    if use_system_tools() {
        if let Some(path) = std::env::var_os("CMAKE") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(format!(
                "CMAKE 环境变量指向的可执行文件不存在：{}",
                path.display()
            ));
        }
        if let Some(path) = find_on_path("cmake") {
            return Ok(path);
        }
    }
    ensure_tool(Tool::Cmake, None)
}

/// 解析 Ninja：默认使用托管缓存/下载。Ninja zip 需要 libarchive 解包，
/// 复用上一步解析到的 cmake（系统或托管）。只有显式开启系统工具旁路，
/// 才读取 PATH。
pub fn resolve_ninja(cmake: &Path) -> Result<PathBuf, String> {
    if use_system_tools()
        && let Some(path) = find_on_path("ninja")
    {
        return Ok(path);
    }
    ensure_tool(Tool::Ninja, Some(cmake))
}

/// Windows sanitizer 的 compiler-rt 运行库目录（`<资源目录>/lib/windows`）：
/// 动态 ASan 的 DLL 在此，测试环境必须能解析；链接期导入库由 CMake 侧按
/// clang 驱动器的注入序列显式消费（native/cmake/build-policy.cmake）。
/// 非 Windows 的 sanitizer 运行库随驱动器静态注入，无此目录需求。
pub fn compiler_rt_dll_dir(llvm: &LlvmCompilers) -> Result<PathBuf, String> {
    let output = Command::new(&llvm.clangxx)
        .arg("-print-resource-dir")
        .output()
        .map_err(|error| format!("执行 clang -print-resource-dir：{error}"))?;
    if !output.status.success() {
        return Err("clang -print-resource-dir 失败".into());
    }
    let resource = String::from_utf8_lossy(&output.stdout);
    let runtime = PathBuf::from(resource.trim()).join("lib").join("windows");
    if !runtime.is_dir() {
        return Err(format!("ASan 运行库目录不存在：{}", runtime.display()));
    }
    Ok(runtime)
}

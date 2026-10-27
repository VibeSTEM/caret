//! 持锁安装、归档校验编排、裁剪与完成标记发布。

use std::fs;
use std::path::{Path, PathBuf};

use fs4::{FileExt, TryLockError};

use crate::archive::{download, extract, sha256_file};
use crate::assets::Tool;
use crate::paths::{exe_name, find_binary, find_cmake_binary, set_executable, tools_root};

impl Tool {
    /// 解包后二进制的定位：CMake 在 `<top>/bin/`（macOS 额外嵌套
    /// CMake.app/Contents），Ninja 在解包根。
    fn locate_installed(self, root: &Path) -> Option<PathBuf> {
        let exe = exe_name(self.name());
        match self {
            Tool::Ninja => {
                let candidate = root.join(&exe);
                candidate.is_file().then_some(candidate)
            }
            Tool::Uv => find_binary(root, "uv"),
            Tool::Hyperfine => find_binary(root, "hyperfine"),
            Tool::Cmake => find_cmake_binary(root, &exe, 0),
            Tool::Llvm => {
                let compiler = if cfg!(windows) { "clang-cl" } else { "clang++" };
                find_binary(root, compiler)
            }
        }
    }
}

/// 定位 → 缺失时按清单获取 → 校验 → 解包 → 写 marker。`cmake` 仅在
/// 解压 Ninja zip 时使用。
pub(super) fn ensure_tool(tool: Tool, cmake: Option<&Path>) -> Result<PathBuf, String> {
    // 工具始终装进 `.toolchain/`（或 CARET_TOOLCHAIN_ROOT）。覆盖率的独立对象目录
    // 在 Cargo target 里，不另设一份工具缓存，也不回退到 target/。
    let toolchain = crate::toolchain_root()?;
    let name = tool.name();
    let Some(asset) = tool.asset() else {
        return Err(format!(
            "本平台（{}/{}/托管清单）没有固定的 {name} 预编译资产；\
             请登记固定资产，或显式设置 CARET_USE_SYSTEM_TOOLS=1 后用 CMAKE/PATH 提供本机工具；\
             供给资产登记见 ai-docs/standards/dependency-acquisition.md",
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    };

    let identity = format!("{}-{}", tool.version(), asset.sha256);
    let install = |staging: &Path| -> Result<(), String> {
        let archives = toolchain.join("archives");
        fs::create_dir_all(&archives).map_err(|e| e.to_string())?;
        let suffix = if cfg!(windows) && matches!(tool, Tool::Llvm) {
            "exe"
        } else {
            "archive"
        };
        let archive = archives.join(format!("{name}-{identity}.{suffix}"));
        eprintln!("[caret-tools] {name}：检查归档缓存 {}", archive.display());
        if !archive.is_file() || sha256_file(&archive)? != asset.sha256 {
            let partial = archives.join(format!("{name}-{identity}.partial"));
            download(&asset, &partial, name, tool.version())?;
            eprintln!("[caret-tools] {name}：校验下载归档 SHA256");
            let actual = sha256_file(&partial)?;
            if actual != asset.sha256 {
                fs::remove_file(&partial).map_err(|e| e.to_string())?;
                return Err(format!(
                    "{name} SHA256 不符：期望 {}，实际 {actual}",
                    asset.sha256
                ));
            }
            if archive.exists() {
                fs::remove_file(&archive).map_err(|e| e.to_string())?;
            }
            fs::rename(&partial, &archive).map_err(|e| e.to_string())?;
        }
        extract(tool, &archive, staging, cmake)?;
        let binary = tool
            .locate_installed(staging)
            .ok_or_else(|| format!("{name} 解包后缺少可执行文件"))?;
        set_executable(&binary)?;
        if matches!(tool, Tool::Llvm) {
            slim_llvm(staging)?;
        }
        // 归档只服务本次下载校验与解包；发布即删，避免与解包树在缓存中
        // 长期双份（约 1.5 GB）。损坏路径由 marker 语义承担：安装树损坏时
        // 删版本目录，重建走受限重下载，不依赖常驻归档。
        if let Err(e) = fs::remove_file(&archive) {
            eprintln!("[caret-tools] {name}：清理安装归档失败（忽略）：{e}");
        }
        Ok(())
    };
    // 缓存完整性检查和重装必须共用安装锁；锁外删除损坏目录可能删掉
    // 另一安装者刚发布的有效工具，也会让其余等待者因目录已删除而失败。
    let directory = install_directory_validated(&toolchain, name, &identity, install, |root| {
        tool.locate_installed(root).is_some()
    })?;
    tool.locate_installed(&directory).ok_or_else(|| {
        format!(
            "托管 {name} 缓存损坏：{}；清理该版本目录后重试",
            directory.display()
        )
    })
}

/// OS 锁在进程退出时自动释放；锁文件不能删除，否则等待者可能锁住不同 inode。
pub fn install_lock(toolchain_root: &Path, name: &str) -> Result<fs::File, String> {
    let locks = toolchain_root.join("locks");
    fs::create_dir_all(&locks).map_err(|e| e.to_string())?;
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(locks.join(format!("{name}.lock")))
        .map_err(|e| e.to_string())?;
    // 未竞争时不输出；只有真的发生等待才记录，避免例行日志刷屏。
    match FileExt::try_lock(&file) {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            eprintln!("[caret-tools] {name}：等待安装锁");
            FileExt::lock(&file).map_err(|error| format!("获取 {name} 安装锁失败：{error}"))?;
            eprintln!("[caret-tools] {name}：已获得安装锁");
        }
        Err(TryLockError::Error(error)) => return Err(format!("获取 {name} 安装锁失败：{error}")),
    }
    Ok(file)
}

/// 在互斥区内构建临时目录，写入完成标记后原子重命名；中断残留只在下次持锁时清理。
pub fn install_directory(
    toolchain_root: &Path,
    name: &str,
    identity: &str,
    install: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    install_directory_validated(toolchain_root, name, identity, install, |_| true)
}

fn install_directory_validated(
    toolchain_root: &Path,
    name: &str,
    identity: &str,
    install: impl FnOnce(&Path) -> Result<(), String>,
    is_valid: impl Fn(&Path) -> bool,
) -> Result<PathBuf, String> {
    let _lock = install_lock(toolchain_root, name)?;
    let parent = tools_root(toolchain_root).join(name);
    let destination = parent.join(identity);
    if fs::read_to_string(destination.join(".complete"))
        .ok()
        .as_deref()
        == Some(identity)
        && is_valid(&destination)
    {
        // 命中缓存是例行路径，不打日志；安装与自愈才输出诊断。
        return Ok(destination);
    }
    fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let staging = parent.join(format!(".{identity}.staging"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|e| e.to_string())?;
    install(&staging)?;
    if !is_valid(&staging) {
        return Err(format!("{name} 安装未生成有效的工具目录"));
    }
    fs::write(staging.join(".complete"), identity).map_err(|e| e.to_string())?;
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    eprintln!("[caret-tools] {name}：安装完成 {}", destination.display());
    Ok(destination)
}

/// 官方 LLVM 发布包包含编译器、LLDB/MLIR 工具和开发库；此处按白名单
/// 保留构建与质量检查所需的编译器、clang-tidy、覆盖率、符号化和 AR 工具，
/// Windows 还保留 lld-link。lib 仅保留编译器资源目录 lib/clang，供内建头、
/// 覆盖率和 sanitizer 运行库使用。白名单与安装验证器同步，新增工具先登记。
fn slim_llvm(root: &Path) -> Result<(), String> {
    // 按前缀保留：官方发布里 clang/clang++ 常是指向 clang-<major> 等带版本
    // 名的链接，精确名匹配会删掉真身、留下悬空链接。
    // 前缀命中后再排除确定不用的胖工具；lld 前缀会误吞 lldb 全家，先排除。
    const KEEP_PREFIXES: &[&str] = &[
        "clang",
        "llvm-ar",
        "llvm-ranlib",
        "llvm-profdata",
        "llvm-cov",
        "llvm-symbolizer",
        "lld",
        "ld.lld",
    ];
    const DROP_TOOLS: &[&str] = &[
        "clangd",
        "clang-repl",
        "clang-tblgen",
        "clang-check",
        "clang-refactor",
        "clang-extdef-mapping",
        "clang-include-fixer",
        "clang-linker-wrapper",
        "clang-offload-bundler",
        "clang-offload-packager",
    ];
    for entry in fs::read_dir(root).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if !entry.file_name().to_string_lossy().starts_with("LLVM-") {
            continue;
        }
        let tree = entry.path();
        let bin = tree.join("bin");
        if bin.is_dir() {
            for entry in fs::read_dir(&bin).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let mut name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with(".exe") {
                    name.truncate(name.len() - 4);
                }
                let keep = KEEP_PREFIXES.iter().any(|p| name.starts_with(p))
                    && !DROP_TOOLS.contains(&name.as_str());
                if !keep {
                    let path = entry.path();
                    fs::remove_file(&path)
                        .or_else(|_| fs::remove_dir_all(&path))
                        .map_err(|e| format!("裁剪 {} 失败：{e}", path.display()))?;
                }
            }
        }
        let lib = tree.join("lib");
        if lib.is_dir() {
            for entry in fs::read_dir(&lib).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry.file_name() == "clang" {
                    continue;
                }
                let path = entry.path();
                fs::remove_file(&path)
                    .or_else(|_| fs::remove_dir_all(&path))
                    .map_err(|e| format!("裁剪 {} 失败：{e}", path.display()))?;
            }
        }
        let include = tree.join("include");
        if include.is_dir() {
            fs::remove_dir_all(&include).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

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
    let mut directory = install_directory(&toolchain, name, &identity, install)?;
    // 历史缓存树可能被旧版裁剪留下悬空链接（如 clang++ → clang-<ver> 真身
    // 被删）；marker 命中但二进制缺失时，删除该版本目录重装一次自愈，仍
    // 失败才报损坏，不再要求手工清理。
    if tool.locate_installed(&directory).is_none() {
        eprintln!("[caret-tools] {name}：安装树不完整，删除后重装自愈");
        fs::remove_dir_all(&directory).map_err(|e| e.to_string())?;
        directory = install_directory(&toolchain, name, &identity, install)?;
    }
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
    let _lock = install_lock(toolchain_root, name)?;
    let parent = tools_root(toolchain_root).join(name);
    let destination = parent.join(identity);
    if fs::read_to_string(destination.join(".complete"))
        .ok()
        .as_deref()
        == Some(identity)
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
    fs::write(staging.join(".complete"), identity).map_err(|e| e.to_string())?;
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &destination).map_err(|e| e.to_string())?;
    eprintln!("[caret-tools] {name}：安装完成 {}", destination.display());
    Ok(destination)
}

/// 官方 LLVM 发布包面向全量工具链消费者：bin 是数百个各 100–290 MB 的
/// 胖二进制（含 MLIR 工具），lib 是 LLDB/MLIR 静态库与 dylib，include 是
/// LLVM 开发头文件。本项目只用 clang 系编译器、clang-tidy 与 coverage/AR
/// 工具（042 验证器要求 clang-tidy/llvm-profdata/llvm-cov 在 llvm/bin 下，
/// Windows 链接使用 lld-link）。解包后按白名单裁剪，安装树从约 7.4 GB
/// 降到约 2 GB；lib 仅保留编译器资源目录 lib/clang（内建头与覆盖/消毒
/// 运行时）。白名单与验证器同步，新增工具先登记再使用。
fn slim_llvm(root: &Path) -> Result<(), String> {
    // 按前缀保留：官方发布里 clang/clang++ 常是指向 clang-<major> 等带版本
    // 名的链接，精确名匹配会删掉真身、留下悬空链接（run 35430519988 实证）。
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
mod tests {
    use super::*;

    #[test]
    fn llvm_slim_keeps_whitelist_and_resource_dir() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("caret-slim-test-{}", std::process::id()));
        let tree = root.join("LLVM-22.1.7-test");
        let bin = tree.join("bin");
        fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        for name in ["clang", "clang-22", "clang-tidy.exe", "mlir-opt", "clangd"] {
            fs::write(bin.join(name), "x").map_err(|e| e.to_string())?;
        }
        // 官方包里 clang++ 常是指向版本化真身的链接：真身按前缀保留，
        // 链接才不会悬空。
        #[cfg(unix)]
        std::os::unix::fs::symlink(bin.join("clang-22"), bin.join("clang++"))
            .map_err(|e| e.to_string())?;
        let lib = tree.join("lib");
        fs::create_dir_all(lib.join("clang").join("22").join("lib")).map_err(|e| e.to_string())?;
        fs::write(lib.join("libclang.dylib"), "x").map_err(|e| e.to_string())?;
        fs::create_dir_all(tree.join("include")).map_err(|e| e.to_string())?;

        super::slim_llvm(&root)?;

        // 前缀命中的工具与版本化真身保留（.exe 剥离后匹配），clangd 与
        // mlir-opt 裁除；lib/include 开发内容裁除，编译器资源目录 lib/clang
        // 原样保留。
        for kept in ["clang", "clang-22", "clang-tidy.exe"] {
            assert!(bin.join(kept).is_file(), "{kept} 不应被裁剪");
        }
        for trimmed in ["mlir-opt", "clangd"] {
            assert!(!bin.join(trimmed).exists(), "{trimmed} 应被裁剪");
        }
        #[cfg(unix)]
        assert!(
            bin.join("clang++")
                .metadata()
                .map_err(|e| e.to_string())?
                .is_file(),
            "clang++ 链接不应悬空"
        );
        assert!(!lib.join("libclang.dylib").exists(), "lib 开发库应被裁剪");
        assert!(lib.join("clang").join("22").is_dir(), "资源目录应保留");
        assert!(!tree.join("include").exists(), "include 开发头应被裁剪");
        fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[test]
    fn concurrent_installers_publish_once_and_failed_upgrade_preserves_previous()
    -> Result<(), String> {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let root = std::env::temp_dir().join(format!("caret-lock-test-{}", std::process::id()));
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|e| e.to_string())?;
        }
        let count = Arc::new(AtomicUsize::new(0));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let root = root.clone();
                let count = count.clone();
                std::thread::spawn(move || {
                    install_directory(&root, "fixture", "v1", |staging| {
                        count.fetch_add(1, Ordering::SeqCst);
                        fs::write(staging.join("binary"), "v1").map_err(|e| e.to_string())
                    })
                })
            })
            .collect();
        for worker in workers {
            worker.join().map_err(|_| "安装线程 panic")??;
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert!(
            install_directory(&root, "fixture", "v2", |staging| {
                fs::write(staging.join("partial"), "incomplete").map_err(|e| e.to_string())?;
                Err("模拟安装中断".into())
            })
            .is_err()
        );
        let original = root.join("tools/fixture/v1/binary");
        assert_eq!(
            fs::read_to_string(&original).map_err(|e| e.to_string())?,
            "v1"
        );
        assert!(!root.join("tools/fixture/v2").exists());
        let upgraded = install_directory(&root, "fixture", "v2", |staging| {
            assert!(!staging.join("partial").exists());
            fs::write(staging.join("binary"), "v2").map_err(|e| e.to_string())
        })?;
        assert_eq!(
            fs::read_to_string(upgraded.join("binary")).map_err(|e| e.to_string())?,
            "v2"
        );
        install_directory(&root, "fixture", "v1", |_| Err("旧版本不应重新安装".into()))?;
        fs::remove_dir_all(root).map_err(|e| e.to_string())
    }
    #[test]
    fn crash_during_install() -> Result<(), String> {
        if let Some(root) = std::env::var_os("CARET_TEST_INSTALL_CRASH") {
            install_directory(
                std::path::Path::new(&root),
                "crash-fixture",
                "v1",
                |staging| {
                    fs::write(staging.join("partial"), "partial").map_err(|e| e.to_string())?;
                    // 不执行析构函数，模拟进程中断；OS 必须释放安装锁。
                    std::process::exit(23);
                },
            )?;
        }
        Ok(())
    }

    #[test]
    fn process_exit_releases_install_lock_and_retry_discards_partial_files() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("caret-crash-test-{}", std::process::id()));
        let status =
            std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .args(["--exact", "install::tests::crash_during_install"])
                .env("CARET_TEST_INSTALL_CRASH", &root)
                .status()
                .map_err(|e| e.to_string())?;
        assert_eq!(status.code(), Some(23));
        let directory = install_directory(&root, "crash-fixture", "v1", |staging| {
            assert!(!staging.join("partial").exists());
            fs::write(staging.join("binary"), "complete").map_err(|e| e.to_string())
        })?;
        assert!(directory.join("binary").is_file());
        fs::remove_dir_all(root).map_err(|e| e.to_string())
    }

    #[test]
    fn invalid_marker_rebuilds_without_publishing_failed_attempt() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("caret-marker-test-{}", std::process::id()));
        let directory = install_directory(&root, "fixture", "v1", |staging| {
            fs::write(staging.join("binary"), "original").map_err(|e| e.to_string())
        })?;
        fs::write(directory.join(".complete"), "wrong identity").map_err(|e| e.to_string())?;
        let failure = install_directory(&root, "fixture", "v1", |staging| {
            fs::write(staging.join("partial"), "incomplete").map_err(|e| e.to_string())?;
            Err("interrupted".to_owned())
        });
        assert_eq!(failure, Err("interrupted".to_owned()));
        assert_eq!(
            fs::read_to_string(directory.join("binary")).map_err(|e| e.to_string())?,
            "original"
        );
        let replacement = install_directory(&root, "fixture", "v1", |staging| {
            assert!(!staging.join("partial").exists());
            fs::write(staging.join("binary"), "replacement").map_err(|e| e.to_string())
        })?;
        assert_eq!(replacement, directory);
        assert_eq!(
            fs::read_to_string(directory.join("binary")).map_err(|e| e.to_string())?,
            "replacement"
        );
        assert_eq!(
            fs::read_to_string(directory.join(".complete")).map_err(|e| e.to_string())?,
            "v1"
        );
        install_directory(&root, "fixture", "v1", |_| {
            Err("cache hit must not install".to_owned())
        })?;
        fs::remove_dir_all(root).map_err(|e| e.to_string())
    }
}

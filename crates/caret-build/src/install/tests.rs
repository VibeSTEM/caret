//! 验证安装事务、文件部署和失败回滚。

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
fn concurrent_installers_publish_once_and_failed_upgrade_preserves_previous() -> Result<(), String>
{
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
fn concurrent_cache_repair_checks_integrity_under_the_install_lock() -> Result<(), String> {
    use std::sync::{
        Arc, Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let root = std::env::temp_dir().join(format!("caret-repair-test-{}", std::process::id()));
    // 完成标记存在，但工具二进制已损坏；四个调用者同时要求自愈。
    let destination = install_directory(&root, "fixture", "v1", |_| Ok(()))?;
    let repairs = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(4));
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let root = root.clone();
            let repairs = repairs.clone();
            let start = start.clone();
            std::thread::spawn(move || {
                start.wait();
                let path = install_directory_validated(
                    &root,
                    "fixture",
                    "v1",
                    |staging| {
                        repairs.fetch_add(1, Ordering::SeqCst);
                        fs::write(staging.join("binary"), "repaired").map_err(|e| e.to_string())
                    },
                    |path| path.join("binary").is_file(),
                )?;
                fs::read_to_string(path.join("binary")).map_err(|e| e.to_string())
            })
        })
        .collect();
    for worker in workers {
        assert_eq!(
            worker.join().map_err(|_| "repair thread panicked")??,
            "repaired"
        );
    }
    assert_eq!(repairs.load(Ordering::SeqCst), 1);
    assert_eq!(
        fs::read_to_string(destination.join(".complete")).map_err(|e| e.to_string())?,
        "v1"
    );
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
    let status = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
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

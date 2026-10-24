//! 各质量工具执行顺序、检查 / 修复模式及扫描诊断。

use std::error::Error;
use std::path::Path;
use std::process::Command;

use crate::build::{build_benchmark_moc, build_launcher, run_qmllint};
use crate::command::{cargo, cargo_scanner, run, run_scanner};
use crate::format::run_cmake_tool;
use crate::paths::{native_build_dir, repository_root, target_root};
use crate::test_scope::{NativeLintScope, is_cmake_path};

pub(super) fn lint(
    tool: Option<&str>,
    check: bool,
    changed: bool,
    arguments: &[String],
) -> Result<(), Box<dyn Error>> {
    let objcxx_only = match arguments {
        [] => false,
        [argument] if argument == "--objcxx-only" => true,
        _ => return Err("lint 仅接受工具名及 --check、--changed、--objcxx-only".into()),
    };
    if objcxx_only
        && (!matches!(tool, Some("clang-tidy" | "includes")) || !cfg!(target_os = "macos"))
    {
        return Err("--objcxx-only 仅用于 macOS 的 clang-tidy/includes".into());
    }
    match tool {
        None if changed => lint_changed(check),
        None => {
            eprintln!("cargo lint [1/9] Rust Clippy");
            lint(Some("clippy"), check, false, &[])?;
            eprintln!("cargo lint [2/9] unused Cargo dependencies");
            lint(Some("machete"), check, false, &[])?;
            eprintln!("cargo lint [3/9] CMake format and lint");
            lint(Some("cmake"), check, false, &[])?;
            eprintln!("cargo lint [4/9] Rust/CMake layering");
            crate::layering::check()?;
            eprintln!("cargo lint [5/9] native build and QML metadata");
            build_launcher()?;
            eprintln!("cargo lint [6/9] qmllint");
            run_qmllint()?;
            build_benchmark_moc()?;
            eprintln!("cargo lint [7/9] clang-tidy");
            scan_clang_tidy(false, check, &NativeLintScope::All, false)?;
            eprintln!("cargo lint [8/9] include-cleaner");
            scan_clang_tidy(true, check, &NativeLintScope::All, false)?;
            eprintln!("cargo lint [9/9] cppcheck");
            scan_cppcheck()
        }
        Some("clippy") if changed => run_changed_clippy(check),
        Some("clippy") if check => cargo(
            "clippy",
            [
                "--locked",
                "--workspace",
                "--all-targets",
                "--target-dir",
                "--",
                "-D",
                "warnings",
            ],
        ),
        // 修复模式先应用机器可修复建议，再落回检查模式确认剩余问题。
        Some("clippy") => {
            cargo(
                "clippy",
                [
                    "--fix",
                    "--locked",
                    "--workspace",
                    "--all-targets",
                    "--allow-dirty",
                ],
            )?;
            lint(Some("clippy"), true, false, &[])
        }
        Some("machete") => cargo_scanner("machete", []),
        Some("cmake") if changed => run_changed_cmake_lint(),
        Some("cmake") => run_cmake_lint(),
        Some("layering") => crate::layering::run_lint(),
        Some("qmllint") => {
            build_launcher()?;
            run_qmllint()
        }
        Some("clang-tidy") => run_clang_tidy(false, check, changed, objcxx_only),
        Some("includes") => run_clang_tidy(true, check, changed, objcxx_only),
        Some("cppcheck") => run_cppcheck(changed),
        Some(other) => Err(format!(
            "未知 lint 工具 '{other}'；可用：clippy、machete、cmake、layering、qmllint、clang-tidy、\
             includes、cppcheck；--check 只检查不修复"
        )
        .into()),
    }
}

fn lint_changed(check: bool) -> Result<(), Box<dyn Error>> {
    if crate::test_scope::force_full_scope() {
        return lint(None, check, false, &[]);
    }
    let paths = crate::test_scope::changed_paths()?;
    if paths.is_empty() {
        println!("变更 lint：没有变更路径");
        return Ok(());
    }
    let rust_packages = crate::test_scope::changed_lint_packages()?;
    if !rust_packages.is_empty() {
        run_changed_clippy(check)?;
        lint(Some("machete"), check, false, &[])?;
    }

    if paths.iter().any(|path| is_layering_path(path)) {
        run_changed_cmake_lint()?;
        crate::layering::check()?;
    }

    let native_scope = active_native_scope(crate::test_scope::native_lint_scope()?, false);
    let qml_changed = paths.iter().any(|path| {
        path.starts_with("qml/") || path.starts_with("tests/qml/") || path.starts_with("native/")
    }) || matches!(native_scope, NativeLintScope::All);

    if !native_scope.is_empty() || qml_changed {
        // 当前平台共用一次构建；没有静态分析输入时不准备 LLVM/编译数据库。
        build_launcher()?;
    }
    if !native_scope.is_empty() {
        build_benchmark_moc()?;
        scan_clang_tidy(false, check, &native_scope, false)?;
        scan_clang_tidy(true, check, &native_scope, false)?;
        // unusedFunction 依赖跨翻译单元信息，源文件集合或共享输入变化均完整检查。
        scan_cppcheck()?;
    }

    if qml_changed {
        run_qmllint()?;
    }
    Ok(())
}

fn is_layering_path(path: &str) -> bool {
    is_cmake_path(path)
        || path == "Cargo.toml"
        || path.ends_with("/Cargo.toml")
        || path == "Cargo.lock"
}

fn run_changed_clippy(check: bool) -> Result<(), Box<dyn Error>> {
    let packages = crate::test_scope::changed_lint_packages()?;
    if packages.is_empty() {
        println!("Clippy 变更检查：没有受影响的 Rust package");
        return Ok(());
    }

    let mut command = Command::new("cargo");
    command.args(["clippy", "--locked", "--all-targets"]);
    if !check {
        command.args(["--fix", "--allow-dirty"]);
    }
    for package in &packages {
        command.arg("--package").arg(package);
    }
    command.arg("--target-dir").arg(target_root());
    if check {
        command.args(["--", "-D", "warnings"]);
    }
    let label = packages.iter().cloned().collect::<Vec<_>>().join(", ");
    run(&format!("Clippy 变更包（{label}）"), command)?;
    if !check {
        run_changed_clippy(true)?;
    }
    Ok(())
}

fn run_cmake_lint() -> Result<(), Box<dyn Error>> {
    run_cmake_tool("cmake-lint", &[])
}

fn run_changed_cmake_lint() -> Result<(), Box<dyn Error>> {
    if crate::test_scope::force_full_scope() {
        return run_cmake_lint();
    }
    let root = repository_root()?;
    let files = crate::test_scope::changed_paths()?
        .iter()
        .map(|path| root.join(path))
        .filter(|path| {
            path.is_file()
                && (path
                    .file_name()
                    .is_some_and(|name| name == "CMakeLists.txt")
                    || path
                        .extension()
                        .is_some_and(|extension| extension == "cmake"))
                && ["native", "qml", "tools"]
                    .iter()
                    .any(|directory| path.starts_with(root.join(directory)))
        })
        .collect::<Vec<_>>();
    if files.is_empty() {
        println!("cmake-lint 变更检查：没有修改的 CMake 文件");
        return Ok(());
    }
    let mut command = caret_build::python::command(root, "cmake-lint")?;
    command.args(&files);
    run_scanner("cmake-lint 变更文件", command)
}

fn active_native_scope(mut scope: NativeLintScope, objcxx_only: bool) -> NativeLintScope {
    if let NativeLintScope::Sources(sources) = &mut scope {
        sources.retain(|source| {
            let objcxx = Path::new(source).extension().is_some_and(|ext| ext == "mm");
            if objcxx_only {
                objcxx
            } else {
                !objcxx || cfg!(target_os = "macos")
            }
        });
    }
    scope
}

fn run_clang_tidy(
    includes_only: bool,
    check: bool,
    changed: bool,
    objcxx_only: bool,
) -> Result<(), Box<dyn Error>> {
    let scope = active_native_scope(
        if changed {
            crate::test_scope::native_lint_scope()?
        } else {
            NativeLintScope::All
        },
        objcxx_only,
    );
    if scope.is_empty() {
        println!("LLVM 变更检查：没有当前平台的静态分析输入，跳过构建");
        return Ok(());
    }
    build_launcher()?;
    if !objcxx_only {
        build_benchmark_moc()?;
    }
    scan_clang_tidy(includes_only, check, &scope, objcxx_only)
}

/// 输入已由共享影响规则分类；已有源码无法映射到编译图时保守扩大扫描。
fn select_native_commands(
    mut commands: Vec<caret_build::database::CompileCommand>,
    scope: &NativeLintScope,
    root: &Path,
    objcxx_only: bool,
) -> Result<Vec<caret_build::database::CompileCommand>, Box<dyn Error>> {
    if commands.is_empty() {
        return Err("clang-tidy 翻译单元清单为空".into());
    }
    if objcxx_only {
        commands.retain(|entry| entry.source().extension().is_some_and(|ext| ext == "mm"));
        if commands.is_empty() {
            return Err("Objective-C++ 翻译单元清单为空".into());
        }
    }
    if let NativeLintScope::Sources(sources) = scope {
        let selected = sources
            .iter()
            .map(|source| root.join(source))
            .filter(|source| source.is_file())
            .map(|source| source.canonicalize().unwrap_or(source))
            .collect::<std::collections::BTreeSet<_>>();
        let registered = commands
            .iter()
            .map(|entry| {
                let source = entry.source();
                source.canonicalize().unwrap_or(source)
            })
            .collect::<std::collections::BTreeSet<_>>();
        if selected.is_subset(&registered) {
            commands.retain(|entry| {
                let source = entry.source();
                selected.contains(&source.canonicalize().unwrap_or(source))
            });
        } else {
            println!("变更源码未完整登记在编译图中，扩大到当前平台完整 LLVM 扫描");
        }
    }
    Ok(commands)
}

fn scan_clang_tidy(
    includes_only: bool,
    check: bool,
    scope: &NativeLintScope,
    objcxx_only: bool,
) -> Result<(), Box<dyn Error>> {
    let directory = native_build_dir().join("quality");
    let commands = select_native_commands(
        caret_build::database::read(&directory.join("compile_commands.json"))?,
        scope,
        repository_root()?,
        objcxx_only,
    )?;
    if commands.is_empty() {
        println!("LLVM 变更检查：已删除的源码不在当前编译图中");
        return Ok(());
    }
    println!("LLVM 检查：{} 个自有翻译单元", commands.len());
    let llvm = caret_build::resolve_llvm_compilers()?;
    let tool = llvm
        .root
        .join("bin")
        .join(caret_build::exe_name("clang-tidy"));
    let env = caret_build::native_test_env(env!("CARET_TEST_HOST"))?;
    // TU 之间相互独立、--fix 也只写各自源文件，按核数分片并行缩短最重
    // 门禁（单遍串行要数分钟）；单 TU 失败不短路，聚合后原样转发完整
    // 诊断。
    let workers = std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, 8);
    let chunk_size = commands.len().div_ceil(workers);
    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = commands
            .chunks(chunk_size)
            .map(|chunk| {
                let tool = &tool;
                let env = &env;
                let directory = &directory;
                scope.spawn(move || -> Vec<String> {
                    let mut local = Vec::new();
                    for entry in chunk {
                        let file = entry.source();
                        let mut command = Command::new(tool);
                        command.envs(env.clone());
                        command.arg("-p").arg(directory).arg("-quiet");
                        if includes_only {
                            command.arg("--checks=-*,misc-include-cleaner");
                        }
                        if !check {
                            command.arg("--fix");
                        }
                        command.arg(&file);
                        if let Err(error) =
                            run_scanner(&format!("clang-tidy {}", file.display()), command)
                        {
                            local.push(error.to_string());
                        }
                    }
                    local
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| {
                handle
                    .join()
                    .unwrap_or_else(|_| vec!["clang-tidy 工作线程异常退出".to_owned()])
            })
            .collect()
    });
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n").into())
    }
}

fn run_cppcheck(changed: bool) -> Result<(), Box<dyn Error>> {
    if changed && active_native_scope(crate::test_scope::native_lint_scope()?, false).is_empty() {
        println!("Cppcheck 变更检查：没有静态分析输入，跳过构建");
        return Ok(());
    }
    build_launcher()?;
    build_benchmark_moc()?;
    scan_cppcheck()
}

fn scan_cppcheck() -> Result<(), Box<dyn Error>> {
    let database = native_build_dir().join("quality/cppcheck.json");
    let mut entries = caret_build::database::read(&database)?;
    let root = repository_root()?;
    // Qt 6.11 的编译器宏超出 Cppcheck 解析能力；使用 Qt 库模型，LLVM 负责真实头文件。
    for entry in &mut entries {
        entry.exclude_include_root(&caret_build::toolchain_root()?.join("qt"))?;
    }
    let database = database.with_file_name("cppcheck-configured.json");
    caret_build::database::write(&database, &entries)?;
    let mut command = caret_build::python::command(repository_root()?, "cppcheck")?;
    command
        .arg(format!("--project={}", database.display()))
        // 只补充跨翻译单元的未使用函数；常规诊断由 clang-tidy 负责。
        .args([
            "--enable=unusedFunction",
            "--error-exitcode=1",
            "--inline-suppr",
            "--quiet",
            "--check-level=exhaustive",
            "--suppress=missingIncludeSystem",
            // CXX Slice 迭代器的类型擦除被 Cppcheck 误判；仅排除其固定生成头。
            "--suppress=eraseDereference:*cxxbridge/include/rust/cxx.h",
        ]);
    command.arg(format!(
        "--library=qt,googletest,{}",
        root.join("tests/cppcheck-qt.cfg").display()
    ));
    command.arg(if cfg!(windows) {
        "--platform=win64"
    } else {
        "--platform=unix64"
    });
    command.arg(format!(
        "--suppress=unusedFunction:{}/*",
        target_root().display()
    ));
    command.arg(format!(
        "--suppress-xml={}",
        repository_root()?
            .join("tests/cppcheck-suppressions.xml")
            .display()
    ));
    run_scanner("cppcheck", command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_build::database::CompileCommand;
    use std::collections::BTreeSet;

    #[test]
    fn native_scan_preserves_shared_consumers_and_rejects_an_empty_platform_graph()
    -> Result<(), Box<dyn Error>> {
        let root = std::env::temp_dir().join(format!("caret-native-scope-{}", std::process::id()));
        std::fs::create_dir_all(&root)?;
        for source in ["a.cpp", "b.mm", "unregistered.cpp"] {
            std::fs::write(root.join(source), "")?;
        }
        let commands = ["a.cpp", "b.mm"]
            .map(|file| CompileCommand {
                directory: root.clone(),
                file: file.into(),
                arguments: Some(vec!["clang++".into()]),
                command: None,
            })
            .to_vec();
        let scope = |source: &str| NativeLintScope::Sources(BTreeSet::from([source.to_owned()]));
        let result = (|| -> Result<(), Box<dyn Error>> {
            let changed = select_native_commands(commands.clone(), &scope("a.cpp"), &root, false)?;
            assert_eq!(changed.len(), 1);
            assert!(changed[0].source().ends_with("a.cpp"));
            assert_eq!(
                select_native_commands(commands.clone(), &NativeLintScope::All, &root, false)?
                    .len(),
                2
            );
            // 已存在但未登记的源文件不能成功筛空；删除的源文件则无需再分析。
            assert_eq!(
                select_native_commands(commands.clone(), &scope("unregistered.cpp"), &root, false)?
                    .len(),
                2
            );
            assert!(
                select_native_commands(commands.clone(), &scope("deleted.cpp"), &root, false)?
                    .is_empty()
            );
            let objcxx =
                select_native_commands(commands.clone(), &NativeLintScope::All, &root, true)?;
            assert_eq!(objcxx.len(), 1);
            assert!(objcxx[0].source().ends_with("b.mm"));
            assert!(
                select_native_commands(
                    vec![commands[0].clone()],
                    &NativeLintScope::All,
                    &root,
                    true
                )
                .is_err()
            );
            assert!(select_native_commands(vec![], &NativeLintScope::All, &root, false).is_err());
            Ok(())
        })();
        std::fs::remove_dir_all(root)?;
        result
    }

    #[test]
    fn lint_rejects_unrecognized_options_without_running_tools() {
        assert!(lint(Some("clippy"), true, true, &["--unknown".into()]).is_err());
        assert!(lint(None, true, true, &["--objcxx-only".into()]).is_err());
    }
}

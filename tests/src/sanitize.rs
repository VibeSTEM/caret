//! 平台 sanitizer 矩阵、构建目录和子进程环境。

use std::error::Error;
use std::path::Path;
use std::process::Command;

use crate::build::run_ctest_in;
use crate::command::run;
use crate::paths::{repository_root, target_root};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SanitizerProfile {
    Address,
    AddressUndefined,
    Thread,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SanitizerPlatform {
    Linux,
    MacOs,
    Windows,
}

const fn sanitizer_platform() -> SanitizerPlatform {
    if cfg!(windows) {
        SanitizerPlatform::Windows
    } else if cfg!(target_os = "macos") {
        SanitizerPlatform::MacOs
    } else {
        SanitizerPlatform::Linux
    }
}

impl SanitizerProfile {
    const fn name(self) -> &'static str {
        match self {
            Self::Address => "asan",
            Self::AddressUndefined => "asan-ubsan",
            Self::Thread => "tsan",
        }
    }

    const fn flags(self) -> &'static str {
        match self {
            Self::Address => "address",
            Self::AddressUndefined => "address,undefined",
            Self::Thread => "thread",
        }
    }
}

/// 项目矩阵：Unix 使用 ASan+UBSan 和独立的 TSan 构建，Windows 使用 ASan。
/// clang-cl 的 UBSan 支持有限，TSan 不进入 Windows 矩阵；不同组合不共用运行库。
fn sanitizer_profiles(platform: SanitizerPlatform) -> &'static [SanitizerProfile] {
    if platform == SanitizerPlatform::Windows {
        &[SanitizerProfile::Address]
    } else {
        &[SanitizerProfile::AddressUndefined, SanitizerProfile::Thread]
    }
}

pub(super) fn sanitize() -> Result<(), Box<dyn Error>> {
    for profile in sanitizer_profiles(sanitizer_platform()) {
        sanitize_profile(*profile)?;
    }
    Ok(())
}

/// 每个组合构建独立的插桩 CMake 树并完整执行 CTest；UBSan 以
/// `-fno-sanitize-recover=undefined` 保证错误即非零退出，sanitizer 缺省
/// 退出码（ASan/UBSan 非零、TSan 66）经 CTest 原样失败。目录名由 flags
/// 派生，与 launcher build.rs 的构建树命名规则保持一致。
fn sanitize_profile(profile: SanitizerProfile) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let target = target_root();
    let build = sanitizer_build_command(root, target, profile.flags());
    run(&format!("sanitizer {} build", profile.name()), build)?;
    let native = target.join("native").join(format!(
        "debug-sanitizer-{}",
        profile.flags().replace(',', "-")
    ));
    if !native.is_dir() {
        return Err(format!("sanitizer 构建树不存在：{}", native.display()).into());
    }
    if profile == SanitizerProfile::Address && sanitizer_platform() == SanitizerPlatform::Windows {
        verify_windows_asan_import_order(&native, &caret_build::resolve_llvm_compilers()?.root)?;
    }
    // Linux Qt 6.11 的预编译 QML loader 在 TSan 下报告 QtCore 内部的分配/释放竞态
    // （Nightly run 40206639630）；仅该平台的 TSan 排除此测试。升级托管 Qt 后复查。
    // 其他 sanitizer、平台和常规测试仍完整运行。
    run_ctest_in(
        &native,
        sanitizer_test_env(profile)?,
        &format!("sanitizer {} ctest", profile.name()),
        None,
        None,
        ctest_exclusion(profile, sanitizer_platform()),
    )
}

/// 确认 Windows 动态 ASan 在 Qt 模块之前初始化，避免未插桩 DLL 的启动期分配被误报。
fn verify_windows_asan_import_order(native: &Path, llvm_root: &Path) -> Result<(), Box<dyn Error>> {
    let binary = native
        .join("qml")
        .join(caret_build::exe_name("caret_shell_load_test"));
    let readobj = llvm_root
        .join("bin")
        .join(caret_build::exe_name("llvm-readobj"));
    let output = Command::new(readobj)
        .arg("--coff-imports")
        .arg(&binary)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "llvm-readobj 无法读取 Windows QML 测试导入表：{}\n{}",
            binary.display(),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let imports = String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|line| line.trim().strip_prefix("Name:").map(str::trim))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    validate_windows_asan_imports(&imports)?;
    println!("Windows ASan import order: {}", imports.join(" -> "));
    Ok(())
}

fn validate_windows_asan_imports(imports: &[String]) -> Result<(), String> {
    let asan_index = imports
        .iter()
        .position(|name| name.eq_ignore_ascii_case("clang_rt.asan_dynamic-x86_64.dll"));
    let qt_index = imports
        .iter()
        .position(|name| name.eq_ignore_ascii_case("Qt6Core.dll"));
    let (Some(asan_index), Some(qt_index)) = (asan_index, qt_index) else {
        return Err(format!(
            "Windows QML 测试导入表缺少 ASan/Qt6Core：{}",
            imports.join(", ")
        ));
    };
    if asan_index != 0 || asan_index > qt_index {
        return Err(format!(
            "Windows ASan 必须是首个导入依赖并先于 Qt6Core.dll 初始化；当前导入顺序：{}",
            imports.join(" -> ")
        ));
    }
    Ok(())
}

fn ctest_exclusion(profile: SanitizerProfile, platform: SanitizerPlatform) -> Option<&'static str> {
    (profile == SanitizerProfile::Thread && platform == SanitizerPlatform::Linux)
        .then_some("^Qml\\.ShellLoad$")
}

/// 托管 LLVM 的符号器和 Windows ASan 运行库加入 PATH；所有问题默认阻断。
/// Unix 显式开启泄漏检查；macOS 仅抑制 CI 已确认的系统框架进程级缓存。
fn sanitizer_test_env(
    profile: SanitizerProfile,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, Box<dyn Error>> {
    let root = repository_root()?;
    let environment = caret_build::native_test_env(env!("CARET_TEST_HOST"))?;
    let llvm = caret_build::resolve_llvm_compilers()?;
    let mut prepend = vec![llvm.root.join("bin")];
    if cfg!(windows) {
        prepend.push(caret_build::compiler_rt_dll_dir(&llvm)?);
    }
    let mut environment = caret_build::prepend_path(environment, prepend)?;
    environment.extend(sanitizer_runtime_options(
        profile,
        sanitizer_platform(),
        root,
    ));
    Ok(environment)
}

fn sanitizer_runtime_options(
    profile: SanitizerProfile,
    platform: SanitizerPlatform,
    root: &Path,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut options = Vec::new();
    if platform != SanitizerPlatform::Windows {
        options.push(("ASAN_OPTIONS".into(), "detect_leaks=1".into()));
    }
    if profile == SanitizerProfile::Thread {
        options.push(("TSAN_OPTIONS".into(), "halt_on_error=1".into()));
    }
    if platform == SanitizerPlatform::MacOs && profile == SanitizerProfile::AddressUndefined {
        options.push((
            "LSAN_OPTIONS".into(),
            format!(
                "suppressions={}",
                root.join("tests/fixtures/sanitizers/macos-system-frameworks-lsan.supp")
                    .display()
            )
            .into(),
        ));
    }
    options
}

fn sanitizer_build_command(root: &Path, target: &Path, flags: &str) -> Command {
    let mut build = Command::new("cargo");
    build
        .current_dir(root)
        .args(["build", "--locked", "-p", "caret-launcher", "--target-dir"])
        .arg(target)
        .env("CARET_NATIVE_SANITIZER", flags);
    build
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_preserve_platform_matrix() {
        let expected = if sanitizer_platform() == SanitizerPlatform::Windows {
            &[SanitizerProfile::Address][..]
        } else {
            &[SanitizerProfile::AddressUndefined, SanitizerProfile::Thread][..]
        };
        assert_eq!(sanitizer_profiles(sanitizer_platform()), expected);
        assert_eq!(
            sanitizer_profiles(SanitizerPlatform::Windows),
            &[SanitizerProfile::Address]
        );
        assert_eq!(
            sanitizer_profiles(SanitizerPlatform::Linux),
            &[SanitizerProfile::AddressUndefined, SanitizerProfile::Thread]
        );
        assert_eq!(
            sanitizer_profiles(SanitizerPlatform::MacOs),
            &[SanitizerProfile::AddressUndefined, SanitizerProfile::Thread]
        );
    }

    #[test]
    fn sanitizer_profiles_keep_checks_enabled_and_exclude_only_the_known_qt_tsan_test() {
        let root = Path::new("repository root");
        for platform in [
            SanitizerPlatform::Linux,
            SanitizerPlatform::MacOs,
            SanitizerPlatform::Windows,
        ] {
            for profile in sanitizer_profiles(platform) {
                let options = sanitizer_runtime_options(*profile, platform, root);
                for (name, value) in &options {
                    if name == "LSAN_OPTIONS" {
                        assert!(value.to_string_lossy().ends_with(
                            "tests/fixtures/sanitizers/macos-system-frameworks-lsan.supp"
                        ));
                    }
                }
                if *profile == SanitizerProfile::Thread {
                    assert!(options.iter().any(|(name, value)| name == "TSAN_OPTIONS"
                        && value.to_string_lossy().starts_with("halt_on_error=1")));
                }
                if platform == SanitizerPlatform::Windows {
                    assert!(options.is_empty());
                } else {
                    assert!(options.contains(&("ASAN_OPTIONS".into(), "detect_leaks=1".into())));
                }
                assert_eq!(
                    ctest_exclusion(*profile, platform),
                    (platform == SanitizerPlatform::Linux && *profile == SanitizerProfile::Thread)
                        .then_some("^Qml\\.ShellLoad$")
                );
            }
        }
    }

    #[test]
    fn build_command_preserves_locked_target_and_sanitizer_flags() {
        let root = Path::new("repository with spaces");
        let target = Path::new("target with spaces");
        let command = sanitizer_build_command(root, target, "address,undefined");
        assert_eq!(command.get_program(), "cargo");
        assert_eq!(command.get_current_dir(), Some(root));
        let args = command
            .get_args()
            .map(std::ffi::OsStr::to_os_string)
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            [
                "build",
                "--locked",
                "-p",
                "caret-launcher",
                "--target-dir",
                "target with spaces"
            ]
            .map(std::ffi::OsString::from)
        );
        assert_eq!(
            command.get_envs().collect::<Vec<_>>(),
            vec![(
                std::ffi::OsStr::new("CARET_NATIVE_SANITIZER"),
                Some(std::ffi::OsStr::new("address,undefined"))
            )]
        );
    }

    #[test]
    fn windows_asan_import_must_precede_qt_core() {
        let valid = vec![
            "clang_rt.asan_dynamic-x86_64.dll".to_owned(),
            "KERNEL32.dll".to_owned(),
            "Qt6Core.dll".to_owned(),
        ];
        assert!(validate_windows_asan_imports(&valid).is_ok());

        let late_runtime = vec![
            "Qt6Core.dll".to_owned(),
            "clang_rt.asan_dynamic-x86_64.dll".to_owned(),
        ];
        assert!(validate_windows_asan_imports(&late_runtime).is_err());

        let runtime_after_system_import = vec![
            "KERNEL32.dll".to_owned(),
            "clang_rt.asan_dynamic-x86_64.dll".to_owned(),
            "Qt6Core.dll".to_owned(),
        ];
        assert!(validate_windows_asan_imports(&runtime_after_system_import).is_err());
    }
}

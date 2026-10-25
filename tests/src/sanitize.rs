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
    // 当前仅插桩自有 native 代码，Qt 和 Rust 的同步边界仍需结合普通测试判断。
    // 没有本仓库复现证据时不继承来源项目的用例排除或第三方抑制清单。
    run_ctest_in(
        &native,
        sanitizer_test_env(profile)?,
        &format!("sanitizer {} ctest", profile.name()),
        None,
        None,
        None,
    )
}

/// 托管 LLVM 的符号器和 Windows ASan 运行库加入 PATH；所有问题默认阻断。
/// Unix 显式开启泄漏检查，不引用未在本仓库登记和验证的抑制文件。
fn sanitizer_test_env(
    profile: SanitizerProfile,
) -> Result<Vec<(std::ffi::OsString, std::ffi::OsString)>, Box<dyn Error>> {
    let environment = caret_build::native_test_env(env!("CARET_TEST_HOST"))?;
    let llvm = caret_build::resolve_llvm_compilers()?;
    let mut prepend = vec![llvm.root.join("bin")];
    if cfg!(windows) {
        prepend.push(caret_build::compiler_rt_dll_dir(&llvm)?);
    }
    let mut environment = caret_build::prepend_path(environment, prepend)?;
    environment.extend(sanitizer_runtime_options(profile, sanitizer_platform()));
    Ok(environment)
}

fn sanitizer_runtime_options(
    profile: SanitizerProfile,
    platform: SanitizerPlatform,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let mut options = Vec::new();
    if platform != SanitizerPlatform::Windows {
        options.push(("ASAN_OPTIONS".into(), "detect_leaks=1".into()));
    }
    if profile == SanitizerProfile::Thread {
        options.push(("TSAN_OPTIONS".into(), "halt_on_error=1".into()));
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
    fn supported_profiles_do_not_suppress_reports_or_reference_missing_lists() {
        for platform in [
            SanitizerPlatform::Linux,
            SanitizerPlatform::MacOs,
            SanitizerPlatform::Windows,
        ] {
            for profile in sanitizer_profiles(platform) {
                let options = sanitizer_runtime_options(*profile, platform);
                for (name, value) in &options {
                    assert_ne!(name, "LSAN_OPTIONS");
                    assert!(!value.to_string_lossy().contains("suppressions="));
                }
                if *profile == SanitizerProfile::Thread {
                    assert!(options.contains(&("TSAN_OPTIONS".into(), "halt_on_error=1".into())));
                }
                if platform == SanitizerPlatform::Windows {
                    assert!(options.is_empty());
                } else {
                    assert!(options.contains(&("ASAN_OPTIONS".into(), "detect_leaks=1".into())));
                }
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
}

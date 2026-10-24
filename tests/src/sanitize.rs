//! 平台 sanitizer 矩阵、构建目录、子进程环境与已登记排除。

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

/// sanitizer 矩阵（任务 042）：ASan+UBSan 是三平台主组合；TSan 与其他
/// sanitizer 运行库互斥，只能独立构建，且官方支持平台不含 Windows；clang-cl
/// 仅支持部分 UBSan 检查，Windows 矩阵只验证 ASan。官方依据见任务 042。
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
    // 组合级排除（边界登记 042）：当前仅插桩 C++，TSan 无法识别 Rust std
    // 同步关系（rust-lang/rust#110485），相关异步结果测试仍由普通 CTest、
    // ASan/UBSan 与 Rust 侧 Miri 覆盖。
    // QML 测试栈（Qt/glib/系统库）连续三轮仅产出第三方噪声，无自有信号。
    // macOS 仅额外排除一个经未插桩 Qt QFuture/QPromise 内部同步的用例。
    // Windows asan 下未插桩 Qt DLL 走 ucrt/RTL 堆而 ASan 用自有分配器，
    // QML 引擎跨模块对象生命周期触发 bad-free。
    let exclude = sanitizer_test_exclude(profile, sanitizer_platform());
    run_ctest_in(
        &native,
        sanitizer_test_env(profile)?,
        &format!("sanitizer {} ctest", profile.name()),
        None,
        None,
        exclude.as_deref(),
    )
}

const TSAN_PROJECT_VIEW_MODEL_SHARED_EXCLUDES: &[&str] = &[
    "ConfirmedImportCompletesThroughBackgroundViewModelRequest",
    "DuplicateImportOrdinalsFollowImportOrderAcrossActivationAndClose",
    "ReopenLoadsWelcomeOnlyAndActivatesSavedRecordOnDemand",
    "FailedLoadRetainsTabAndCloseReleasesActivationState",
    "FillSettingsConfirmAsynchronouslyAndReopenFromRust",
    "StructuredImportFailuresPreserveTheCommittedProjectAndViewport",
    "PreviewsImportsAndPersistsLatestRecord",
    "PreviewCancellationReplacementAndProjectSwitchIgnoreOldResults",
    "PreviewNotificationsCanReplaceOrCancelTheCompletedRequest",
];

const TSAN_PROJECT_VIEW_MODEL_CROSS_PLATFORM_EXCLUDES: &[&str] =
    &["ImportInteractionStateMachineGatesAndSerializesCommands"];

const TSAN_LINUX_PROJECT_VIEW_MODEL_EXCLUDES: &[&str] = &[
    "CreatesOpensRenamesAndSavesThroughRustService",
    "ProjectCommandsReturnAdmissionAndPublishWithoutBlockingGuiEvents",
    "ProjectWaiterStartupFailureNeverAdmitsFilesystemWork",
    "CompletionDoesNotEraseAnErrorFromTheNextReentrantCommand",
    "DestroyingProjectRequestOwnerDoesNotAccessDeletedQtObjects",
    "DisplayPreparationCoalescesSelectionAndRejectsTheOldDisplayResult",
    "GpuSamplingRejectsEmptyFramesAndWaitsForTheTargetScene",
    "DestroyingDisplayPreparationOwnerDoesNotAccessDeletedQtObjects",
    "CreatesAndOpensProjectThroughUnicodeFilesystemPaths",
    "VariableDocumentsPreserveDiagnosticsAndRejectStaleUpdates",
    "OldVariableRequestCannotWriteIntoAnotherProject",
    "VariableWaiterStartupFailureDoesNotSubmitOrFreezeProject",
    "VariableSaveRejectsConflictingSwitchUntilQueuedCompletion",
    "VariableReadCompletionFromOldProjectDoesNotReplaceNewProjection",
    "VariableReadKeepsGuiResponsiveWhileFilesystemIsBlocked",
    "DestroyingVariableRequestOwnerDoesNotWaitForQtCompletion",
    "MapsRustErrorsWithoutCreatingInvalidTargets",
    "ImportProjectionRefreshFailureDoesNotRepeatCommittedImport",
    "DocumentTabsFollowWelcomeImportAndClose",
    "ReorderingAndClosingDocumentsPreservesActiveMeshSelection",
    "ClosedDocumentsReleaseTheirMeshSnapshotsAfterRepeatedSwitching",
    "ClosingLastTabFlipsPlaceholderVisibility",
];

/// 只排除 TSan 无法观察同步边界的具体用例；普通 CTest 和 ASan/UBSan 仍覆盖全部测试。
fn sanitizer_test_exclude(
    profile: SanitizerProfile,
    platform: SanitizerPlatform,
) -> Option<String> {
    match profile {
        SanitizerProfile::Thread => {
            let platform_excludes = match platform {
                SanitizerPlatform::Linux => TSAN_LINUX_PROJECT_VIEW_MODEL_EXCLUDES,
                SanitizerPlatform::MacOs => &[],
                SanitizerPlatform::Windows => return None,
            };
            let names = TSAN_PROJECT_VIEW_MODEL_SHARED_EXCLUDES
                .iter()
                .chain(TSAN_PROJECT_VIEW_MODEL_CROSS_PLATFORM_EXCLUDES)
                .chain(platform_excludes)
                .copied()
                .collect::<Vec<_>>();
            Some(format!(
                "^(Ffi|Qml)\\.|^ProjectViewModelTest\\.({})$",
                names.join("|")
            ))
        }
        SanitizerProfile::Address if platform == SanitizerPlatform::Windows => {
            Some("^Qml\\.".to_owned())
        }
        _ => None,
    }
}

/// sanitizer 测试环境：托管 LLVM bin 前置到 PATH，让运行时报告用配套
/// llvm-symbolizer 符号化；Windows 的 ASan 动态运行库 DLL 由编译器资源
/// 目录解析。LeakSanitizer 在 macOS 默认关闭，显式开启；第三方 SDK/系统
/// 运行时的已知问题按 `tests/lsan-suppressions.txt`（泄漏）与
/// `tests/tsan-suppressions.txt`（竞态，仅 tsan 组合）抑制，自有代码的
/// 问题报告仍然阻断。
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
    if !cfg!(windows) {
        let suppressions = repository_root()?.join("tests/lsan-suppressions.txt");
        environment.push((
            std::ffi::OsString::from("ASAN_OPTIONS"),
            std::ffi::OsString::from("detect_leaks=1"),
        ));
        environment.push((
            std::ffi::OsString::from("LSAN_OPTIONS"),
            std::ffi::OsString::from(format!(
                "suppressions={}",
                suppressions.to_str().ok_or("抑制清单路径不是 UTF-8")?
            )),
        ));
    }
    if profile == SanitizerProfile::Thread {
        let suppressions = repository_root()?.join("tests/tsan-suppressions.txt");
        environment.push((
            std::ffi::OsString::from("TSAN_OPTIONS"),
            std::ffi::OsString::from(format!(
                "suppressions={}",
                suppressions.to_str().ok_or("抑制清单路径不是 UTF-8")?
            )),
        ));
    }
    Ok(environment)
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
    fn tsan_excludes_only_tests_with_uninstrumented_sync_boundaries() {
        fn project_view_model_names(exclude: &str) -> Vec<&str> {
            exclude
                .split_once("ProjectViewModelTest\\.(")
                .map(|(_, names)| names.trim_end_matches(")$").split('|').collect())
                .unwrap_or_default()
        }

        let linux = sanitizer_test_exclude(SanitizerProfile::Thread, SanitizerPlatform::Linux)
            .unwrap_or_default();
        let macos = sanitizer_test_exclude(SanitizerProfile::Thread, SanitizerPlatform::MacOs)
            .unwrap_or_default();
        let linux_names = project_view_model_names(&linux);
        let macos_names = project_view_model_names(&macos);
        for exclude in [&linux, &macos] {
            assert!(exclude.starts_with("^(Ffi|Qml)\\.|^ProjectViewModelTest\\.("));
            assert!(exclude.ends_with(")$"));
        }
        assert_eq!(TSAN_PROJECT_VIEW_MODEL_SHARED_EXCLUDES.len(), 9);
        assert_eq!(TSAN_PROJECT_VIEW_MODEL_CROSS_PLATFORM_EXCLUDES.len(), 1);
        assert_eq!(TSAN_LINUX_PROJECT_VIEW_MODEL_EXCLUDES.len(), 22);
        assert_eq!(linux_names.len(), 32);
        assert_eq!(macos_names.len(), 10);
        for name in TSAN_PROJECT_VIEW_MODEL_SHARED_EXCLUDES {
            assert!(linux_names.contains(name));
            assert!(macos_names.contains(name));
        }
        for name in TSAN_PROJECT_VIEW_MODEL_CROSS_PLATFORM_EXCLUDES {
            assert!(linux_names.contains(name));
            assert!(macos_names.contains(name));
        }
        for name in TSAN_LINUX_PROJECT_VIEW_MODEL_EXCLUDES {
            assert!(linux_names.contains(name));
            assert!(!macos_names.contains(name));
        }

        assert_eq!(
            sanitizer_test_exclude(SanitizerProfile::AddressUndefined, SanitizerPlatform::Linux),
            None
        );
        assert_eq!(
            sanitizer_test_exclude(SanitizerProfile::Address, SanitizerPlatform::Windows)
                .as_deref(),
            Some("^Qml\\.")
        );
        assert_eq!(
            sanitizer_test_exclude(SanitizerProfile::Address, SanitizerPlatform::Linux),
            None
        );
        assert_eq!(
            sanitizer_test_exclude(SanitizerProfile::Thread, SanitizerPlatform::Windows),
            None
        );
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

//! 验证各平台 sanitizer 构建与运行参数。

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
                    assert!(
                        value.to_string_lossy().ends_with(
                            "tests/fixtures/sanitizers/macos-system-frameworks-lsan.supp"
                        )
                    );
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
fn windows_asan_probe_build_targets_the_qml_smoke_executable() {
    let native = Path::new("native build with spaces");
    let command = windows_asan_probe_build_command(native);
    assert_eq!(command.get_program(), "cmake");
    assert_eq!(
        command.get_args().collect::<Vec<_>>(),
        [
            std::ffi::OsStr::new("--build"),
            native.as_os_str(),
            std::ffi::OsStr::new("--target"),
            std::ffi::OsStr::new("caret_shell_load_test"),
        ]
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

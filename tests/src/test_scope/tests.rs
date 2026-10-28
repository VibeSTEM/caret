//! 验证变更路径选择测试范围和检查任务。

use super::*;

#[test]
fn reverse_dependency_closure_is_transitive() {
    let reverse = BTreeMap::from([
        ("base".into(), BTreeSet::from(["middle".into()])),
        ("middle".into(), BTreeSet::from(["app".into()])),
    ]);
    assert_eq!(
        dependency_closure(BTreeSet::from(["base".into()]), &reverse),
        BTreeSet::from(["base".into(), "middle".into(), "app".into()])
    );
}

#[test]
fn local_scope_escalates_to_full_suite_when_impact_is_unknown() -> Result<(), Box<dyn Error>> {
    let plan = Plan {
        packages_l2: BTreeSet::from(["caret-app".into(), "caret-tests".into()]),
        full: true,
        ..Plan::default()
    };
    for scope in ["l1", "l2", "all"] {
        assert_eq!(
            selected_packages(scope, &plan)?,
            plan.packages(),
            "{scope} must not become a false-green partial run"
        );
        assert!(scope_runs_native(scope, &plan));
    }
    Ok(())
}

#[test]
fn documents_are_recognized_only_outside_code_and_test_roots() {
    assert!(is_document_path("ai-docs/task/120.md"));
    assert!(is_document_path("README.md"));
    assert!(!is_document_path("tests/fixtures/readme.md"));
    assert!(!is_document_path("native/app/README.md"));
    assert!(!is_document_path("ai-docs/unregistered-input.js"));
}

#[test]
fn prototype_input_runs_qml_tests_and_unknown_document_inputs_fail_closed()
-> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let prototype = make_plan(&["prototypes/shell/shell.js".into()], &root)?;
    assert!(prototype.code && prototype.native && prototype.qml);
    assert!(prototype.native_labels.contains("qml"));
    assert!(!prototype.full);
    let unknown = make_plan(&["ai-docs/unregistered-input.js".into()], &root)?;
    assert!(unknown.full);
    Ok(())
}

#[test]
fn unknown_modules_and_shared_test_support_fail_closed() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for input in [
        "crates/unregistered-module/src/lib.rs",
        "tests/support/qt/theme_snapshot_helpers.hpp",
    ] {
        let plan = make_plan(&[input.into()], &root)?;
        assert!(
            plan.full && plan.rust && plan.native && plan.qml,
            "{input}: {plan:?}"
        );
        assert!(plan.native_labels.contains("qml"));
        if input.starts_with("crates/") {
            assert!(plan.qml_lint, "{input}: {plan:?}");
            assert_eq!(
                plan.rust_lint_packages.len(),
                cargo_packages(&root)?.len(),
                "{input}: {plan:?}"
            );
        }
    }
    Ok(())
}

#[test]
fn native_analysis_follows_build_and_generated_header_inputs() {
    for input in [
        "qml/CMakeLists.txt",
        "tools/sdk/toolchain.cmake",
        "native/removed_header.hpp",
        "crates/caret-ffi/src/lib.rs",
        "crates/caret-ffi/Cargo.toml",
        "crates/launcher/build.rs",
        "crates/caret-build/src/assets.rs",
    ] {
        let scope = native_scope_for_paths(&[input.into()]);
        if is_cmake_path(input) || input.ends_with(".cmake") {
            assert!(scope.is_empty(), "CMake-only input must skip C++ lint");
        } else {
            assert_eq!(scope, NativeLintScope::All, "{input}");
        }
    }
    let source = "native/adapters/window_chrome_metrics_mac.mm";
    assert_eq!(
        native_scope_for_paths(&[source.into()]),
        NativeLintScope::Sources(BTreeSet::from([source.into()]))
    );
    assert!(native_scope_for_paths(&["crates/caret-app/src/settings/theme.rs".into()]).is_empty());
    assert!(native_scope_for_paths(&["qml/App.qml".into()]).is_empty());
}

#[test]
fn label_regex_is_anchored_and_deterministic() {
    let labels = BTreeSet::from(["qml".into(), "i18n".into()]);
    assert_eq!(label_regex(&labels), "^(i18n|qml)$");
}

#[test]
fn repository_crate_change_selects_direct_package_and_consumers() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let plan = make_plan(&["crates/caret-foundation/Cargo.toml".into()], &root)?;
    assert!(plan.packages_l1.contains("caret-foundation"));
    assert!(plan.packages_l2.is_empty(), "{plan:?}");
    assert!(!plan.native);
    assert!(plan.native_labels.is_empty());
    assert!(!plan.full);
    Ok(())
}

#[test]
fn qml_lint_follows_direct_and_mixed_qml_bridge_changes() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        "crates/caret-app/src/settings/theme.rs",
        "crates/caret-app/src/settings/theme/schema.rs",
        "crates/caret-ffi/src/lib.rs",
    ] {
        assert!(make_plan(&[path.into()], &root)?.qml_lint, "{path}");
    }
    let storage = make_plan(&["crates/caret-app/src/document_storage.rs".into()], &root)?;
    assert!(!storage.qml_lint, "{storage:?}");
    assert!(!storage.native, "{storage:?}");
    let theme = make_plan(&["crates/caret-app/src/settings/theme.rs".into()], &root)?;
    assert!(theme.native && theme.qml, "{theme:?}");

    let mixed = make_plan(
        &["Cargo.toml".into(), "crates/caret-ffi/src/lib.rs".into()],
        &root,
    )?;
    assert!(mixed.full, "{mixed:?}");
    assert!(mixed.qml_lint && mixed.cpp && mixed.qml, "{mixed:?}");
    Ok(())
}

#[test]
fn native_test_source_skips_rust_lint_and_qml_lint() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let plan = make_plan(&["tests/cpp/i18n/qm_load_test.cpp".into()], &root)?;
    assert!(plan.native, "{plan:?}");
    assert!(plan.cpp, "{plan:?}");
    assert!(plan.cpp_lint, "{plan:?}");
    assert!(!plan.rust, "{plan:?}");
    assert!(plan.rust_lint_packages.is_empty(), "{plan:?}");
    assert!(!plan.qml, "{plan:?}");
    assert!(!plan.qml_lint, "{plan:?}");
    assert_eq!(plan.native_labels, BTreeSet::from(["cpp".into()]));

    let shell = make_plan(&["tests/cpp/shell/shell_load_test.cpp".into()], &root)?;
    assert!(shell.native && shell.cpp && shell.qml, "{shell:?}");
    assert!(shell.cpp_lint, "{shell:?}");
    assert!(!shell.rust && !shell.qml_lint, "{shell:?}");
    assert!(shell.rust_lint_packages.is_empty(), "{shell:?}");
    assert_eq!(shell.native_labels, BTreeSet::from(["qml".into()]));

    let qml_cmake = make_plan(&["qml/CMakeLists.txt".into()], &root)?;
    assert!(qml_cmake.qml && qml_cmake.qml_lint, "{qml_cmake:?}");
    assert!(qml_cmake.cmake && qml_cmake.layering, "{qml_cmake:?}");
    assert!(!qml_cmake.cpp_lint, "{qml_cmake:?}");
    let i18n_cmake = make_plan(&["native/i18n/CMakeLists.txt".into()], &root)?;
    assert!(i18n_cmake.cmake && i18n_cmake.layering, "{i18n_cmake:?}");
    assert!(!i18n_cmake.cpp_lint, "{i18n_cmake:?}");
    assert!(i18n_cmake.rust_lint_packages.is_empty(), "{i18n_cmake:?}");
    Ok(())
}

#[test]
fn lint_runner_changes_stay_scoped_to_runner_tests() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let path = "tests/src/lint.rs";
    let plan = make_plan(&[path.into()], &root)?;
    assert!(!plan.full, "{path}: {plan:?}");
    assert!(!plan.qml_lint, "{path}: {plan:?}");
    assert!(!plan.cpp_lint && !plan.cpp_lint_all, "{path}: {plan:?}");
    assert!(plan.packages_l1.contains("caret-tests"), "{plan:?}");
    assert!(!plan.native, "{path}: {plan:?}");
    Ok(())
}

#[test]
fn rust_test_harness_sources_stay_scoped_to_the_runner() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        "tests/Cargo.toml",
        "tests/src/coverage.rs",
        "tests/src/layering.rs",
    ] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(plan.rust, "{path}: {plan:?}");
        assert!(plan.packages_l1.contains("caret-tests"), "{path}: {plan:?}");
        assert!(!plan.full, "{path}: {plan:?}");
        assert!(!plan.native, "{path}: {plan:?}");
        assert!(
            plan.layering == (path == "tests/src/layering.rs" || path == "tests/Cargo.toml"),
            "{path}: {plan:?}"
        );
    }
    let build = make_plan(&["tests/src/build.rs".into()], &root)?;
    assert!(build.native, "{build:?}");
    assert!(build.native_labels.contains("qml"), "{build:?}");
    assert!(!build.full, "{build:?}");
    let selector = make_plan(&["tests/src/test_scope.rs".into()], &root)?;
    assert!(!selector.full && selector.rust, "{selector:?}");
    assert!(selector.packages_l1.contains("caret-tests"), "{selector:?}");
    assert_eq!(
        selector.rust_lint_packages,
        BTreeSet::from(["caret-tests".into()]),
        "{selector:?}"
    );
    assert!(selector.native_labels.is_empty(), "{selector:?}");
    assert!(!selector.cpp_lint, "{selector:?}");
    assert!(!selector.qml_lint, "{selector:?}");
    Ok(())
}

#[test]
fn dslc_and_app_changes_select_their_packages() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let dslc = make_plan(&["crates/caret-dslc/src/main.rs".into()], &root)?;
    assert!(dslc.packages_l1.contains("caret-dslc"), "{dslc:?}");
    assert!(dslc.packages_l2.is_empty(), "{dslc:?}");
    assert!(!dslc.native);

    let app = make_plan(&["crates/caret-app/src/lib.rs".into()], &root)?;
    assert!(app.packages_l1.contains("caret-app"), "{app:?}");
    // 普通应用服务只编译 Rust 消费端，不构建 Qt/CMake launcher。
    assert!(app.packages_l2.contains("caret-ffi"), "{app:?}");
    assert!(!app.packages_l2.contains("caret-launcher"), "{app:?}");
    assert!(!app.native && !app.qml, "{app:?}");
    assert!(!app.full);
    assert!(!app.qml_lint, "{app:?}");
    Ok(())
}

#[test]
fn criterion_benches_select_their_registered_cargo_packages() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for (path, package) in [
        (
            "tests/performance/rust/cpu/dsl_core_criterion.rs",
            "caret-dsl-core",
        ),
        (
            "tests/performance/rust/cpu/layout_criterion.rs",
            "caret-layout",
        ),
        (
            "tests/performance/rust/cpu/novel_document_criterion.rs",
            "caret-engine",
        ),
        (
            "tests/performance/rust/cpu/document_view_input_criterion.rs",
            "caret-app",
        ),
        (
            "tests/performance/rust/cpu/document_open_delete_criterion.rs",
            "caret-app",
        ),
        (
            "tests/performance/rust/cpu/document_history_save_criterion.rs",
            "caret-app",
        ),
    ] {
        assert!(is_rust_package_path(path), "{path}");
        let plan = make_plan(&[path.into()], &root)?;
        assert!(plan.rust, "{path}");
        assert!(plan.packages_l1.contains(package), "{plan:?}");
        assert!(!plan.full, "{path} should stay package-scoped");
    }
    assert!(!is_rust_package_path(
        "tests/performance/qt/cpu/example.cpp"
    ));
    Ok(())
}

#[test]
fn qt_performance_sources_do_not_select_performance_or_native_tests() -> Result<(), Box<dyn Error>>
{
    let root = repository_root()?;
    for path in [
        "tests/performance/qt/cpu/software_canvas_benchmark.cpp",
        "tests/performance/qt/gpu/headless_canvas_benchmark.cpp",
        "tests/performance/qt/canvas_fixture.hpp",
    ] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(!plan.full, "{path}");
        assert!(!plan.native, "{path}");
        assert!(!plan.cpp, "{path}");
        assert!(!plan.qml, "{path}");
    }
    Ok(())
}

#[test]
fn workspace_and_test_harness_changes_skip_qml_lint() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        "tests/Cargo.toml",
        "tests/src/layering.rs",
        ".github/workflows/ci.yml",
        ".github/actions/changed-paths/action.yml",
    ] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(!plan.qml_lint, "{path}: {plan:?}");
    }
    Ok(())
}

#[test]
fn ffi_sources_select_the_ffi_package_and_native_labels() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        "crates/caret-ffi/src/lib.rs",
        "crates/caret-ffi/Cargo.toml",
        "crates/caret-ffi/build.rs",
    ] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(!plan.full, "{path}: {plan:?}");
        assert!(plan.packages_l1.contains("caret-ffi"), "{path}: {plan:?}");
        assert!(plan.native_labels.contains("qml"), "{path}: {plan:?}");
    }
    let outside = make_plan(&["unregistered/outside-any-crate.rs".into()], &root)?;
    assert!(outside.full, "{outside:?}");
    Ok(())
}

#[test]
fn unknown_native_module_selects_all_tests() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let plan = make_plan(&["native/unregistered/src/adapter.cpp".into()], &root)?;
    assert!(plan.full && plan.native && plan.qml, "{plan:?}");
    assert_eq!(plan.native_labels, all_native_labels());
    Ok(())
}

#[test]
fn dsl_build_dependency_selects_native_translation_tests() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let plan = make_plan(&["crates/caret-dsl-core/Cargo.toml".into()], &root)?;
    assert!(plan.packages_l1.contains("caret-dsl-core"));
    assert!(plan.packages_l2.contains("caret-dslc"), "{plan:?}");
    assert!(plan.packages_l2.contains("caret-app"), "{plan:?}");
    assert!(plan.native_labels.contains("i18n"));
    assert!(plan.native_labels.contains("qml"));
    Ok(())
}

#[test]
fn theme_sources_select_the_app_package_and_qml_tests() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        "qml/Themes/gui-default.pa",
        "qml/Themes/dark.pa",
        "qml/Themes/light.pa",
    ] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(plan.rust, "{plan:?}");
        assert!(plan.packages_l1.contains("caret-app"), "{plan:?}");
        assert!(plan.packages_l2.contains("caret-ffi"), "{plan:?}");
        assert!(!plan.packages_l2.contains("caret-launcher"), "{plan:?}");
        assert!(plan.native && plan.qml, "{plan:?}");
        assert!(plan.native_labels.contains("qml"), "{plan:?}");
        assert!(!plan.full, "{plan:?}");
    }
    Ok(())
}

#[test]
fn full_fallback_paths_schedule_every_domain() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    for path in [
        Some("native/CMakeLists.txt"),
        Some("unregistered/new-area/file.rs"),
        None,
    ] {
        let paths = path.map(|path| vec![path.into()]).unwrap_or_default();
        let plan = make_plan(&paths, &root)?;
        assert!(plan.full, "{path:?} must select the full suite");
        assert!(plan.code, "{path:?} must schedule code checks");
        assert!(plan.cargo_deps, "{path:?} must schedule dependency checks");
        assert!(
            plan.rust && plan.native && plan.qml,
            "{path:?} must schedule all domains"
        );
        assert!(plan.layering, "{path:?} must schedule layering checks");
        assert_eq!(plan.packages_l1.len(), 0);
        assert_eq!(plan.packages_l2.len(), cargo_packages(&root)?.len());
        assert_eq!(plan.native_labels, all_native_labels());
        if path.is_none() || path.is_some_and(|path| path.starts_with("unregistered/")) {
            assert!(plan.qml_lint, "{path:?} must retain fail-closed QML lint");
        }
        if path.is_none() {
            assert!(plan.cpp_lint_all && plan.cmake_lint_all, "{plan:?}");
            assert_eq!(plan.rust_lint_packages.len(), cargo_packages(&root)?.len());
        }
    }
    let launcher_build = make_plan(&["crates/launcher/build.rs".into()], &root)?;
    assert!(!launcher_build.full, "{launcher_build:?}");
    assert!(
        launcher_build.cpp && launcher_build.cmake,
        "{launcher_build:?}"
    );
    assert!(launcher_build.native, "{launcher_build:?}");
    Ok(())
}

#[test]
fn public_rust_build_inputs_expand_quality_together_with_tests() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let path = "crates/caret-build/src/assets.rs";
    let plan = make_plan(&[path.into()], &root)?;
    assert!(plan.full, "{path}");
    Ok(())
}

#[test]
fn native_module_change_selects_registered_label_closure() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let plan = make_plan(&["native/i18n/qm_catalog.cpp".into()], &root)?;
    assert!(!plan.full);
    assert_eq!(
        plan.native_labels,
        BTreeSet::from(["cpp".into(), "i18n".into(), "qml".into()])
    );
    let app = make_plan(&["native/viewmodels/theme/theme_catalog.cpp".into()], &root)?;
    assert!(!app.full);
    assert_eq!(app.native_labels, BTreeSet::from(["qml".into()]));
    assert!(!app.qml_lint, "{app:?}");
    for header in [
        "native/adapters/document_path_adapter.hpp",
        "native/adapters/platform_fonts.hpp",
        "native/viewmodels/theme/theme_view_model.hpp",
        "native/adapters/window_chrome_metrics.hpp",
    ] {
        assert!(make_plan(&[header.into()], &root)?.qml_lint, "{header}");
    }
    let i18n = make_plan(&["native/i18n/qm_catalog.cpp".into()], &root)?;
    assert!(!i18n.qml_lint, "{i18n:?}");
    Ok(())
}

#[test]
fn tool_configs_expand_only_their_own_lint_scope() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let clang_format = make_plan(&[".clang-format".into()], &root)?;
    assert!(!clang_format.full);
    assert!(clang_format.cpp_format_all);
    assert!(!clang_format.cpp_lint && !clang_format.qml_lint);

    let cmake = make_plan(&["native/CMakeLists.txt".into()], &root)?;
    assert!(cmake.cmake && cmake.layering);
    assert!(!cmake.cpp_lint && !cmake.qml_lint);

    let clang_tidy = make_plan(&[".clang-tidy".into()], &root)?;
    assert!(clang_tidy.cpp_lint && clang_tidy.cpp_lint_all);
    assert!(!clang_tidy.qml_lint && !clang_tidy.full);

    let cmake_format = make_plan(&[".cmake-format.py".into()], &root)?;
    assert!(cmake_format.cmake && cmake_format.cmake_lint_all);
    assert!(cmake_format.cmake_format_all);
    assert!(!cmake_format.cpp_lint && !cmake_format.qml_lint);

    let cargo_config = make_plan(&["Cargo.toml".into()], &root)?;
    assert!(cargo_config.full, "{cargo_config:?}");
    assert!(cargo_config.cpp && cargo_config.cmake && cargo_config.layering);
    assert!(!cargo_config.cpp_lint && !cargo_config.qml_lint);
    assert!(cargo_config.native && cargo_config.qml);

    let ci = make_plan(&[".github/workflows/ci.yml".into()], &root)?;
    assert!(!ci.full, "{ci:?}");
    assert!(ci.runner_tests, "{ci:?}");
    assert!(!ci.rust && !ci.code, "{ci:?}");
    assert!(ci.rust_lint_packages.is_empty(), "{ci:?}");
    assert!(!ci.cpp_lint, "{ci:?}");

    let mixed = make_plan(
        &[
            ".github/workflows/ci.yml".into(),
            "crates/caret-foundation/src/lib.rs".into(),
        ],
        &root,
    )?;
    assert!(mixed.runner_tests && mixed.rust, "{mixed:?}");
    assert!(mixed.rust_lint_packages.contains("caret-foundation"));
    assert!(!mixed.cpp_lint && !mixed.qml_lint, "{mixed:?}");

    for path in ["resources/i18n/caret-en.pa", "qml/Themes/gui-default.pa"] {
        let plan = make_plan(&[path.into()], &root)?;
        assert!(plan.rust, "{path}: {plan:?}");
        assert!(plan.rust_lint_packages.is_empty(), "{path}: {plan:?}");
    }
    Ok(())
}

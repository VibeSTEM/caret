//! 验证构建数据库的发现和解析行为。

use super::*;

fn entry(file: &str) -> CompileCommand {
    CompileCommand {
        directory: PathBuf::from("/repo"),
        file: file.into(),
        arguments: None,
        command: Some("\"/tools/LLVM 22/clang++\" -c source.cc".into()),
    }
}

#[test]
fn keeps_handwritten_cxx_and_tests_without_generated_code_or_headers() {
    let entries = [
        "native/app/main.cpp",
        "native/app/window_chrome_metrics_mac.mm",
        "tests/cpp/ffi/test.cc",
        "tests/qml/main.cpp",
        "tests/performance/gpu/viewport_benchmark.cpp",
        "crates/caret-ffi/src/ffi_support.cc",
        "target/cxxbridge/lib.rs.cc",
        "native/app/main.h",
        "native-external/file.cc",
    ]
    .map(entry)
    .to_vec();
    let selected = owned(entries, Path::new("/repo"));
    assert_eq!(selected.len(), 6);
    assert!(
        selected
            .iter()
            .any(|entry| entry.file == Path::new("crates/caret-ffi/src/ffi_support.cc"))
    );
}

#[test]
fn handles_compiler_paths_with_spaces_and_rejects_missing_commands() {
    let mut entry = entry("source.cc");
    assert_eq!(
        entry.compiler().unwrap_or_else(|e| panic!("{e}")),
        Path::new("/tools/LLVM 22/clang++")
    );
    entry.arguments = Some(vec!["/tools/compiler with spaces".into(), "-c".into()]);
    assert_eq!(
        entry.compiler().unwrap_or_else(|e| panic!("{e}")),
        Path::new("/tools/compiler with spaces")
    );
    entry.arguments = Some(vec![]);
    assert!(entry.compiler().is_err());
}

#[test]
fn library_model_removes_only_the_selected_dependency_includes() -> Result<(), String> {
    let mut entry = entry("source.cc");
    entry.command = Some(
            r#""/tools/LLVM 22/clang++" -I"/repo/target/qt/include" -isystem "/repo/target/qt/lib/Qt Core" -I /repo/native -I/repo/target/qt-extra -DQT_CORE_LIB=1 -c source.cc"#.into(),
        );
    entry.exclude_include_root(Path::new("/repo/target/qt"))?;
    assert_eq!(
        entry.arguments,
        Some(vec![
            "/tools/LLVM 22/clang++".into(),
            "-I".into(),
            "/repo/native".into(),
            "-I/repo/target/qt-extra".into(),
            "-DQT_CORE_LIB=1".into(),
            "-c".into(),
            "source.cc".into(),
        ])
    );
    assert!(entry.command.is_none());
    entry.arguments = Some(vec![
        "clang-cl.exe".into(),
        "/I/repo/target/qt/include".into(),
        "-imsvc".into(),
        "/sdk/include".into(),
    ]);
    entry.exclude_include_root(Path::new("/repo/target/qt"))?;
    assert_eq!(entry.arguments.as_ref().map(Vec::len), Some(3));
    Ok(())
}

#[test]
fn rejects_system_compiler_even_when_managed_binary_exists() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("caret-db-test-{}", std::process::id()));
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let managed = root.join("managed compiler");
    let system = root.join("system compiler");
    for file in [&managed, &system] {
        std::fs::write(file, "binary").map_err(|e| e.to_string())?;
    }
    let mut command = entry("native/source.cc");
    command.arguments = Some(vec![managed.to_string_lossy().into_owned()]);
    verify(&[command.clone()], &managed)?;
    command.arguments = Some(vec![system.to_string_lossy().into_owned()]);
    assert!(verify(&[command], &managed).is_err());
    assert!(verify(&[], &managed).is_err());
    std::fs::remove_dir_all(root).map_err(|e| e.to_string())
}

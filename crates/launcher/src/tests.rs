//! 验证桌面启动器入口的参数和初始化流程。

#[cfg(unix)]
use super::ExitCode;
use super::{prepared_qt_path, qt_bin_for, run, runtime_path};
use std::path::{Path, PathBuf};

#[test]
fn missing_product_reports_unavailable() {
    let missing = Path::new("/caret/definitely/missing/caret");
    let error = match run(Some(missing), &[]) {
        Ok(code) => panic!("缺失产物应返回错误，实际退出码 {code:?}"),
        Err(error) => error,
    };
    assert!(error.contains("native 产物不存在"), "{error}");
}

#[test]
fn absent_injection_reports_configuration_error() {
    let error = match run(None, &[]) {
        Ok(code) => panic!("未注入产物路径应返回错误，实际退出码 {code:?}"),
        Err(error) => error,
    };
    assert!(error.contains("CARET_NATIVE_BIN"), "{error}");
}

#[cfg(unix)]
#[test]
fn forwards_child_exit_code() {
    let code = match run(Some(Path::new("/usr/bin/false")), &[]) {
        Ok(code) => code,
        Err(error) => panic!("无法转发 /usr/bin/false：{error}"),
    };
    assert_eq!(code, ExitCode::from(1));
}

#[cfg(unix)]
#[test]
fn forwards_child_success() {
    let code = match run(Some(Path::new("/usr/bin/true")), &[]) {
        Ok(code) => code,
        Err(error) => panic!("无法转发 /usr/bin/true：{error}"),
    };
    assert_eq!(code, ExitCode::SUCCESS);
}

#[test]
fn qt_bin_requires_windows_target_and_injection() {
    assert_eq!(qt_bin_for(Some("/qt/bin"), false), None);
    assert_eq!(
        qt_bin_for(Some("/qt/bin"), true),
        Some(PathBuf::from("/qt/bin"))
    );
    assert_eq!(qt_bin_for(None, true), None);
    assert_eq!(qt_bin_for(Some(""), true), None);
}

#[test]
fn runtime_path_prepends_front_directory() -> Result<(), std::env::JoinPathsError> {
    let inherited = std::env::join_paths([Path::new("/a"), Path::new("/b")])?;
    let path = runtime_path(PathBuf::from("/qt/bin"), Some(inherited))?;
    let paths: Vec<_> = std::env::split_paths(&path).collect();
    assert_eq!(
        paths,
        [Path::new("/qt/bin"), Path::new("/a"), Path::new("/b")]
    );
    Ok(())
}

#[test]
fn runtime_path_survives_missing_inherited_path() -> Result<(), std::env::JoinPathsError> {
    let path = runtime_path(PathBuf::from("/qt/bin"), None)?;
    assert_eq!(
        std::env::split_paths(&path).collect::<Vec<_>>(),
        vec![Path::new("/qt/bin")]
    );
    Ok(())
}

#[test]
fn qt_path_rejects_missing_runtime_directory() {
    let missing = std::env::temp_dir().join(format!("caret-missing-qt-{}", std::process::id()));
    let error = match prepared_qt_path(&missing, None) {
        Ok(path) => panic!("缺失运行库目录应报错，实际 {path:?}"),
        Err(error) => error,
    };
    assert!(error.contains("托管 Qt 运行库目录不存在"), "{error}");
}

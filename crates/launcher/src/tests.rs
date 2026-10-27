//! 验证桌面启动器入口的参数和初始化流程。

use super::{ExitCode, run};
use std::path::Path;

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

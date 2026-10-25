//! Workspace 测试的 native 聚合入口；断言仍由 CTest 和 qmllint 负责。

use std::error::Error;
use std::process::Command;

#[test]
fn native_tests() -> Result<(), Box<dyn Error>> {
    let status = Command::new(env!("CARGO_BIN_EXE_caret-tests"))
        .arg("native-test")
        .status()?;
    if !status.success() {
        return Err(format!("native 聚合失败：{status}").into());
    }
    Ok(())
}

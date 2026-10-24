//! runner 的仓库 / Cargo / native 目录；只消费 build.rs 的真实配置。

use std::error::Error;
use std::path::Path;

const TESTS_MANIFEST_DIR: &str = env!("CARGO_MANIFEST_DIR");

pub(super) fn target_root() -> &'static Path {
    Path::new(env!("CARET_TEST_TARGET_DIR"))
}

pub(super) fn native_build_dir() -> &'static Path {
    Path::new(env!("CARET_TEST_NATIVE_DIR"))
}

pub(super) fn repository_root() -> Result<&'static Path, Box<dyn Error>> {
    Path::new(TESTS_MANIFEST_DIR)
        .parent()
        .ok_or_else(|| "tests package 必须位于仓库根目录下".into())
}

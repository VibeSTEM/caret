//! 识别链接器输出中的 caret-ffi Rust 静态库。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

/// Cargo staticlib 的文件名。`caret_ffi` 与带哈希的 `caret_ffi-<hash>` 都算。
/// cxx 的 `caret_ffi_bridge` 归档不含 Rust，不能当成链接产物。
pub(crate) fn is_caret_ffi_staticlib(name: &str) -> bool {
    let Some(stem) = name
        .strip_suffix(".a")
        .or_else(|| name.strip_suffix(".lib"))
    else {
        return false;
    };
    let stem = stem.strip_prefix("lib").unwrap_or(stem);
    stem == "caret_ffi" || stem.starts_with("caret_ffi-")
}

#[cfg(test)]
mod tests {
    use super::is_caret_ffi_staticlib;

    #[test]
    fn accepts_rust_staticlib_and_rejects_cxx_bridge_archive() {
        assert!(is_caret_ffi_staticlib("libcaret_ffi.a"));
        assert!(is_caret_ffi_staticlib("caret_ffi.lib"));
        assert!(is_caret_ffi_staticlib("libcaret_ffi-46f6bde6fe8c525a.a"));
        assert!(!is_caret_ffi_staticlib("libcaret_ffi_bridge.a"));
        assert!(!is_caret_ffi_staticlib("libcaret_ffi_bridge-abc.a"));
        assert!(!is_caret_ffi_staticlib("libcaret_ffi.rlib"));
    }
}

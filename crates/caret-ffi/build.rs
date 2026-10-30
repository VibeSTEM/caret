//! 准备 caret-ffi 构建所需的受管工具和 CXX 桥接配置。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use caret_build as provision;

fn main() -> Result<(), io::Error> {
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARET_USE_SYSTEM_TOOLS");
    println!("cargo:rerun-if-env-changed=CARET_TOOLCHAIN_ROOT");
    println!("cargo:rerun-if-env-changed=CC");
    println!("cargo:rerun-if-env-changed=CXX");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let out_dir = PathBuf::from(
        env::var_os("OUT_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Cargo 必须设置 OUT_DIR"))?,
    );
    let llvm = provision::resolve_llvm_compilers().map_err(io::Error::other)?;
    let compiler = match (
        env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc"),
        llvm.clang_cl.as_ref(),
    ) {
        (true, Some(path)) => path,
        _ => &llvm.clangxx,
    };

    let mut builder = cxx_build::bridge("src/lib.rs");
    let cxx_standard_flag = if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        "/std:c++20"
    } else {
        "-std=c++20"
    };
    let cxx_exception_flag =
        (env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")).then_some("/EHsc");
    let sdk_env = provision::windows_sdk_env(&env::var("TARGET").map_err(io::Error::other)?)
        .map_err(io::Error::other)?;
    for (key, value) in sdk_env {
        builder.env(key, value);
    }
    if let Some(sdk) = provision::macos_sdk().map_err(io::Error::other)? {
        let deployment = env::var("MACOSX_DEPLOYMENT_TARGET")
            .unwrap_or_else(|_| provision::MACOS_DEPLOYMENT_TARGET.to_owned());
        builder
            .flag("-isysroot")
            .flag(sdk.as_os_str())
            .flag(format!("-mmacosx-version-min={deployment}"))
            .flag("-nostdinc++")
            .flag("-isystem")
            .flag(sdk.join("usr/include/c++/v1").as_os_str());
    }
    builder
        .compiler(compiler)
        .flag_if_supported(cxx_standard_flag);
    if let Some(flag) = cxx_exception_flag {
        builder.flag(flag);
    }
    builder.compile("caret_ffi_bridge");

    let tool = builder.get_compiler();
    let directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| io::Error::other("Cargo 未设置 CARGO_MANIFEST_DIR"))?,
    );
    let commands = builder
        .get_files()
        .map(|file| {
            let mut arguments = vec![tool.path().to_string_lossy().into_owned()];
            arguments.extend(
                tool.args()
                    .iter()
                    .map(|arg| arg.to_string_lossy().into_owned()),
            );
            arguments.push(if tool.is_like_msvc() { "/c" } else { "-c" }.into());
            arguments.push(file.to_string_lossy().into_owned());
            provision::database::CompileCommand {
                directory: directory.clone(),
                file: file.to_path_buf(),
                arguments: Some(arguments),
                command: None,
            }
        })
        .collect::<Vec<_>>();
    let database = out_dir.join("compile_commands.json");
    provision::database::write(&database, &commands).map_err(io::Error::other)?;
    println!("cargo:compile_database={}", database.display());

    let generated_header = find_generated_header(&out_dir).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("cxx-build 没有在 {} 生成 FFI 头文件", out_dir.display()),
        )
    })?;
    let cxx_include = out_dir.join("cxxbridge").join("include");
    fs::create_dir_all(&cxx_include)?;
    let public_header = cxx_include.join("caret_ffi.h");
    fs::copy(&generated_header, &public_header).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "复制 {} 到 {} 失败：{error}",
                generated_header.display(),
                public_header.display()
            ),
        )
    })?;
    println!("cargo:include={}", cxx_include.display());
    Ok(())
}

fn find_generated_header(root: &Path) -> Option<PathBuf> {
    let mut directories = vec![root.to_owned()];
    while let Some(directory) = directories.pop() {
        let entries = fs::read_dir(&directory).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path.file_name().is_some_and(|name| name == "lib.rs.h") {
                return Some(path);
            }
        }
    }
    None
}

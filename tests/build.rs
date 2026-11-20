use std::env;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("caret-tests build script 失败：{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| "Cargo 未设置 OUT_DIR".to_string())?);
    let target_root = caret_build::target_root(&out_dir)?;
    let target = env::var("TARGET").map_err(|e| e.to_string())?;
    // 格式工具由 runner 通过显式 --target 构建，因此路径总带宿主 triple。
    let cargo_target_dir = target_root.join(&target);
    let profile = env::var("PROFILE").map_err(|error| format!("Cargo 未设置 PROFILE：{error}"))?;
    let build_type = match profile.as_str() {
        "debug" => "Debug",
        "release" => "Release",
        other => return Err(format!("未知 PROFILE '{other}'，无法映射 CMAKE_BUILD_TYPE")),
    };

    let native_dir = target_root.join("native").join(&profile);

    println!(
        "cargo:rustc-env=CARET_TEST_TARGET_DIR={}",
        target_root.display()
    );
    println!(
        "cargo:rustc-env=CARET_TEST_CARGO_TARGET_DIR={}",
        cargo_target_dir.display()
    );
    println!(
        "cargo:rustc-env=CARET_TEST_NATIVE_DIR={}",
        native_dir.display()
    );
    println!("cargo:rustc-env=CARET_TEST_BUILD_TYPE={build_type}");
    println!("cargo:rustc-env=CARET_TEST_HOST={target}");
    Ok(())
}

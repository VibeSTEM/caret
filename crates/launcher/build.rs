//! Cargo 调度 CMake。CMake 消费已经生成的 staticlib，本脚本不再启动 Cargo。
//!
//! 产物在 `target/native/<profile>`。Qt 安装在 `.toolchain/qt`，不写入 `target/`。

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::SystemTime;

use caret_build as provision;
use caret_dsl_core::{
    ArtifactSource, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_SOURCES, MAX_SOURCE_BYTES, parse_artifacts,
};

#[path = "src/ffi_names.rs"]
mod ffi_names;

const RERUN_PATHS: &[&str] = &[
    "build.rs",
    "../../native",
    "../../tests/cpp",
    "../../qml",
    "../../resources/i18n",
    "../caret-ffi",
];

const RERUN_ENVS: &[&str] = &[
    "CMAKE",
    "CARET_USE_SYSTEM_TOOLS",
    "CARET_TOOLCHAIN_ROOT",
    "CARET_QT_MIRROR",
    "CARET_GTEST_ROOT",
    "CARET_GTEST_URL",
    "CARET_GTEST_SHA256",
    "CC",
    "CXX",
    "CLANG_FORMAT",
    "CMAKE_GENERATOR",
    "CMAKE_GENERATOR_PLATFORM",
    "CMAKE_PREFIX_PATH",
    "CARET_ENABLE_PERFORMANCE",
    "CARET_NATIVE_COVERAGE",
    "CARET_NATIVE_SANITIZER",
    "DEVELOPER_DIR",
    "MACOSX_DEPLOYMENT_TARGET",
];

fn main() -> ExitCode {
    for path in RERUN_PATHS {
        println!("cargo:rerun-if-changed={path}");
    }
    for key in RERUN_ENVS {
        println!("cargo:rerun-if-env-changed={key}");
    }

    match orchestrate() {
        Ok(product) => {
            println!("cargo:rustc-env=CARET_NATIVE_BIN={}", product.display());
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("caret-launcher 构建脚本失败：{message}");
            ExitCode::FAILURE
        }
    }
}

fn orchestrate() -> Result<PathBuf, String> {
    let manifest_dir = required_var("CARGO_MANIFEST_DIR")?;
    let out_dir = PathBuf::from(required_var("OUT_DIR")?);
    let profile = required_var("PROFILE")?;
    let build_type = match profile.as_str() {
        "debug" => "Debug",
        "release" => "Release",
        other => return Err(format!("未知 PROFILE '{other}'，无法映射 CMAKE_BUILD_TYPE")),
    };

    let native_dir = PathBuf::from(&manifest_dir).join("../../native");
    if !native_dir.is_dir() {
        return Err(format!("native 目录不可达：{}", native_dir.display()));
    }
    let target_root = provision::target_root(&out_dir)?;
    let sanitizer = std::env::var("CARET_NATIVE_SANITIZER").unwrap_or_default();
    let mut native_profile = if std::env::var("CARET_NATIVE_COVERAGE").as_deref() == Ok("1") {
        format!("{profile}-coverage")
    } else {
        profile.clone()
    };
    if !sanitizer.is_empty() {
        native_profile = format!("{native_profile}-sanitizer-{}", sanitizer.replace(',', "-"));
    }
    let binary_dir = target_root.join("native").join(&native_profile);

    let i18n_dir = PathBuf::from(&manifest_dir).join("../../resources/i18n");
    let ts_dir = binary_dir.join("generated/i18n-ts");
    emit_translation_sources(&i18n_dir, &ts_dir)?;

    let staging = provision::ensure_qt()?;
    let qt_provision = staging
        .parent()
        .ok_or_else(|| format!("Qt staging 没有父目录：{}", staging.display()))?;
    let cmake = provision::resolve_cmake()?;
    let llvm = provision::resolve_llvm_compilers()?;
    if std::env::var("CMAKE_GENERATOR").is_ok_and(|value| value != "Ninja") {
        return Err("统一构建仅支持 Ninja，以保证三平台导出真实 compile_commands.json".into());
    }
    let ninja = provision::resolve_ninja(&cmake)?;
    let target_triple = required_var("TARGET")?;
    let sdk_env = provision::windows_sdk_env(&target_triple)?;
    let (ffi_include, ffi_staticlib) = ffi_artifacts(&out_dir)?;

    let cxx_compiler = if cfg!(windows) {
        llvm.clang_cl.as_ref().ok_or("LLVM 缺少 clang-cl")?
    } else {
        &llvm.clangxx
    };
    let c_compiler = if cfg!(windows) {
        cxx_compiler
    } else {
        &llvm.clang
    };

    let mut configure = Command::new(&cmake);
    configure
        .arg("-S")
        .arg(&native_dir)
        .arg("-B")
        .arg(&binary_dir)
        .arg("-G")
        .arg("Ninja")
        .arg(format!("-DCMAKE_BUILD_TYPE={build_type}"))
        .arg("-DCARET_ENABLE_APP=ON")
        .arg(format!(
            "-DCARET_TOOLCHAIN_ROOT={}",
            provision::toolchain_root()?.display()
        ))
        .arg(format!("-DCARET_TARGET_TRIPLE={target_triple}"))
        .arg(format!("-DCARET_FFI_INCLUDE_DIR={}", ffi_include.display()))
        .arg(format!(
            "-DCARET_FFI_STATIC_LIB={}",
            ffi_staticlib.display()
        ))
        .arg(format!("-DQT_PROVISION_DIR={}", qt_provision.display()))
        .arg(format!("-DCARET_I18N_TS_DIR={}", ts_dir.display()))
        .envs(sdk_env.clone())
        .arg(format!("-DCMAKE_C_COMPILER={}", c_compiler.display()))
        .arg(format!("-DCMAKE_CXX_COMPILER={}", cxx_compiler.display()))
        .arg(format!("-DCMAKE_MAKE_PROGRAM={}", ninja.display()))
        .arg(format!("-DCARET_LLVM_VERSION={}", provision::LLVM_VERSION))
        .arg(format!(
            "-DCARET_USE_SYSTEM_TOOLS={}",
            if provision::use_system_tools() {
                "ON"
            } else {
                "OFF"
            }
        ))
        .arg(format!(
            "-DCARET_ENABLE_COVERAGE={}",
            if std::env::var("CARET_NATIVE_COVERAGE").as_deref() == Ok("1") {
                "ON"
            } else {
                "OFF"
            }
        ))
        .arg(format!(
            "-DCARET_ENABLE_PERFORMANCE={}",
            if std::env::var("CARET_ENABLE_PERFORMANCE").as_deref() == Ok("1") {
                "ON"
            } else {
                "OFF"
            }
        ))
        .arg(format!("-DCARET_SANITIZER={sanitizer}"));
    for key in ["CARET_GTEST_ROOT", "CARET_GTEST_URL", "CARET_GTEST_SHA256"] {
        // 每次都显式传值，移除环境覆盖后不得继续沿用旧 CMake cache。
        configure.arg(format!(
            "-D{key}={}",
            std::env::var(key).unwrap_or_default()
        ));
    }
    if let Some(sdk) = provision::macos_sdk()? {
        configure.arg(format!("-DCMAKE_OSX_SYSROOT={}", sdk.display()));
        let deployment = match std::env::var("MACOSX_DEPLOYMENT_TARGET") {
            Ok(version) => version,
            Err(_) => provision::MACOS_DEPLOYMENT_TARGET.to_owned(),
        };
        configure.arg(format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={deployment}"));
    }
    run_step("configure", &mut configure)?;

    let mut build = Command::new(&cmake);
    let mut test_env = provision::native_test_env(&target_triple)?;
    if cfg!(windows) && !sanitizer.is_empty() {
        let runtime = provision::compiler_rt_dll_dir(&llvm)?;
        test_env = provision::prepend_path(test_env, vec![runtime])?;
    }
    build
        .envs(test_env)
        .arg("--build")
        .arg(&binary_dir)
        .arg("--config")
        .arg(build_type);
    run_step("build", &mut build)?;

    let ffi_database = PathBuf::from(required_var("DEP_CARET_FFI_COMPILE_DATABASE")?);
    let mut commands = provision::database::read(&binary_dir.join("compile_commands.json"))?;
    commands.extend(provision::database::read(&ffi_database)?);
    let root = PathBuf::from(&manifest_dir)
        .parent()
        .and_then(Path::parent)
        .ok_or("launcher 必须位于 crates/launcher")?
        .to_path_buf();
    let quality_dir = binary_dir.join("quality");
    fs::create_dir_all(&quality_dir).map_err(|error| error.to_string())?;
    let owned = provision::database::owned(commands.clone(), &root);
    provision::database::write(&quality_dir.join("compile_commands.json"), &owned)?;
    let mut cppcheck: Vec<_> = owned
        .into_iter()
        .filter(|entry| entry.source().extension().is_none_or(|ext| ext != "mm"))
        .collect();
    cppcheck.extend(commands.into_iter().filter(|entry| {
        entry
            .file
            .file_name()
            .is_some_and(|name| name == "mocs_compilation.cpp" || name == "lib.rs.cc")
    }));
    provision::database::write(&quality_dir.join("cppcheck.json"), &cppcheck)?;

    println!("cargo:rustc-env=CARET_CMAKE_EXECUTABLE={}", cmake.display());
    println!(
        "cargo:rustc-env=CARET_NATIVE_BUILD_DIR={}",
        binary_dir.display()
    );
    println!("cargo:rustc-env=CARET_NATIVE_BUILD_TYPE={build_type}");

    locate_product(&binary_dir, build_type, &provision::exe_name("caret"))
}

fn locate_product(binary_dir: &Path, build_type: &str, exe_name: &str) -> Result<PathBuf, String> {
    let app_dir = binary_dir.join("app");
    let candidates = [
        app_dir
            .join("caret.app")
            .join("Contents")
            .join("MacOS")
            .join(exe_name),
        app_dir.join(exe_name),
        app_dir.join(build_type).join(exe_name),
        binary_dir.join(build_type).join(exe_name),
        binary_dir.join(exe_name),
    ];
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            let expected = candidates
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(" 或 ");
            format!("CMake 构建成功但未找到产物（尝试：{expected}）")
        })
}

fn ffi_artifacts(out_dir: &Path) -> Result<(PathBuf, PathBuf), String> {
    let include = std::env::var_os("DEP_CARET_FFI_INCLUDE")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo 未提供 DEP_CARET_FFI_INCLUDE，caret-ffi 桥接头不可用".to_owned())?;
    if !include.join("caret_ffi.h").is_file() {
        return Err(format!(
            "caret-ffi 生成头不存在：{}",
            include.join("caret_ffi.h").display()
        ));
    }

    let profile_dir = out_dir.ancestors().nth(3).ok_or_else(|| {
        format!(
            "无法从 launcher OUT_DIR 推导 profile：{}",
            out_dir.display()
        )
    })?;
    let deps_dir = profile_dir.join("deps");
    let mut candidates = Vec::new();
    for directory in [profile_dir, deps_dir.as_path()] {
        let entries = fs::read_dir(directory)
            .map_err(|error| format!("读取 caret-ffi 产物目录 {}：{error}", directory.display()))?;
        candidates.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|value| value.to_str())
                        .is_some_and(ffi_names::is_caret_ffi_staticlib)
                }),
        );
    }
    candidates.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH)
    });
    let staticlib = candidates.into_iter().next_back().ok_or_else(|| {
        format!(
            "未找到 caret-ffi staticlib（检查 {} 与 {}）",
            profile_dir.display(),
            deps_dir.display()
        )
    })?;
    Ok((include, staticlib))
}

fn emit_translation_sources(i18n_dir: &Path, ts_dir: &Path) -> Result<(), String> {
    let entries = fs::read_dir(i18n_dir)
        .map_err(|error| format!("读取 i18n 目录 {} 失败：{error}", i18n_dir.display()))?;
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|error| format!("读取 i18n 目录项失败：{error}"))?
            .path();
        if path.is_file() && path.extension().is_some_and(|extension| extension == "pa") {
            if paths.len() == MAX_ARTIFACT_SOURCES {
                return Err(format!(
                    "{} 下的 .pa 文件超过来源上限 {MAX_ARTIFACT_SOURCES}",
                    i18n_dir.display()
                ));
            }
            paths.push(path);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(format!(
            "{} 下没有 .pa 字典；语言字典缺失时构建不支持继续",
            i18n_dir.display()
        ));
    }
    let mut loaded = Vec::with_capacity(paths.len());
    let mut total_bytes = 0usize;
    for path in paths {
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| format!("字典文件名不是有效 UTF-8：{}", path.display()))?;
        let remaining_bytes = MAX_ARTIFACT_BYTES - total_bytes;
        let mut bytes = Vec::new();
        File::open(&path)
            .map_err(|error| format!("打开 {} 失败：{error}", path.display()))?
            .take(remaining_bytes.min(MAX_SOURCE_BYTES) as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(format!(
                "pa.source_too_large: {} 超过单文件上限 {MAX_SOURCE_BYTES} 字节",
                path.display()
            ));
        }
        if bytes.len() > remaining_bytes {
            return Err(format!(
                "i18n 字典总字节数超过上限 {MAX_ARTIFACT_BYTES}（读取 {} 时）",
                path.display()
            ));
        }
        total_bytes += bytes.len();
        let source = String::from_utf8(bytes).map_err(|error| {
            format!(
                "pa.invalid_utf8 at byte {} in {}",
                error.utf8_error().valid_up_to(),
                path.display()
            )
        })?;
        loaded.push((format!("resources/i18n/{file_name}"), source));
    }

    let snapshot = parse_artifacts(
        loaded
            .iter()
            .map(|(source_id, source)| ArtifactSource { source_id, source }),
    )
    .map_err(|error| error.to_string())?;
    let generated = snapshot
        .emit_translation_sources()
        .map_err(|error| error.to_string())?;
    let mut files = BTreeMap::new();
    for output in generated {
        if files
            .insert(
                output.file_name().to_owned(),
                output.contents().as_bytes().to_vec(),
            )
            .is_some()
        {
            return Err(format!(
                "多个语言 Artifact 输出同名 TS：{}",
                output.file_name()
            ));
        }
    }
    provision::replace_directory_with_files(ts_dir, &files)
}

fn run_step(name: &str, command: &mut Command) -> Result<(), String> {
    match command.status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(match status.code() {
            Some(code) => format!("CMake {name} 失败（退出码 {code}），诊断见上方输出"),
            None => format!("CMake {name} 被信号终止，诊断见上方输出"),
        }),
        Err(error) => Err(format!(
            "无法执行 {:?}：{error}。请检查托管工具缓存与平台 SDK；参见 ai-docs/standards/dependency-acquisition.md",
            command.get_program()
        )),
    }
}

fn required_var(name: &str) -> Result<String, String> {
    std::env::var(name).map_err(|_| format!("Cargo 未提供环境变量 {name}，构建脚本运行环境异常"))
}

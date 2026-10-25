//! 手动桌面 Runtime 安装；SDK 独立生产入口位于 sdk.rs。

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

use crate::command::run;
use crate::paths::{native_build_dir, repository_root, target_root};

pub(super) fn desktop(arguments: Vec<String>) -> Result<(), Box<dyn Error>> {
    if arguments.len() > 1 || arguments.first().is_some_and(|arg| arg.starts_with('-')) {
        return Err(
            "用法：cargo caret-package [输出目录]；Release 用 cargo run --locked --release -p caret-tests -- package [输出目录]".into(),
        );
    }
    crate::build::build_launcher()?;
    let root = repository_root()?;
    let prefix = arguments.first().map(PathBuf::from).unwrap_or_else(|| {
        target_root()
            .join("package")
            .join(if env!("CARET_TEST_BUILD_TYPE") == "Release" {
                "release"
            } else {
                "debug"
            })
    });
    let prefix = if prefix.is_absolute() {
        prefix
    } else {
        root.join(prefix)
    };
    // 不清除任意用户目录；拒绝非空目标，防止旧文件混入产物。
    if prefix.exists() && fs::read_dir(&prefix)?.next().is_some() {
        return Err(format!("安装目录非空，请使用新的输出目录：{}", prefix.display()).into());
    }
    collect_licenses(&native_build_dir().join("package-licenses"))?;
    let mut install = Command::new(caret_build::resolve_cmake()?);
    install.envs(caret_build::native_test_env(env!("CARET_TEST_HOST"))?);
    install
        .args(["--install"])
        .arg(native_build_dir())
        .args([
            "--config",
            env!("CARET_TEST_BUILD_TYPE"),
            "--component",
            "Runtime",
            "--prefix",
        ])
        .arg(&prefix);
    run("安装可搬迁桌面目录", install)?;
    println!("Caret 安装目录：{}", prefix.display());
    Ok(())
}

fn collect_licenses(destination: &Path) -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--locked",
            "--format-version",
            "1",
            "--filter-platform",
            env!("CARET_TEST_HOST"),
        ])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "无法读取许可来源：{}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    if destination.exists() {
        fs::remove_dir_all(destination)?;
    }
    fs::create_dir_all(destination)?;
    fs::copy(root.join("LICENSE"), destination.join("Caret-LICENSE"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("Cargo metadata 缺少 packages")?;
    for package in packages {
        if package["source"].is_null() {
            continue;
        }
        let name = package["name"].as_str().ok_or("package 缺少 name")?;
        let version = package["version"].as_str().ok_or("package 缺少 version")?;
        let manifest = Path::new(
            package["manifest_path"]
                .as_str()
                .ok_or("package 缺少 manifest_path")?,
        );
        let source = manifest.parent().ok_or("manifest 缺少目录")?;
        let target = destination.join(format!("{name}-{version}"));
        fs::create_dir_all(&target)?;
        let notice = serde_json::json!({
            "name": name,
            "version": version,
            "license": package["license"],
            "repository": package["repository"],
            "source": package["source"],
        });
        fs::write(
            target.join("source.json"),
            serde_json::to_vec_pretty(&notice)?,
        )?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let file = entry.path();
            let name = entry.file_name();
            let upper = name.to_string_lossy().to_ascii_uppercase();
            if file.is_file()
                && ["LICENSE", "COPYING", "NOTICE"]
                    .iter()
                    .any(|prefix| upper.starts_with(prefix))
            {
                fs::copy(file, target.join(name))?;
            }
        }
        if let Some(license) = package["license_file"].as_str() {
            let path = source.join(license);
            if path.is_file() {
                fs::copy(path, target.join("declared-license"))?;
            }
        }
    }
    Ok(())
}

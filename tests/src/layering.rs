//! Rust 与 CMake 仓库内依赖边的架构 lint。

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::process::Command;

use serde_json::Value;

use crate::build::build_launcher;
use crate::command::run;
use crate::paths::{repository_root, target_root};

const ALLOWED_RUST_EDGES: &[(&str, &[&str])] = &[
    ("caret-foundation", &[]),
    ("caret-build", &[]),
    ("caret-dsl-core", &[]),
    ("caret-math", &[]),
    ("caret-doc", &[]),
    ("caret-layout", &["caret-doc", "caret-math"]),
    (
        "caret-engine",
        &["caret-doc", "caret-layout", "caret-structure", "caret-math"],
    ),
    ("caret-structure", &["caret-doc"]),
    ("caret-dslc", &["caret-dsl-core"]),
    (
        "caret-app",
        &["caret-engine", "caret-doc", "caret-dsl-core"],
    ),
    ("caret-tests", &["caret-app", "caret-build", "caret-doc"]),
    // 运行时只依赖 caret-app 读取内置主题。build script 使用 caret-build 定位托管 Clang。
    ("caret-ffi", &["caret-app", "caret-build"]),
    (
        "caret-launcher",
        &["caret-ffi", "caret-build", "caret-dsl-core"],
    ),
];

pub(super) fn run_lint() -> Result<(), Box<dyn Error>> {
    check()?;
    build_launcher()?;
    Ok(())
}

pub(super) fn check() -> Result<(), Box<dyn Error>> {
    check_cargo_graph()?;
    check_cmake_fixture()?;
    Ok(())
}

pub(super) fn check_cargo_graph() -> Result<(), Box<dyn Error>> {
    let root = repository_root()?;
    let output = Command::new("cargo")
        .args(["metadata", "--locked", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata 失败（{}）：{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let metadata: Value = serde_json::from_slice(&output.stdout)?;
    let edges = validate_cargo_metadata(&metadata)
        .map_err(|error| format!("Cargo 分层检查失败：{error}"))?;
    for (source, dependency) in edges {
        println!("Cargo 允许依赖：{source} -> {dependency}");
    }
    Ok(())
}

fn validate_cargo_metadata(metadata: &Value) -> Result<BTreeSet<(String, String)>, String> {
    let member_ids = metadata
        .get("workspace_members")
        .and_then(Value::as_array)
        .ok_or_else(|| "metadata 缺少 workspace_members 数组".to_owned())?
        .iter()
        .map(|id| {
            id.as_str()
                .map(str::to_owned)
                .ok_or_else(|| "workspace member id 不是字符串".to_owned())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    let packages = metadata
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| "metadata 缺少 packages 数组".to_owned())?;

    let mut package_names = BTreeMap::new();
    for package in packages {
        let id = required_string(package, "id")?;
        if !member_ids.contains(id) {
            continue;
        }
        let name = required_string(package, "name")?;
        package_names.insert(id.to_owned(), name.to_owned());
    }
    if let Some(missing) = member_ids
        .iter()
        .find(|id| !package_names.contains_key(*id))
    {
        return Err(format!("workspace member 在 packages 中缺失：{missing}"));
    }
    let workspace_names = package_names.values().cloned().collect::<BTreeSet<_>>();
    let mut edges = BTreeSet::new();

    for package in packages {
        let id = required_string(package, "id")?;
        let Some(source) = package_names.get(id) else {
            continue;
        };
        let allowed = ALLOWED_RUST_EDGES
            .iter()
            .find_map(|(name, edges)| (*name == source).then_some(*edges))
            .ok_or_else(|| format!("workspace crate 未登记分层：{source}"))?;
        let dependencies = package
            .get("dependencies")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("{source} 缺少 dependencies 数组"))?;
        for dependency in dependencies {
            let destination = required_string(dependency, "name")?;
            if !workspace_names.contains(destination) {
                continue;
            }
            if !allowed.contains(&destination) {
                let kind = dependency
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("normal");
                return Err(format!(
                    "禁止的 workspace 依赖边：{source} -> {destination}（{kind}）"
                ));
            }
            edges.insert((source.to_owned(), destination.to_owned()));
        }
    }
    Ok(edges)
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("metadata 字段 {key} 缺失或不是字符串"))
}

fn check_cmake_fixture() -> Result<(), Box<dyn Error>> {
    let cmake = caret_build::resolve_cmake()?;
    let script = repository_root()?.join("native/cmake/tests/check-layering.cmake");
    if !script.is_file() {
        return Err(format!("CMake layering fixture 不存在：{}", script.display()).into());
    }
    let mut command = Command::new(&cmake);
    command
        .arg(format!("-DCMAKE_COMMAND={}", cmake.display()))
        .arg(format!(
            "-DCARET_FIXTURE_ROOT={}",
            target_root().join("caret-layering-fixtures").display()
        ))
        .arg("-P")
        .arg(&script)
        .current_dir(repository_root()?);
    run("CMake layering fixture", command)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::validate_cargo_metadata;

    #[test]
    fn accepts_registered_workspace_edges_and_ignores_external_dependencies() {
        let metadata = json!({
            "workspace_members": ["app-id", "dsl-id"],
            "packages": [
                {"id": "app-id", "name": "caret-app", "dependencies": [
                    {"name": "caret-dsl-core", "kind": null},
                    {"name": "serde", "kind": null}
                ]},
                {"id": "dsl-id", "name": "caret-dsl-core", "dependencies": []}
            ]
        });
        let result = validate_cargo_metadata(&metadata);
        assert!(
            result.is_ok(),
            "expected registered graph to pass: {result:?}"
        );
    }

    #[test]
    fn rejects_forbidden_workspace_edge_with_source_and_kind() {
        let metadata = json!({
            "workspace_members": ["dsl-id", "app-id"],
            "packages": [
                {"id": "dsl-id", "name": "caret-dsl-core", "dependencies": [
                    {"name": "caret-app", "kind": "build"}
                ]},
                {"id": "app-id", "name": "caret-app", "dependencies": []}
            ]
        });
        let error = validate_cargo_metadata(&metadata);
        assert!(
            error.is_err(),
            "forbidden architecture edge unexpectedly passed"
        );
        let message = error.err().unwrap_or_default();
        assert!(message.contains("caret-dsl-core -> caret-app"));
        assert!(message.contains("build"));
    }

    #[test]
    fn rejects_workspace_crate_missing_from_allowed_layer_table() {
        let metadata = json!({
            "workspace_members": ["unknown-id"],
            "packages": [{"id": "unknown-id", "name": "caret-new-crate", "dependencies": []}]
        });
        let error = validate_cargo_metadata(&metadata);
        assert!(
            error.is_err(),
            "unregistered workspace crate unexpectedly passed"
        );
        assert!(error.err().unwrap_or_default().contains("caret-new-crate"));
    }
}

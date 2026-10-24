//! 合并 CMake 与 CXX 的真实编译命令；静态检查只选择自有翻译单元。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompileCommand {
    pub directory: PathBuf,
    pub file: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
}

impl CompileCommand {
    pub fn source(&self) -> PathBuf {
        self.directory.join(&self.file)
    }

    /// 用库模型代替指定依赖的头文件，同时保留自有头文件和真实编译选项。
    pub fn exclude_include_root(&mut self, root: &Path) -> Result<(), String> {
        let arguments = if let Some(arguments) = &self.arguments {
            arguments.clone()
        } else {
            shlex::split(
                self.command
                    .as_deref()
                    .ok_or("编译命令缺少 command/arguments")?,
            )
            .ok_or("编译命令包含无效引号")?
        };
        let prefixes = [
            "-isystem",
            "-imsvc",
            "-iframework",
            "/external:I",
            "-I",
            "-F",
            "/I",
        ];
        let mut filtered = Vec::new();
        let mut args = arguments.into_iter();
        while let Some(arg) = args.next() {
            if prefixes.contains(&arg.as_str()) {
                let path = args.next().ok_or_else(|| format!("{arg} 缺少路径"))?;
                if !self.directory.join(&path).starts_with(root) {
                    filtered.extend([arg, path]);
                }
            } else if !prefixes.iter().any(|prefix| {
                arg.strip_prefix(prefix)
                    .is_some_and(|path| self.directory.join(path).starts_with(root))
            }) {
                filtered.push(arg);
            }
        }
        self.arguments = Some(filtered);
        self.command = None;
        Ok(())
    }

    /// CMake 的 command 使用引号保护含空格的编译器路径；CXX 直接提供 arguments。
    pub fn compiler(&self) -> Result<PathBuf, String> {
        let value = if let Some(args) = &self.arguments {
            args.first().ok_or("编译命令 arguments 为空")?.as_str()
        } else {
            let command = self
                .command
                .as_deref()
                .ok_or("编译命令缺少 command/arguments")?
                .trim();
            if let Some(quoted) = command.strip_prefix('"') {
                quoted.split_once('"').ok_or("编译器路径引号未闭合")?.0
            } else {
                command.split_whitespace().next().ok_or("编译命令为空")?
            }
        };
        Ok(self.directory.join(value))
    }
}

pub fn read(path: &Path) -> Result<Vec<CompileCommand>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取 {}：{e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("解析 {}：{e}", path.display()))
}

pub fn write(path: &Path, commands: &[CompileCommand]) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(commands).map_err(|e| e.to_string())?;
    std::fs::write(path, bytes).map_err(|e| format!("写入 {}：{e}", path.display()))
}

/// 自有 C++ 与 Objective-C++ 翻译单元；头文件随实际消费者检查。
pub fn is_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("cpp" | "cc" | "cxx" | "mm")
    )
}

/// 同时供格式检查与增量静态分析识别公共头输入，包括已删除的路径。
pub fn is_header(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("h" | "hh" | "hpp" | "hxx")
    )
}

/// 排除构建目录、第三方与头文件；头文件随包含它的真实翻译单元检查。
pub fn owned(commands: Vec<CompileCommand>, root: &Path) -> Vec<CompileCommand> {
    let directories = [
        "native",
        "tests/cpp",
        "tests/qml",
        "tests/performance",
        "crates/caret-ffi",
    ];
    let mut commands: Vec<_> = commands
        .into_iter()
        .filter(|entry| {
            let source = entry.source();
            is_source(&source)
                && directories
                    .iter()
                    .any(|dir| source.starts_with(root.join(dir)))
        })
        .collect();
    commands.sort_by_key(CompileCommand::source);
    commands.dedup_by_key(|entry| entry.source());
    commands
}

/// 比较实际编译命令中的可执行文件，阻止只安装托管资产却继续使用系统编译器。
pub fn verify(commands: &[CompileCommand], compiler: &Path) -> Result<(), String> {
    if commands.is_empty() {
        return Err("自有 C++ 编译命令为空".into());
    }
    let expected = compiler.canonicalize().map_err(|e| e.to_string())?;
    for entry in commands {
        let actual = entry.compiler()?;
        if actual
            .canonicalize()
            .map_err(|e| format!("{}：{e}", actual.display()))?
            != expected
        {
            return Err(format!(
                "{} 使用 {}，期望 {}",
                entry.source().display(),
                actual.display(),
                compiler.display()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(file: &str) -> CompileCommand {
        CompileCommand {
            directory: PathBuf::from("/repo"),
            file: file.into(),
            arguments: None,
            command: Some("\"/tools/LLVM 22/clang++\" -c source.cc".into()),
        }
    }

    #[test]
    fn keeps_handwritten_cxx_and_tests_without_generated_code_or_headers() {
        let entries = [
            "native/app/main.cpp",
            "native/app/window_chrome_metrics_mac.mm",
            "tests/cpp/ffi/test.cc",
            "tests/qml/main.cpp",
            "tests/performance/gpu/viewport_benchmark.cpp",
            "crates/caret-ffi/src/ffi_support.cc",
            "target/cxxbridge/lib.rs.cc",
            "native/app/main.h",
            "native-external/file.cc",
        ]
        .map(entry)
        .to_vec();
        let selected = owned(entries, Path::new("/repo"));
        assert_eq!(selected.len(), 6);
        assert!(
            selected
                .iter()
                .any(|entry| entry.file == Path::new("crates/caret-ffi/src/ffi_support.cc"))
        );
    }

    #[test]
    fn handles_compiler_paths_with_spaces_and_rejects_missing_commands() {
        let mut entry = entry("source.cc");
        assert_eq!(
            entry.compiler().unwrap_or_else(|e| panic!("{e}")),
            Path::new("/tools/LLVM 22/clang++")
        );
        entry.arguments = Some(vec!["/tools/compiler with spaces".into(), "-c".into()]);
        assert_eq!(
            entry.compiler().unwrap_or_else(|e| panic!("{e}")),
            Path::new("/tools/compiler with spaces")
        );
        entry.arguments = Some(vec![]);
        assert!(entry.compiler().is_err());
    }

    #[test]
    fn library_model_removes_only_the_selected_dependency_includes() -> Result<(), String> {
        let mut entry = entry("source.cc");
        entry.command = Some(
            r#""/tools/LLVM 22/clang++" -I"/repo/target/qt/include" -isystem "/repo/target/qt/lib/Qt Core" -I /repo/native -I/repo/target/qt-extra -DQT_CORE_LIB=1 -c source.cc"#.into(),
        );
        entry.exclude_include_root(Path::new("/repo/target/qt"))?;
        assert_eq!(
            entry.arguments,
            Some(vec![
                "/tools/LLVM 22/clang++".into(),
                "-I".into(),
                "/repo/native".into(),
                "-I/repo/target/qt-extra".into(),
                "-DQT_CORE_LIB=1".into(),
                "-c".into(),
                "source.cc".into(),
            ])
        );
        assert!(entry.command.is_none());
        entry.arguments = Some(vec![
            "clang-cl.exe".into(),
            "/I/repo/target/qt/include".into(),
            "-imsvc".into(),
            "/sdk/include".into(),
        ]);
        entry.exclude_include_root(Path::new("/repo/target/qt"))?;
        assert_eq!(entry.arguments.as_ref().map(Vec::len), Some(3));
        Ok(())
    }

    #[test]
    fn rejects_system_compiler_even_when_managed_binary_exists() -> Result<(), String> {
        let root = std::env::temp_dir().join(format!("caret-db-test-{}", std::process::id()));
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let managed = root.join("managed compiler");
        let system = root.join("system compiler");
        for file in [&managed, &system] {
            std::fs::write(file, "binary").map_err(|e| e.to_string())?;
        }
        let mut command = entry("native/source.cc");
        command.arguments = Some(vec![managed.to_string_lossy().into_owned()]);
        verify(&[command.clone()], &managed)?;
        command.arguments = Some(vec![system.to_string_lossy().into_owned()]);
        assert!(verify(&[command], &managed).is_err());
        assert!(verify(&[], &managed).is_err());
        std::fs::remove_dir_all(root).map_err(|e| e.to_string())
    }
}

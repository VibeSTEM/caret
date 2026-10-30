//! 合并 CMake 与 CXX 的真实编译命令；静态检查只选择自有翻译单元。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

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
mod tests;

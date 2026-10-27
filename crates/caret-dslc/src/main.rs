use std::env;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

use caret_dsl_core::{
    Kind, MAX_SOURCE_BYTES, SourceKind, emit_ts, format_source, fsm, parse, source_kind,
};

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("caret-dslc: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage().into());
    };
    match command {
        "check" | "validate" => {
            let input = required_path(&arguments, 1)?;
            let source = read_source(input)?;
            validate_source(&source)?;
            Ok(())
        }
        "emit-ts" => {
            let input = required_path(&arguments, 1)?;
            let output = required_path(&arguments, 2)?;
            let source = read_source(input)?;
            if source_kind(&source) == Some(SourceKind::Fsm) {
                return Err("emit-ts requires kind: language".into());
            }
            let document = parse(&source)?;
            if document.kind != Kind::Language {
                return Err("emit-ts requires kind: language".into());
            }
            let locale = arguments
                .get(3)
                .map(String::as_str)
                .or(document.language.as_deref())
                .unwrap_or("en");
            let ts = emit_ts(&document, locale)?;
            write_atomically(output, ts.as_bytes())
        }
        "format" => {
            let check = arguments.get(1).is_some_and(|value| value == "--check");
            let input_index = usize::from(check) + 1;
            let input = required_path(&arguments, input_index)?;
            let source = read_source(input)?;
            let formatted = match source_kind(&source) {
                Some(SourceKind::Fsm) => fsm::format_source(&source)?,
                _ => format_source(&source)?,
            };
            if check {
                if formatted == source {
                    Ok(())
                } else {
                    Err(format!("{} requires formatting", input.display()).into())
                }
            } else if formatted == source {
                Ok(())
            } else {
                write_atomically(input, formatted.as_bytes())
            }
        }
        _ => Err(usage().into()),
    }
}

/// `check` 按头部 kind 行分流：fsm 走独立 schema，其余共用主 parser。
fn validate_source(source: &str) -> Result<(), Box<dyn std::error::Error>> {
    match source_kind(source) {
        Some(SourceKind::Fsm) => fsm::parse(source).map(|_| ())?,
        _ => parse(source).map(|_| ())?,
    }
    Ok(())
}

fn required_path(arguments: &[String], index: usize) -> Result<&Path, String> {
    arguments.get(index).map(Path::new).ok_or_else(usage)
}

fn read_source(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?
        .take(MAX_SOURCE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return Err(format!(
            "pa.source_too_large: source exceeds {MAX_SOURCE_BYTES} bytes in {}",
            path.display()
        ));
    }
    String::from_utf8(bytes).map_err(|error| {
        format!(
            "pa.invalid_utf8 at byte {} in {}",
            error.utf8_error().valid_up_to(),
            path.display()
        )
    })
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let temporary = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("out")
    ));
    // 创建/写入/落盘三步共用同一错误面（同目标同操作语义），任一失败都
    // 以临时文件路径报告；临时文件残留由调用方重试覆盖。
    let write = fs::File::create(&temporary)
        .and_then(|mut file| file.write_all(contents).and_then(|_| file.sync_all()));
    write.map_err(|error| format!("write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("replace {}: {error}", path.display()))?;
    Ok(())
}

fn usage() -> String {
    "usage: caret-dslc check <input.pa> | emit-ts <input.pa> <output.ts> [locale] | format [--check] <input.pa>; kind: fsm is accepted by check/format".to_owned()
}

#[cfg(test)]
mod tests;

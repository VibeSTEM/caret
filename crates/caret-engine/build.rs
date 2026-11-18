// 从共享命令行协议描述生成 Rust 快照映射。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("missing manifest")?);
    let protocol_path = manifest_dir.join("../../shared/command_line_protocol.json");
    println!("cargo:rerun-if-changed={}", protocol_path.display());
    let protocol: serde_json::Value = serde_json::from_slice(&fs::read(protocol_path)?)?;
    let modes = protocol["modes"].as_array().ok_or("missing modes")?;
    let mut generated = String::new();

    let fields = protocol["fields"].as_object().ok_or("missing fields")?;
    generated.push_str(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq)]\npub enum CommandLineWireField {\n",
    );
    for symbol in fields.keys() {
        generated.push_str(&format!("    /// 共享协议字段。\n    {symbol},\n"));
    }
    generated.push_str("}\nimpl CommandLineWireField {\n    /// 返回共享协议字段名。\n    pub fn wire_name(self) -> &'static str {\n        match self {\n");
    for (symbol, wire_name) in fields {
        let wire_name = rust_string(wire_name.as_str().ok_or("invalid field name")?)?;
        generated.push_str(&format!("            Self::{symbol} => {wire_name},\n"));
    }
    generated.push_str("        }\n    }\n}\n");
    generated.push_str("impl CommandLineMode {\n");

    generated.push_str("    /// 返回共享协议中的稳定模式名。\n    pub fn wire_name(self) -> &'static str {\n        match self {\n");
    for mode in modes {
        let variant = required_string(mode, "variant")?;
        let wire_name = rust_string(required_string(mode, "wireName")?)?;
        generated.push_str(&format!("            Self::{variant} => {wire_name},\n"));
    }
    generated.push_str("        }\n    }\n");

    generated.push_str("    /// 按共享协议名称解析模式。\n    pub fn from_wire_name(name: &str) -> Option<Self> {\n        match name {\n");
    for mode in modes {
        let variant = required_string(mode, "variant")?;
        let wire_name = rust_string(required_string(mode, "wireName")?)?;
        generated.push_str(&format!(
            "            {wire_name} => Some(Self::{variant}),\n"
        ));
    }
    generated.push_str("            _ => None,\n        }\n    }\n");

    generated.push_str("    /// 按共享协议规则校验快照前缀。\n    pub fn accepts_wire_prefix(self, prefix: &str) -> bool {\n        match self {\n");
    for mode in modes {
        let variant = required_string(mode, "variant")?;
        let default_prefix = rust_string(required_string(mode, "defaultPrefix")?)?;
        let expression = match &mode["requiredSuffix"] {
            serde_json::Value::Null => format!("prefix == {default_prefix}"),
            serde_json::Value::String(suffix) => format!(
                "prefix.starts_with({default_prefix}) && prefix.ends_with({})",
                rust_string(suffix)?
            ),
            _ => return Err("requiredSuffix must be a string or null".into()),
        };
        generated.push_str(&format!("            Self::{variant} => {expression},\n"));
    }
    generated.push_str("        }\n    }\n");

    generated.push_str("    /// 返回旧快照省略 prefix 字段时使用的兼容缺省值。\n    pub fn wire_default_prefix(self) -> &'static str {\n        match self {\n");
    for mode in modes {
        let variant = required_string(mode, "variant")?;
        let default_prefix = rust_string(required_string(mode, "defaultPrefix")?)?;
        generated.push_str(&format!(
            "            Self::{variant} => {default_prefix},\n"
        ));
    }
    generated.push_str("        }\n    }\n");
    let default_mode = required_string(&protocol, "defaultMode")?;
    let default_mode_prefix = modes
        .iter()
        .find(|mode| mode["wireName"].as_str() == Some(default_mode))
        .ok_or("default mode is missing from modes")?;
    generated.push_str(&format!(
        "    /// 返回无活动命令行时的共享协议默认模式和前缀。\n    pub fn wire_default() -> (&'static str, &'static str) {{ ({}, {}) }}\n",
        rust_string(default_mode)?,
        rust_string(required_string(default_mode_prefix, "defaultPrefix")?)?,
    ));
    generated.push_str("}\n");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("missing OUT_DIR")?);
    fs::write(out_dir.join("command_line_protocol.rs"), generated)?;
    Ok(())
}

fn required_string<'a>(value: &'a serde_json::Value, key: &str) -> Result<&'a str, Box<dyn Error>> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("missing string field: {key}").into())
}

fn rust_string(value: &str) -> Result<String, Box<dyn Error>> {
    Ok(serde_json::to_string(value)?)
}

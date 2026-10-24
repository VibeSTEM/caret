//! language / theme / variables 的数据模型及只读查询。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 头部 `kind` 行判定的文档入口；`fsm` 由 [`crate::fsm`] 模块解析。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    Language,
    Theme,
    Variables,
    Fsm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Language,
    Theme,
    Variables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Finished,
    Unfinished,
    Vanished,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType {
    Bool,
    Int,
    Real,
    String,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Variable {
    pub name: String,
    pub value_type: ValueType,
    pub expression: String,
    /// 源码中的表达式字节范围；仅供诊断定位，不进入序列化 AST。
    #[serde(skip)]
    pub expression_start: usize,
    #[serde(skip)]
    pub expression_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Translation {
    pub forms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub context: String,
    pub id: String,
    pub source: String,
    pub old_source: Option<String>,
    pub translations: BTreeMap<String, Translation>,
    pub status: Option<Status>,
    pub comment: Option<String>,
    pub extra: Option<String>,
    pub numerus: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub kind: Kind,
    pub catalog: Option<String>,
    pub language: Option<String>,
    pub source_language: String,
    pub messages: BTreeMap<String, Message>,
    /// 源码声明顺序；messages 仍作为查找索引。
    #[serde(default)]
    pub message_order: Vec<String>,
    /// 格式化时保留的独立注释。
    #[serde(default)]
    pub comments: Vec<String>,
    pub values: Vec<Variable>,
}

/// 匹配 input 开头的 source 文本，优先返回最长匹配。
pub fn source_prefix<'a>(document: &'a Document, input: &str) -> Option<(&'a str, &'a Message)> {
    document
        .messages
        .values()
        .filter(|message| input.starts_with(&message.source))
        .max_by_key(|message| message.source.chars().count())
        .map(|message| (message.id.as_str(), message))
}

//! 文档会话的只读运行期上下文与不可变变量快照。
//!
//! 上下文由应用服务持有，不保存 UI 引擎对象。任务捕获共享快照后，后续
//! 变量提交不会改变其输入；会话替换或关闭会协作取消旧作用域。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use crate::path::{PathError, PathService, ResourceRef, RootCategory};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// 变量 DSL 的基础运行时值；表达式和语义校验由 DSL 层负责。
#[derive(Debug, Clone, PartialEq)]
pub enum RuntimeValue {
    Boolean(bool),
    Integer(i64),
    Real(f64),
    String(String),
    Resource(ResourceRef),
}

/// 不可变变量快照；克隆只增加共享引用，不复制变量表。
#[derive(Debug, Clone, PartialEq)]
pub struct VariableSnapshot {
    revision: u64,
    values: Arc<BTreeMap<String, RuntimeValue>>,
}

impl VariableSnapshot {
    fn empty() -> Self {
        Self {
            revision: 0,
            values: Arc::new(BTreeMap::new()),
        }
    }

    /// 返回该快照的独立修订号。
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// 按名称读取值；调用方不能通过该引用修改快照。
    pub fn get(&self, name: &str) -> Option<&RuntimeValue> {
        self.values.get(name)
    }

    /// 只读遍历已排序的变量项。
    pub fn iter(&self) -> impl Iterator<Item = (&str, &RuntimeValue)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value))
    }

    /// 变量项数量。
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// 快照是否为空。
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// 快照提交被拒绝的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeCommitError {
    NoContext,
    Busy,
    Closed,
    StaleContext,
    EmptyVariableName,
    NonFiniteNumber(String),
    StaleRevision { expected: u64, current: u64 },
    RevisionExhausted,
}

impl std::fmt::Display for RuntimeCommitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy => write!(formatter, "runtime variable write pending"),
            Self::NoContext => write!(formatter, "no active runtime context"),
            Self::Closed => write!(formatter, "runtime context is closed"),
            Self::StaleContext => write!(formatter, "runtime task scope is stale"),
            Self::EmptyVariableName => write!(formatter, "variable name is empty"),
            Self::NonFiniteNumber(name) => write!(formatter, "variable {name:?} is not finite"),
            Self::StaleRevision { expected, current } => {
                write!(
                    formatter,
                    "stale variable revision {expected}; current is {current}"
                )
            }
            Self::RevisionExhausted => write!(formatter, "variable revision exhausted"),
        }
    }
}

impl std::error::Error for RuntimeCommitError {}

/// 当前运行时会话；只提供变量读取与文档资源根内现存资源解析。
#[derive(Debug)]
pub struct RuntimeContext {
    identity: Arc<()>,
    generation: u64,
    paths: PathService,
    variables: VariableSnapshot,
    cancelled: Arc<AtomicBool>,
    closed: bool,
}

impl RuntimeContext {
    /// 为当前文档创建只读资源根上下文。
    pub fn for_project(generation: u64, project_root: &Path) -> Result<Self, PathError> {
        let mut paths = PathService::new();
        paths.set_root(RootCategory::Project, project_root)?;
        Ok(Self::new(generation, paths))
    }

    /// 使用宿主已注入的路径根创建运行期会话。
    pub fn new(generation: u64, paths: PathService) -> Self {
        Self {
            identity: Arc::new(()),
            generation,
            paths,
            variables: VariableSnapshot::empty(),
            cancelled: Arc::new(AtomicBool::new(false)),
            closed: false,
        }
    }

    /// 此上下文所属的文档会话代次，与变量快照的修订号分开。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 当前不可变变量快照。
    pub fn variables(&self) -> &VariableSnapshot {
        &self.variables
    }

    /// 创建任务作用域；作用域固定变量输入并共享会话取消信号。
    pub fn capture_task_scope(&self) -> RuntimeTaskScope {
        RuntimeTaskScope {
            identity: Arc::clone(&self.identity),
            generation: self.generation,
            variables: self.variables.clone(),
            paths: self.paths.clone(),
            cancelled: Arc::clone(&self.cancelled),
        }
    }

    /// 按预期修订原子发布完整变量表；失败时当前快照保持不变。
    pub fn commit_variables(
        &mut self,
        expected_revision: u64,
        values: BTreeMap<String, RuntimeValue>,
    ) -> Result<&VariableSnapshot, RuntimeCommitError> {
        if self.closed {
            return Err(RuntimeCommitError::Closed);
        }
        for (name, value) in &values {
            if name.trim().is_empty() {
                return Err(RuntimeCommitError::EmptyVariableName);
            }
            if matches!(value, RuntimeValue::Real(number) if !number.is_finite()) {
                return Err(RuntimeCommitError::NonFiniteNumber(name.clone()));
            }
        }
        if expected_revision != self.variables.revision {
            return Err(RuntimeCommitError::StaleRevision {
                expected: expected_revision,
                current: self.variables.revision,
            });
        }
        let revision = self
            .variables
            .revision
            .checked_add(1)
            .ok_or(RuntimeCommitError::RevisionExhausted)?;
        self.variables = VariableSnapshot {
            revision,
            values: Arc::new(values),
        };
        Ok(&self.variables)
    }

    /// 仅接受当前会话与当前变量修订捕获的任务结果。
    pub fn commit_task_variables(
        &mut self,
        scope: &RuntimeTaskScope,
        values: BTreeMap<String, RuntimeValue>,
    ) -> Result<&VariableSnapshot, RuntimeCommitError> {
        if !self.accepts(scope) {
            return Err(RuntimeCommitError::StaleContext);
        }
        self.commit_variables(scope.variables.revision, values)
    }

    /// 在文档资源根约束下解析现存资源；上下文不暴露任意写入路径能力。
    pub fn resolve_existing(
        &self,
        reference: &ResourceRef,
    ) -> Result<std::path::PathBuf, RuntimeResourceError> {
        if self.closed {
            return Err(RuntimeResourceError::ContextClosed);
        }
        self.paths
            .resolve_existing(reference)
            .map_err(RuntimeResourceError::Path)
    }

    /// 校验资源引用与根授权，但不要求资源文件已经存在。
    pub fn validate_reference(&self, reference: &ResourceRef) -> Result<(), RuntimeResourceError> {
        if self.closed {
            return Err(RuntimeResourceError::ContextClosed);
        }
        self.paths
            .validate_reference(reference)
            .map_err(RuntimeResourceError::Path)
    }

    /// 关闭会话并请求其所有任务作用域停止。
    pub fn close(&mut self) {
        self.closed = true;
        self.cancelled.store(true, Ordering::Release);
    }

    /// 仅当作用域属于当前会话、快照仍是最新且会话未取消时接受其结果。
    pub fn accepts(&self, scope: &RuntimeTaskScope) -> bool {
        !self.closed
            && !scope.is_cancelled()
            && Arc::ptr_eq(&self.identity, &scope.identity)
            && self.generation == scope.generation
            && self.variables.revision == scope.variables.revision
    }
}

impl Drop for RuntimeContext {
    fn drop(&mut self) {
        self.close();
    }
}

/// 运行时资源读取失败。
#[derive(Debug, PartialEq, Eq)]
pub enum RuntimeResourceError {
    ContextClosed,
    TaskCancelled,
    Path(PathError),
}

impl std::fmt::Display for RuntimeResourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextClosed => write!(formatter, "runtime context is closed"),
            Self::TaskCancelled => write!(formatter, "runtime task scope is cancelled"),
            Self::Path(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for RuntimeResourceError {}

/// 任务提交时捕获的会话身份、只读变量快照与资源解析能力。
#[derive(Debug, Clone)]
pub struct RuntimeTaskScope {
    identity: Arc<()>,
    generation: u64,
    variables: VariableSnapshot,
    paths: PathService,
    cancelled: Arc<AtomicBool>,
}

impl RuntimeTaskScope {
    /// 任务提交时的文档会话代次。
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 任务提交时固定的变量快照。
    pub fn variables(&self) -> &VariableSnapshot {
        &self.variables
    }

    /// 任务是否收到会话关闭或切换的协作取消请求。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    /// 在捕获时已授权的文档资源根中读取现存资源。
    pub fn resolve_existing(
        &self,
        reference: &ResourceRef,
    ) -> Result<std::path::PathBuf, RuntimeResourceError> {
        if self.is_cancelled() {
            return Err(RuntimeResourceError::TaskCancelled);
        }
        self.paths
            .resolve_existing(reference)
            .map_err(RuntimeResourceError::Path)
    }

    /// 在捕获时的根授权中校验逻辑引用；不检查文件是否存在。
    pub fn validate_reference(&self, reference: &ResourceRef) -> Result<(), RuntimeResourceError> {
        if self.is_cancelled() {
            return Err(RuntimeResourceError::TaskCancelled);
        }
        self.paths
            .validate_reference(reference)
            .map_err(RuntimeResourceError::Path)
    }
}

#[cfg(test)]
mod tests;

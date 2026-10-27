//! 验证应用运行时任务、快照和取消语义。

use super::{RuntimeCommitError, RuntimeContext, RuntimeValue};
use crate::path::{PathService, ResourceRef, RootCategory};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

struct TempRoot {
    root: PathBuf,
}

impl TempRoot {
    fn new(label: &str) -> std::io::Result<Self> {
        let base = fs::canonicalize(std::env::temp_dir()).unwrap_or_else(|_| std::env::temp_dir());
        let root = base.join(format!("caret-runtime-{}-{label}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn values(name: &str, value: i64) -> BTreeMap<String, RuntimeValue> {
    BTreeMap::from([(name.to_owned(), RuntimeValue::Integer(value))])
}

#[test]
fn task_scope_keeps_its_immutable_variable_snapshot() -> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(7, PathService::new());
    context.commit_variables(0, values("count", 10))?;
    let scope = context.capture_task_scope();

    context.commit_variables(1, values("count", 20))?;

    assert_eq!(scope.variables().revision(), 1);
    assert_eq!(
        scope.variables().get("count"),
        Some(&RuntimeValue::Integer(10))
    );
    assert_eq!(
        context.variables().get("count"),
        Some(&RuntimeValue::Integer(20))
    );
    assert!(!context.accepts(&scope));
    Ok(())
}

#[test]
fn stale_variable_commit_keeps_the_current_snapshot() -> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(1, PathService::new());
    context.commit_variables(0, values("width", 200))?;
    let before = context.variables().clone();

    let result = context.commit_variables(0, values("width", 220));

    assert_eq!(
        result,
        Err(RuntimeCommitError::StaleRevision {
            expected: 0,
            current: 1
        })
    );
    assert_eq!(&before, context.variables());
    Ok(())
}

#[test]
fn task_result_commit_rejects_a_replaced_snapshot() -> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(1, PathService::new());
    context.commit_variables(0, values("count", 10))?;
    let scope = context.capture_task_scope();
    context.commit_variables(1, values("count", 20))?;

    let result = context.commit_task_variables(&scope, values("count", 30));

    assert_eq!(result, Err(RuntimeCommitError::StaleContext));
    assert_eq!(
        context.variables().get("count"),
        Some(&RuntimeValue::Integer(20))
    );
    Ok(())
}

#[test]
fn invalid_values_do_not_replace_the_current_snapshot() -> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(1, PathService::new());
    context.commit_variables(0, values("count", 10))?;
    let before = context.variables().clone();
    let non_finite = BTreeMap::from([("count".to_owned(), RuntimeValue::Real(f64::NAN))]);

    assert_eq!(
        context.commit_variables(1, non_finite),
        Err(RuntimeCommitError::NonFiniteNumber("count".to_owned()))
    );
    assert_eq!(&before, context.variables());
    Ok(())
}

#[test]
fn empty_variable_names_do_not_replace_the_current_snapshot() -> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(1, PathService::new());
    context.commit_variables(0, values("count", 10))?;
    let before = context.variables().clone();

    assert_eq!(
        context.commit_variables(1, values(" ", 20)),
        Err(RuntimeCommitError::EmptyVariableName)
    );
    assert_eq!(&before, context.variables());
    Ok(())
}

#[test]
fn closing_context_cancels_scopes_and_rejects_results() {
    let mut context = RuntimeContext::new(3, PathService::new());
    let scope = context.capture_task_scope();

    context.close();

    assert!(scope.is_cancelled());
    assert!(!context.accepts(&scope));
    assert_eq!(
        context.commit_variables(0, BTreeMap::new()),
        Err(RuntimeCommitError::Closed)
    );
}

#[test]
fn dropping_context_cancels_captured_scopes() {
    let scope = {
        let context = RuntimeContext::new(3, PathService::new());
        context.capture_task_scope()
    };

    assert!(scope.is_cancelled());
}

#[test]
fn runtime_context_resolves_only_existing_resources_under_project_root()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempRoot::new("existing")?;
    std::fs::write(fixture.root.join("note.txt"), b"note")?;
    let context = RuntimeContext::for_project(4, &fixture.root)?;
    let reference = ResourceRef {
        category: RootCategory::Project,
        relative: "note.txt".to_owned(),
    };

    assert_eq!(
        context.resolve_existing(&reference)?,
        fs::canonicalize(fixture.root.join("note.txt"))?
    );
    assert!(
        context
            .resolve_existing(&ResourceRef {
                category: RootCategory::Project,
                relative: "missing.bin".to_owned(),
            })
            .is_err()
    );
    Ok(())
}

#[test]
fn closed_context_and_cancelled_scope_cannot_resolve_resources()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempRoot::new("closed")?;
    let resource_path = fixture.root.join("note.txt");
    std::fs::write(&resource_path, b"note")?;
    let reference = ResourceRef {
        category: RootCategory::Project,
        relative: "note.txt".to_owned(),
    };
    let mut context = RuntimeContext::for_project(4, &fixture.root)?;
    let scope = context.capture_task_scope();

    context.close();

    assert_eq!(
        context.resolve_existing(&reference),
        Err(super::RuntimeResourceError::ContextClosed)
    );
    assert_eq!(
        scope.resolve_existing(&reference),
        Err(super::RuntimeResourceError::TaskCancelled)
    );
    Ok(())
}

#[test]
fn task_scopes_from_another_context_are_not_accepted() {
    let first = RuntimeContext::new(1, PathService::new());
    let second = RuntimeContext::new(1, PathService::new());

    assert!(!second.accepts(&first.capture_task_scope()));
}

#[test]
fn snapshot_iteration_is_sorted_and_remains_fixed_after_a_later_commit()
-> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(17, PathService::new());
    assert!(context.variables().is_empty());
    assert_eq!(context.variables().len(), 0);
    context.commit_variables(
        0,
        BTreeMap::from([
            ("z".to_owned(), RuntimeValue::String("last".to_owned())),
            ("a".to_owned(), RuntimeValue::Boolean(true)),
        ]),
    )?;
    let scope = context.capture_task_scope();
    context.commit_variables(1, BTreeMap::new())?;

    assert_eq!(context.generation(), 17);
    assert_eq!(scope.generation(), 17);
    assert!(!scope.variables().is_empty());
    assert_eq!(scope.variables().len(), 2);
    assert_eq!(
        scope
            .variables()
            .iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>(),
        ["a", "z"]
    );
    assert!(context.variables().is_empty());
    Ok(())
}

#[test]
fn task_scope_validates_uncreated_resources_until_its_context_closes()
-> Result<(), Box<dyn std::error::Error>> {
    let fixture = TempRoot::new("scope-reference")?;
    fs::write(fixture.root.join("note.txt"), b"note")?;
    let mut context = RuntimeContext::for_project(9, &fixture.root)?;
    let scope = context.capture_task_scope();
    let reference = ResourceRef {
        category: RootCategory::Project,
        relative: "future/asset.png".to_owned(),
    };
    context.validate_reference(&reference)?;
    scope.validate_reference(&reference)?;
    assert_eq!(
        scope.resolve_existing(&ResourceRef {
            category: RootCategory::Project,
            relative: "note.txt".to_owned(),
        })?,
        fs::canonicalize(fixture.root.join("note.txt"))?
    );
    assert!(
        scope
            .validate_reference(&ResourceRef {
                category: RootCategory::Project,
                relative: "../outside.txt".to_owned(),
            })
            .is_err()
    );

    context.close();
    assert_eq!(
        context.validate_reference(&reference),
        Err(super::RuntimeResourceError::ContextClosed)
    );
    assert_eq!(
        scope.validate_reference(&reference),
        Err(super::RuntimeResourceError::TaskCancelled)
    );
    Ok(())
}

#[test]
fn current_task_result_commits_once_and_invalidates_its_original_scope()
-> Result<(), RuntimeCommitError> {
    let mut context = RuntimeContext::new(12, PathService::new());
    let scope = context.capture_task_scope();
    let committed = context.commit_task_variables(&scope, values("result", 42))?;
    assert_eq!(committed.revision(), 1);
    assert_eq!(committed.get("result"), Some(&RuntimeValue::Integer(42)));
    assert!(!context.accepts(&scope));
    assert_eq!(
        context.commit_task_variables(&scope, values("result", 43)),
        Err(RuntimeCommitError::StaleContext)
    );
    assert_eq!(
        context.variables().get("result"),
        Some(&RuntimeValue::Integer(42))
    );
    Ok(())
}

#[test]
fn exhausted_variable_revision_rejects_a_commit_without_replacing_the_snapshot() {
    let mut context = RuntimeContext::new(1, PathService::new());
    context.variables.revision = u64::MAX;
    let original = context.variables().clone();

    assert_eq!(
        context.commit_variables(u64::MAX, values("result", 1)),
        Err(RuntimeCommitError::RevisionExhausted)
    );
    assert_eq!(context.variables(), &original);
}

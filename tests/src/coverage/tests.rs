//! 验证覆盖率报告解析和门槛计算。

use super::*;

fn fixture(functions: u64, lines: u64) -> Value {
    let totals = json!({
        "functions": {"count": 100, "covered": functions},
        "lines": {"count": 100, "covered": lines}
    });
    json!({
        "type": "llvm.coverage.json.export",
        "version": "3.1.0",
        "data": [{
            "totals": totals,
            "files": [{
                "filename": "/repo/crates/caret-app/src/lib.rs",
                "summary": totals
            }]
        }]
    })
}

#[test]
fn coverage_scope_tracks_workspace_members_and_excludes_only_infrastructure()
-> Result<(), Box<dyn Error>> {
    let metadata = json!({
        "workspace_members": ["app-id", "future-domain-id", "tests-id"],
        "packages": [
            {"id": "app-id", "name": "caret-app"},
            {"id": "future-domain-id", "name": "caret-future-domain"},
            {"id": "tests-id", "name": "caret-tests"},
            {"id": "external-id", "name": "serde"}
        ]
    });
    assert_eq!(
        coverage_packages_from_metadata(&metadata)?,
        ["caret-app", "caret-future-domain"]
    );
    Ok(())
}

fn parse(value: &Value) -> Result<Report, Box<dyn Error>> {
    Report::parse(&serde_json::to_vec(value)?, Path::new("/repo"))
}
#[test]
fn gate_accepts_boundary_and_rejects_either_deficit() -> Result<(), Box<dyn Error>> {
    parse(&fixture(85, 85))?.check_gate()?;
    assert!(parse(&fixture(84, 100))?.check_gate().is_err());
    assert!(parse(&fixture(100, 84))?.check_gate().is_err());
    let rounded = Metric {
        covered: 84_999,
        count: 100_000,
    };
    assert_eq!(rounded.percent(), "85.00%");
    assert!(!rounded.reaches(FUNCTIONS_MINIMUM));
    Ok(())
}
#[test]
fn rejects_missing_empty_and_invalid_statistics() -> Result<(), Box<dyn Error>> {
    assert!(Report::parse(b"not json", Path::new("/repo")).is_err());
    for version in [json!(null), json!("2.0.1"), json!("4.0.0")] {
        let mut value = fixture(100, 100);
        value["version"] = version;
        assert!(parse(&value).is_err());
    }
    let mut value = fixture(100, 100);
    value["data"][0]["totals"]["lines"] = json!(null);
    assert!(parse(&value).is_err());
    for replacement in [json!([]), json!([{}]), json!([{}, {}])] {
        let mut value = fixture(100, 100);
        value["data"] = replacement;
        assert!(parse(&value).is_err());
    }
    let mut value = fixture(100, 100);
    value["data"][0]["files"] = json!([]);
    assert!(parse(&value).is_err());
    assert!(parse(&fixture(101, 100)).is_err());
    let mut value = fixture(0, 0);
    value["data"][0]["totals"]["functions"]["count"] = json!(0);
    value["data"][0]["totals"]["lines"]["count"] = json!(0);
    assert!(parse(&value)?.check_gate().is_err());
    Ok(())
}
#[test]
fn crate_summary_combines_files_and_preserves_zero_denominator() -> Result<(), Box<dyn Error>> {
    let mut value = fixture(100, 100);
    let file = value["data"][0]["files"][0].clone();
    value["data"][0]["files"]
        .as_array_mut()
        .ok_or("files")?
        .push(file);
    let report = parse(&value)?;
    assert!(
        report
            .summary()
            .contains("caret-app: functions 200/200 (100.00%)")
    );
    assert_eq!(Metric::default().percent(), "n/a");
    let mut metric = Metric {
        covered: u64::MAX,
        count: u64::MAX,
    };
    assert!(
        metric
            .add(Metric {
                covered: 1,
                count: 1
            })
            .is_err()
    );
    Ok(())
}

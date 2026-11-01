//! 按交互职责组织文档视图回归测试。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{Block, Document, InlineNode, Run};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

mod ime;
mod keyboard;
mod layout;
mod math;
mod navigation;

/// 快照 JSON 经 serde_json 默认（非逐位精确）的浮点解析回读，末位可能相差 1 ulp，
/// 几何比较一律带容差。
const GEOMETRY_EPSILON_PT: f64 = 1e-9;

fn assert_pt_close(actual: Option<f64>, expected: f64) {
    let Some(actual) = actual else {
        panic!("几何字段缺失，期望 {expected}");
    };
    assert!(
        (actual - expected).abs() <= GEOMETRY_EPSILON_PT,
        "几何坐标 {actual} 与期望 {expected} 相差超过容差"
    );
}

/// 取出 `environmentHighlights` 中某一层级的全部矩形；该层级没有高亮时返回空数组。
fn environment_highlight_rects(snapshot: &Value, level: &str) -> Value {
    Value::Array(
        snapshot["environmentHighlights"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|highlight| highlight["level"] == level)
            .flat_map(|highlight| highlight["rects"].as_array().cloned().unwrap_or_default())
            .collect(),
    )
}

/// 按字段带容差比较两组 `{pageIndex,x0Pt,y0Pt,x1Pt,y1Pt}` 矩形。
fn assert_rects_close(actual: &Value, expected: &Value) {
    let (Some(actual), Some(expected)) = (actual.as_array(), expected.as_array()) else {
        panic!("矩形列表应为数组：{actual} / {expected}");
    };
    assert_eq!(actual.len(), expected.len(), "矩形数量不一致");
    for (left, right) in actual.iter().zip(expected) {
        assert_eq!(left["pageIndex"], right["pageIndex"]);
        for key in ["x0Pt", "y0Pt", "x1Pt", "y1Pt"] {
            let Some(right) = right[key].as_f64() else {
                panic!("期望矩形缺少 {key}");
            };
            assert_pt_close(left[key].as_f64(), right);
        }
    }
}

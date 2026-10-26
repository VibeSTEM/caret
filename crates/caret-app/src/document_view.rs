//! 为界面提供由 Rust 文档会话生成的只读布局快照。

use caret_doc::Document;
use caret_engine::{DocumentLayoutRuntime, Session};

/// 当前文档会话与可复用的字体/布局运行时。
pub struct DocumentViewRuntime {
    session: Session,
    layout: DocumentLayoutRuntime,
}

impl DocumentViewRuntime {
    /// 创建空白文档会话并初始化一次系统字体目录。
    pub fn new_blank() -> Result<Self, String> {
        let session = Session::new(Document::blank()).map_err(|error| error.to_string())?;
        let layout = DocumentLayoutRuntime::new_system().map_err(|error| error.to_string())?;
        Ok(Self { session, layout })
    }

    /// 按当前会话和字体代际生成界面快照 JSON。
    pub fn layout_snapshot_json(&mut self) -> Result<String, String> {
        let snapshot = self
            .layout
            .layout_session(&self.session)
            .map_err(|error| error.to_string())?;
        serde_json::to_string(&caret_engine::DocumentLayoutView::from(&snapshot))
            .map_err(|error| format!("layout.serialize: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentViewRuntime;
    use serde_json::Value;

    #[test]
    fn blank_document_snapshot_contains_page_and_paragraph_layout() {
        let mut runtime = DocumentViewRuntime::new_blank()
            .unwrap_or_else(|error| panic!("document view runtime failed: {error}"));
        let json = runtime
            .layout_snapshot_json()
            .unwrap_or_else(|error| panic!("document layout snapshot failed: {error}"));
        let snapshot: Value = serde_json::from_str(&json)
            .unwrap_or_else(|error| panic!("layout snapshot JSON failed: {error}"));

        assert_eq!(snapshot["documentGeneration"], "0");
        assert_eq!(snapshot["fontGeneration"], "1");
        assert_eq!(snapshot["pages"].as_array().map(Vec::len), Some(1));
        assert_eq!(snapshot["pages"][0]["widthPt"], 595.2755905511812);
        assert_eq!(snapshot["paragraphs"].as_array().map(Vec::len), Some(1));
        assert_eq!(snapshot["paragraphs"][0]["text"], "");
        assert_eq!(snapshot["paragraphs"][0]["lines"][0]["sourceStart"], 0);
        assert!(
            snapshot["paragraphs"][0]["lines"][0]["heightPt"]
                .as_f64()
                .is_some_and(|height| height > 0.0)
        );
    }
}

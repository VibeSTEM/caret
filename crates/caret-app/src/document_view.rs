//! 为界面提供当前文档的只读页面几何快照。

use caret_doc::Document;

/// 为无活动文档的窗口提供新空白文档页面几何快照；打开真实文档后由会话布局替换。
pub fn blank_document_page_geometry_snapshot() -> Result<String, String> {
    let document = Document::blank();
    let section = document
        .page_sections
        .first()
        .ok_or_else(|| "document.missing_page_section".to_owned())?;
    let geometry = section.geometry().map_err(|error| error.to_string())?;
    let (width_pt, height_pt) = geometry.page_size_pt();
    Ok(serde_json::json!({
        "pages": [{ "widthPt": width_pt, "heightPt": height_pt }]
    })
    .to_string())
}

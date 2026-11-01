//! 按交互职责组织文档视图回归测试。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{Block, Document, ParagraphContent, Run, TextFlow};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

mod ime;
mod keyboard;
mod layout;
mod navigation;

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        source_range: None,
        extra: Default::default(),
    })
}

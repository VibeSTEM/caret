//! 按交互职责组织文档视图回归测试。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{Block, Document, NodeId, Paragraph, ParagraphContent, Run, TextFlow};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

mod ime;
mod keyboard;
mod layout;
mod navigation;

fn text_flow(run: Run) -> ParagraphContent {
    ParagraphContent::Text(TextFlow {
        runs: vec![run],
        extra: Default::default(),
    })
}

/// 构造只含一个纯文本 run 的段落，供各交互测试模块共用。
fn paragraph_with_text(text: &str) -> Paragraph {
    let mut paragraph = Paragraph::empty(NodeId::generate());
    paragraph.inlines.push(text_flow(Run {
        text: text.to_owned(),
        language: None,
        props: Default::default(),
        extra: Default::default(),
    }));
    paragraph
}

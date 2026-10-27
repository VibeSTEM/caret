//! 按交互职责组织文档视图回归测试。

use crate::document_view::DocumentViewRuntime;
use caret_doc::{Block, Document, InlineNode, Run};
use serde_json::Value;
use unicode_segmentation::UnicodeSegmentation;

mod ime;
mod keyboard;
mod layout;
mod navigation;

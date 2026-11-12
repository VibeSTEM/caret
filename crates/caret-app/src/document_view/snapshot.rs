//! 序列化完整布局与轻量交互快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentViewRuntime, InteractionContext, MathInteractionContext, MathTreeInteractionContext,
};
use caret_doc::NodeId;
use caret_engine::{
    Affinity, CompositionRange, DocumentLayoutDeltaView, MathEditTarget, MathTextInputContext,
    MathTreePosition, PageCaretGeometry, Pos, Selection, StructureClass, StructureKind,
    StructurePosition,
};
use serde_json::{Value, json};

fn command_line_state(session: &caret_engine::Session) -> Value {
    let code_font_family = &session.document().global_settings.default_code_font_family;
    let effective_code_font_family = if code_font_family == caret_doc::DEFAULT_FONT_FAMILY {
        caret_font_roles::DEFAULT_CODE_FONT_FAMILY
    } else {
        code_font_family
    };
    session
        .command_line()
        .map(|command_line| {
            let candidates = session
                .command_line_candidates()
                .into_iter()
                .map(|candidate| {
                    json!({
                        "alias": candidate.alias,
                        "commandId": candidate.command_id,
                        "available": candidate.available,
                        "value": candidate.value,
                        "hasChildren": candidate.has_children,
                    })
                })
                .collect::<Vec<_>>();
            let query_prefix = command_line
                .query
                .get(..command_line.cursor)
                .unwrap_or_default();
            let cursor_utf16 =
                command_line.prefix().encode_utf16().count() + query_prefix.encode_utf16().count();
            json!({
                "active": true,
                "mode": match command_line.mode {
                    caret_engine::CommandLineMode::Command => "command",
                    caret_engine::CommandLineMode::Math => "math",
                    caret_engine::CommandLineMode::FontFamily => "fontFamily",
                    caret_engine::CommandLineMode::DefaultFontTarget => "defaultFontTarget",
                    caret_engine::CommandLineMode::DefaultFontScope => "defaultFontScope",
                    caret_engine::CommandLineMode::DefaultFontLanguage => "defaultFontLanguage",
                    caret_engine::CommandLineMode::DefaultFontFamily => "defaultFontFamily",
                },
                "prefix": command_line.prefix(),
                "query": &command_line.query,
                "cursor": command_line.cursor,
                "cursorUtf16": cursor_utf16,
                "preedit": &command_line.preedit,
                "preeditCursorUtf16": command_line.preedit_cursor_utf16,
                "codeFontFamily": effective_code_font_family,
                "selected": command_line.selected,
                "candidates": candidates,
            })
        })
        .unwrap_or_else(|| {
            json!({
                "active": false,
                "mode": "command",
                "prefix": "\\",
                "query": "",
                "cursor": 0,
                "cursorUtf16": 1,
                "codeFontFamily": effective_code_font_family,
                "selected": 0,
                "candidates": [],
            })
        })
}

fn math_input_method_enabled(session: &caret_engine::Session) -> bool {
    let math_query_active = session
        .command_line()
        .is_some_and(|line| line.mode == caret_engine::CommandLineMode::Math);
    !math_query_active
        && (session.math_edit().is_none() || session.math_text_input_context().is_some())
}

fn math_input_method_prefers_latin(session: &caret_engine::Session) -> bool {
    !session
        .command_line()
        .is_some_and(|line| line.mode == caret_engine::CommandLineMode::Math)
        && session.math_text_input_context().is_some()
}

pub(super) fn current_structure_state(session: &caret_engine::Session) -> Value {
    let exited_environment_id = session
        .block_boundary_exit()
        .map(|(environment_id, _)| environment_id);
    let focus = &session.selection().focus;
    let active_block_id = session
        .current_math_edit_target()
        .and_then(|target| match target {
            caret_engine::MathEditTarget::Block { block_id, .. } => Some(block_id.clone()),
            caret_engine::MathEditTarget::Inline { .. } => None,
        })
        .or_else(|| session.block_boundary_exit().map(|(id, _)| id));
    if active_block_id
        .as_ref()
        .is_some_and(|block_id| session.structure_index().get(block_id).is_none())
    {
        return Value::Array(Vec::new());
    }
    let mut stack = match active_block_id.as_ref() {
        Some(block_id) => session
            .structure_index()
            .stack_for_id(block_id)
            .into_iter()
            .filter(|entry| &entry.id != block_id)
            .collect(),
        None => session.structure_index().at(&StructurePosition {
            paragraph_id: focus.paragraph_id.clone(),
            offset_utf8: focus.offset,
        }),
    };
    if let Some(exited_environment_id) = exited_environment_id {
        stack.retain(|entry| entry.id != exited_environment_id);
        if !stack.iter().any(|entry| entry.kind.is_block_environment()) {
            return Value::Array(Vec::new());
        }
    }
    let has_block_environment = stack.iter().any(|entry| entry.kind.is_block_environment());
    let mut entries = Vec::with_capacity(stack.len());
    for entry in stack {
        if has_block_environment && entry.kind.class() == StructureClass::Paragraph {
            continue;
        }
        let (kind, level) = match entry.kind {
            StructureKind::Paragraph => ("paragraph", None),
            StructureKind::Heading { level } => ("heading", Some(level)),
            StructureKind::Section => ("section", None),
            StructureKind::Custom => ("custom", None),
            StructureKind::Equation => ("equation", None),
            StructureKind::Align => ("align", None),
        };
        let mut value = json!({ "id": entry.id, "kind": kind });
        if let Some(level) = level {
            value["level"] = json!(level);
        }
        entries.push(value);
    }
    if let Some(caret_engine::MathEditTarget::Block { block_id, align }) =
        session.current_math_edit_target()
    {
        let kind = if *align { "align" } else { "equation" };
        if !entries
            .iter()
            .any(|entry| entry["id"] == block_id.to_string())
        {
            entries.push(json!({ "id": block_id, "kind": kind }));
        }
    }
    Value::Array(entries)
}

/// 从统一环境视图投影活动块环境；视图已按结构选区与公式块宿主归零。
fn current_block_environment_id(
    session: &caret_engine::Session,
    environment_view: &caret_engine::CurrentEnvironmentView,
) -> Option<NodeId> {
    // 活动块公式只由公式块高亮层呈现，不再推导其外层环境。
    if matches!(
        session.current_math_edit_target(),
        Some(MathEditTarget::Block { .. })
    ) {
        return None;
    }
    environment_view
        .block()
        .iter()
        .rev()
        .find(|entry| entry.source == caret_engine::EnvironmentSource::BlockEnvironment)
        .and_then(|entry| entry.id.clone())
}

fn current_math_block_target(session: &caret_engine::Session) -> Option<MathEditTarget> {
    if session.has_structure_selection() {
        return None;
    }
    match session.current_math_edit_target()? {
        target @ MathEditTarget::Block { .. } => Some(target.clone()),
        MathEditTarget::Inline { .. } => None,
    }
}

/// 将最终选区一次性投影为页面矩形；文本布局负责文字几何，结构环境复用环境范围。
fn math_text_selection_json(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    target: &caret_engine::MathEditTarget,
    anchor: &caret_engine::MathTextPosition,
    focus: &caret_engine::MathTextPosition,
) -> Result<Value, String> {
    let rectangles = snapshot
        .math_text_selection_geometry(target, anchor, focus)
        .ok_or_else(|| "layout.invalid_math_selection_geometry".to_owned())?;
    Ok(Value::Array(
        rectangles
            .into_iter()
            .map(|rectangle| {
                json!({
                    "pageIndex": rectangle.page_index,
                    "x0Pt": rectangle.x0_pt,
                    "y0Pt": rectangle.y0_pt,
                    "x1Pt": rectangle.x1_pt,
                    "y1Pt": rectangle.y1_pt,
                })
            })
            .collect(),
    ))
}

/// 将公式树内部同一序列的锚点/焦点范围投影为页面矩形，并按页面边界裁切。
///
/// 几何缺失按过期投影拒绝发布；越出纸张的公式排版先裁切再交给 Qt，避免整份
/// 交互快照因单条越界矩形被拒绝。
fn math_tree_selection_json(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    target: &MathEditTarget,
    anchor: &MathTreePosition,
    focus: &MathTreePosition,
) -> Result<Value, String> {
    let rectangles = snapshot
        .math_tree_selection_geometry(target, anchor, focus)
        .ok_or_else(|| "layout.invalid_math_tree_selection_geometry".to_owned())?;
    let rectangles = rectangles
        .into_iter()
        .filter_map(|rectangle| {
            let page = snapshot.pages.get(rectangle.page_index)?;
            let (mut x0, mut y0, mut x1, mut y1) = (
                rectangle.x0_pt,
                rectangle.y0_pt,
                rectangle.x1_pt,
                rectangle.y1_pt,
            );
            if ![x0, y0, x1, y1].into_iter().all(f64::is_finite) {
                return None;
            }
            x0 = x0.clamp(0.0, page.width_pt);
            y0 = y0.clamp(0.0, page.height_pt);
            x1 = x1.clamp(0.0, page.width_pt);
            y1 = y1.clamp(0.0, page.height_pt);
            (x1 > x0 && y1 > y0).then_some((rectangle.page_index, x0, y0, x1, y1))
        })
        .collect::<Vec<_>>();
    Ok(Value::Array(
        rectangles
            .into_iter()
            .map(|(page_index, x0_pt, y0_pt, x1_pt, y1_pt)| {
                json!({
                    "pageIndex": page_index,
                    "x0Pt": x0_pt,
                    "y0Pt": y0_pt,
                    "x1Pt": x1_pt,
                    "y1Pt": y1_pt,
                })
            })
            .collect(),
    ))
}

/// 发布前校验：选区与高亮几何只能来自当前会话、文档代际与字体代际。
///
/// 非组合路径的 `base_snapshot()` 已会在过期时重排，因此该检查主要保护缓存的
/// 组合预览快照（以及未来异步发布）不被迟到结果使用；它不负责检测文档内容过期。
/// 失败码 `layout.stale_highlight_snapshot` 表示快照代际与会话不一致。
fn require_current_snapshot(
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    session: &caret_engine::Session,
    font_generation: u64,
) -> Result<(), String> {
    snapshot
        .is_current_for(session, font_generation)
        .then_some(())
        .ok_or_else(|| "layout.stale_highlight_snapshot".to_owned())
}

/// 将页面局部矩形序列化为选区/高亮层共享的界面结构。
fn rectangle_values(
    rectangles: impl IntoIterator<Item = caret_engine::PageSelectionRect>,
) -> Value {
    Value::Array(
        rectangles
            .into_iter()
            .map(|rectangle| {
                json!({
                    "pageIndex": rectangle.page_index,
                    "x0Pt": rectangle.x0_pt,
                    "y0Pt": rectangle.y0_pt,
                    "x1Pt": rectangle.x1_pt,
                    "y1Pt": rectangle.y1_pt,
                })
            })
            .collect(),
    )
}

/// 将结构或普通选区投影为选区层矩形；结构范围同时包含块环境与公式块对象。
///
/// 结构选区存在却无法提供文本端点时拒绝发布，不能退回普通文本选区而静默发布空数组。
/// 选中的公式宿主不在本代快照中同样拒绝，避免覆盖上一份有效选区。
fn selection_rectangles_json(
    selection: &caret_engine::Selection,
    structure_text_bounds: Option<(Pos, Pos)>,
    has_structure_selection: bool,
    selected_environment_ids: &[NodeId],
    selected_math_block_ids: &[NodeId],
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Result<Value, String> {
    let rectangles = if let Some((anchor, focus)) = structure_text_bounds {
        snapshot
            .structure_selection_rectangles(
                &anchor,
                &focus,
                selected_environment_ids,
                selected_math_block_ids,
            )
            .ok_or_else(|| "layout.stale_selection_geometry".to_owned())?
    } else if has_structure_selection {
        // 结构选区没有可用文本端点：拒绝发布，而不是回落到普通选区发布空数组。
        return Err("layout.stale_selection_geometry".to_owned());
    } else {
        snapshot.selection_geometry(selection).unwrap_or_default()
    };
    Ok(rectangle_values(rectangles))
}

/// 将虚拟块外停靠点应用到所有返回路径共享的光标几何。
pub(super) fn caret_geometry_with_block_boundary_exit(
    block_boundary_exit: Option<(NodeId, caret_engine::Direction)>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
    caret: PageCaretGeometry,
) -> PageCaretGeometry {
    if let Some((block_id, direction)) = block_boundary_exit {
        return snapshot.block_boundary_caret(&block_id, direction, caret);
    }
    caret
}

fn block_environment_highlight_rects(
    environment_id: Option<&NodeId>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(environment_id) = environment_id else {
        return Value::Array(Vec::new());
    };
    Value::Array(
        snapshot
            .environment_ranges
            .iter()
            .filter(|range| &range.environment_id == environment_id)
            .map(|range| {
                json!({
                    "pageIndex": range.page_index,
                    "x0Pt": range.x0_pt,
                    "y0Pt": range.y0_pt,
                    "x1Pt": range.x1_pt,
                    "y1Pt": range.y1_pt,
                })
            })
            .collect(),
    )
}

/// 以活动 Equation/Align 的整栏公式几何生成块层高亮；几何来自同一代布局快照。
///
/// 活动块公式目标必然在本代排版中产生分页片段；找不到时按过期投影拒绝发布，
/// 而不是发布空高亮覆盖上一份有效状态。
fn math_block_highlight_rects(
    target: Option<&MathEditTarget>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Result<Value, String> {
    let Some(MathEditTarget::Block { block_id, .. }) = target else {
        return Ok(Value::Array(Vec::new()));
    };
    Ok(rectangle_values(
        snapshot
            .math_block_geometry(std::slice::from_ref(block_id))
            .ok_or_else(|| "layout.stale_math_highlight_geometry".to_owned())?,
    ))
}

/// 将活动数学 TextFlow island 投影为粉色高亮几何，并裁切到页面边界。
fn math_text_highlight_rects(
    context: Option<&MathTextInputContext>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(context) =
        context.filter(|context| matches!(&context.target, MathEditTarget::Block { .. }))
    else {
        return Value::Array(Vec::new());
    };
    let rectangles = snapshot
        .math_text_selection_geometry(&context.target, &context.island_start, &context.island_end)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|rectangle| {
            let page = snapshot.pages.get(rectangle.page_index)?;
            let (x0, y0, x1, y1) = (
                rectangle.x0_pt,
                rectangle.y0_pt,
                rectangle.x1_pt,
                rectangle.y1_pt,
            );
            if ![x0, y0, x1, y1].into_iter().all(f64::is_finite) {
                return None;
            }
            let x0 = x0.clamp(0.0, page.width_pt);
            let y0 = y0.clamp(0.0, page.height_pt);
            let x1 = x1.clamp(0.0, page.width_pt);
            let y1 = y1.clamp(0.0, page.height_pt);
            (x1 > x0 && y1 > y0).then_some((rectangle.page_index, x0, y0, x1, y1))
        })
        .collect::<Vec<_>>();
    Value::Array(
        rectangles
            .into_iter()
            .map(|(page_index, x0_pt, y0_pt, x1_pt, y1_pt)| {
                json!({
                    "pageIndex": page_index,
                    "x0Pt": x0_pt,
                    "y0Pt": y0_pt,
                    "x1Pt": x1_pt,
                    "y1Pt": y1_pt,
                })
            })
            .collect(),
    )
}

/// 行内环境/空公式高亮输入：文本范围、空宿主标记与公式 caret 插入槽。
struct InlineHighlightContext<'a> {
    range: Option<&'a caret_engine::InlineSpan>,
    empty_inline_host: bool,
    math_caret: Option<(caret_engine::PageCaretGeometry, u16)>,
}

/// 将当前最内层行内环境投影为布局层选区矩形。
fn inline_environment_highlight_rects(
    context: InlineHighlightContext<'_>,
    pending_style: bool,
    focus: &Pos,
    visual_affinity: caret_engine::VisualAffinity,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let InlineHighlightContext {
        range,
        empty_inline_host,
        math_caret,
    } = context;
    // 空 InlineMath 与空 InlineEnvironment 一致：不按 U+FFFC 跨度铺满隐藏盒，
    // 而以公式 caret 为中心绘制同一插入槽。Source 分支拿不到公式 caret 时退回
    // 共享 caret/行框插入槽，避免零宽对象或无意义范围高亮。
    if empty_inline_host {
        if let Some((caret, font_size)) = math_caret {
            return insertion_slot_rect_from_caret(caret, font_size, snapshot);
        }
        return insertion_slot_rect(focus, visual_affinity, snapshot);
    }
    let Some(range) = range else {
        if pending_style {
            return insertion_slot_rect(focus, visual_affinity, snapshot);
        }
        return Value::Array(Vec::new());
    };
    if range.start_utf8 == range.end_utf8 {
        return insertion_slot_rect(focus, visual_affinity, snapshot);
    }
    let selection = Selection {
        anchor: Pos {
            story: range.story.clone(),
            paragraph_id: range.paragraph_id.clone(),
            offset: range.start_utf8,
            affinity: Affinity::Downstream,
        },
        focus: Pos {
            story: range.story.clone(),
            paragraph_id: range.paragraph_id.clone(),
            offset: range.end_utf8,
            affinity: Affinity::Upstream,
        },
    };
    let mut rectangles = snapshot
        .selection_geometry(&selection)
        .unwrap_or_default()
        .into_iter()
        .map(|rectangle| {
            json!({
                "pageIndex": rectangle.page_index,
                "x0Pt": rectangle.x0_pt,
                "y0Pt": rectangle.y0_pt,
                "x1Pt": rectangle.x1_pt,
                "y1Pt": rectangle.y1_pt,
            })
        })
        .collect::<Vec<_>>();
    if pending_style
        && let Some(slot) = insertion_slot_rect(focus, visual_affinity, snapshot).as_array()
    {
        rectangles.extend(slot.iter().cloned());
    }
    Value::Array(rectangles)
}

/// 以当前行高和字号为零长度格式范围生成可见插入槽高亮。
fn insertion_slot_rect(
    focus: &Pos,
    visual_affinity: caret_engine::VisualAffinity,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(caret) = snapshot.caret_geometry(focus, visual_affinity) else {
        return Value::Array(Vec::new());
    };
    let Some(font_size) = snapshot.font_size_centipoints_at(focus, visual_affinity) else {
        return Value::Array(Vec::new());
    };
    insertion_slot_rect_from_caret(caret, font_size, snapshot)
}

/// 以给定 caret 几何与字号生成可见插入槽；空宿主公式与零长度格式范围共用。
fn insertion_slot_rect_from_caret(
    caret: caret_engine::PageCaretGeometry,
    font_size: u16,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let Some(page) = snapshot.pages.get(caret.page_index) else {
        return Value::Array(Vec::new());
    };
    let width = (f64::from(font_size) / 100.0 * 0.5)
        .max(4.0)
        .min(page.width_pt);
    let x = caret.x_pt.clamp(0.0, page.width_pt);
    let (x0, x1) = insertion_slot_horizontal_bounds(x, page.width_pt, width);
    let y0 = caret.y0_pt.clamp(0.0, page.height_pt);
    let y1 = caret.y1_pt.clamp(0.0, page.height_pt);
    if ![x0, y0, x1, y1].into_iter().all(f64::is_finite) || x1 <= x0 || y1 <= y0 {
        return Value::Array(Vec::new());
    }
    Value::Array(vec![json!({
        "pageIndex": caret.page_index,
        "x0Pt": x0,
        "y0Pt": y0,
        "x1Pt": x1,
        "y1Pt": y1,
    })])
}

/// 在字形视觉光标两侧居中放置空槽，并在页面边缘整体内移。
fn insertion_slot_horizontal_bounds(x: f64, page_width: f64, width: f64) -> (f64, f64) {
    let width = width.min(page_width);
    let x0 = (x - width / 2.0).clamp(0.0, page_width - width);
    (x0, x0 + width)
}

#[cfg(test)]
mod tests;

impl DocumentViewRuntime {
    /// 只序列化命令查询，按键过滤不读取或重建布局快照。
    pub fn command_line_state_json(&self) -> Result<String, String> {
        serde_json::to_string(&command_line_state(&self.session))
            .map_err(|error| format!("cmdline.serialize: {error}"))
    }

    /// 按当前会话和字体代际生成界面快照 JSON。
    pub fn layout_snapshot_json(&mut self) -> Result<String, String> {
        self.layout_snapshot_json_with_delta(None)
    }

    /// 发布输入法取消结果，使界面只清除 Rust 实际取消的预编辑状态。
    pub(super) fn layout_snapshot_json_after_composition_cancel(
        &mut self,
    ) -> Result<String, String> {
        let snapshot = self.layout_snapshot_json()?;
        let mut view: Value = serde_json::from_str(&snapshot)
            .map_err(|error| format!("layout.serialize: {error}"))?;
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("compositionCancelled".to_owned(), Value::Bool(true));
        serde_json::to_string(&view).map_err(|error| format!("layout.serialize: {error}"))
    }

    pub(super) fn layout_snapshot_json_with_delta(
        &mut self,
        changed_paragraph: Option<(&NodeId, usize, u64)>,
    ) -> Result<String, String> {
        if self.composition_snapshot.is_none() {
            self.base_snapshot()?;
        }
        let composing = self.composition_preview_session.is_some();
        let highlight_session = self
            .composition_preview_session
            .as_mut()
            .unwrap_or(&mut self.session);
        let environment_view = highlight_session
            .current_environment_view()
            .map_err(|error| format!("layout.inline_environment: {error}"))?;
        let math_block_target = current_math_block_target(highlight_session);
        let inline_context =
            math_block_target.is_none() && !highlight_session.has_structure_selection();
        let inline_environment_entry = inline_context
            .then(|| environment_view.innermost_inline())
            .flatten();
        let empty_inline_host =
            inline_environment_entry.is_some_and(|entry| entry.empty_inline_host);
        let inline_environment =
            inline_environment_entry.and_then(|entry| entry.inline_range.as_ref());
        let inline_math_tree_context = if empty_inline_host {
            highlight_session.math_tree_input_context()
        } else {
            None
        };
        let pending_inline_slot = inline_context
            && highlight_session.has_pending_inline_style()
            && highlight_session.selection().anchor == highlight_session.selection().focus;
        let inline_focus = highlight_session.selection().focus.clone();
        let inline_visual_affinity = highlight_session.selection_visual_affinity();
        let math_text_context = math_block_target
            .as_ref()
            .and_then(|_| highlight_session.math_text_input_context());
        let focus_environment_id =
            current_block_environment_id(highlight_session, &environment_view);
        let selection = self.session.selection().clone();
        let has_structure_selection = self.session.has_structure_selection();
        let selected_environment_ids = self.session.selected_block_environments().to_vec();
        let selected_math_block_ids = self.session.selected_math_blocks().to_vec();
        let structure_text_bounds = self.session.structure_selection_text_bounds();
        let snapshot = if composing {
            self.composition_snapshot
                .as_ref()
                .ok_or_else(|| "ime.composition_layout_unavailable".to_owned())?
        } else {
            self.last_snapshot
                .as_ref()
                .ok_or_else(|| "layout.snapshot_unavailable".to_owned())?
        };
        let expected_font_generation = self.layout.font_generation();
        require_current_snapshot(snapshot, &self.session, expected_font_generation)?;
        let inline_math_caret = inline_math_tree_context
            .and_then(|(target, position)| snapshot.math_tree_caret(&target, &position));
        let mut view = match changed_paragraph {
            Some((paragraph_id, page_start_index, base_generation)) => serde_json::to_value(
                DocumentLayoutDeltaView::for_paragraph(
                    snapshot,
                    paragraph_id,
                    page_start_index,
                    base_generation,
                )
                .ok_or_else(|| "layout.changed_paragraph_unavailable".to_owned())?,
            ),
            None => serde_json::to_value(caret_engine::DocumentLayoutView::from(snapshot)),
        }
        .map_err(|error| format!("layout.serialize: {error}"))?;
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("commandLine".to_owned(), command_line_state(&self.session));
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "currentStructure".to_owned(),
                current_structure_state(&self.session),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "blockEnvironmentHighlightRects".to_owned(),
                block_environment_highlight_rects(focus_environment_id.as_ref(), snapshot),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "mathBlockHighlightRects".to_owned(),
                math_block_highlight_rects(math_block_target.as_ref(), snapshot)?,
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "mathTextHighlightRects".to_owned(),
                math_text_highlight_rects(math_text_context.as_ref(), snapshot),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "inlineEnvironmentHighlightRects".to_owned(),
                inline_environment_highlight_rects(
                    InlineHighlightContext {
                        range: inline_environment,
                        empty_inline_host,
                        math_caret: inline_math_caret,
                    },
                    pending_inline_slot,
                    &inline_focus,
                    inline_visual_affinity,
                    snapshot,
                ),
            );
        if composing && changed_paragraph.is_some() {
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "compositionPreview".to_owned(),
                    serde_json::Value::Bool(true),
                );
        }
        let math_context = self.session.math_text_input_context();
        let math_tree_context = self.session.math_tree_input_context();
        let math_tree_selection_context = self.session.math_tree_selection_context();
        let math_geometry_context = self
            .composition_preview_session
            .as_ref()
            .and_then(caret_engine::Session::math_text_input_context)
            .or_else(|| math_context.clone());
        let math_tree_geometry_context = self
            .composition_preview_session
            .as_ref()
            .and_then(caret_engine::Session::math_tree_input_context)
            .or_else(|| math_tree_context.clone());
        let rectangles = if composing {
            Value::Array(Vec::new())
        } else if let Some(context) = &math_context {
            math_text_selection_json(snapshot, &context.target, &context.anchor, &context.focus)?
        } else if let Some((target, anchor, focus)) = &math_tree_selection_context {
            math_tree_selection_json(snapshot, target, anchor, focus)?
        } else {
            selection_rectangles_json(
                &selection,
                structure_text_bounds.clone(),
                has_structure_selection,
                &selected_environment_ids,
                &selected_math_block_ids,
                snapshot,
            )?
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("selectionRects".to_owned(), rectangles);
        if let Some(context) = &math_geometry_context {
            let (caret, font_size_centipoints) = snapshot
                .math_text_caret(&context.target, &context.focus, context.visual_affinity)
                .ok_or_else(|| "layout.invalid_math_caret_geometry".to_owned())?;
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "caretGeometry".to_owned(),
                    json!({
                        "pageIndex": caret.page_index,
                        "xPt": caret.x_pt,
                        "y0Pt": caret.y0_pt,
                        "y1Pt": caret.y1_pt,
                        "fontSizeCentipoints": font_size_centipoints,
                    }),
                );
        } else if let Some((target, position)) = &math_tree_geometry_context {
            let (caret, font_size_centipoints) = snapshot
                .math_tree_caret(target, position)
                .ok_or_else(|| "layout.invalid_math_tree_caret_geometry".to_owned())?;
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "caretGeometry".to_owned(),
                    json!({
                        "pageIndex": caret.page_index,
                        "xPt": caret.x_pt,
                        "y0Pt": caret.y0_pt,
                        "y1Pt": caret.y1_pt,
                        "fontSizeCentipoints": font_size_centipoints,
                    }),
                );
        }
        let composition_rectangles = if composing {
            self.composition_range
                .as_ref()
                .and_then(|range| match range {
                    CompositionRange::Paragraph(selection) => {
                        snapshot.composition_geometry(selection)
                    }
                    CompositionRange::MathText { anchor, focus } => {
                        math_geometry_context.as_ref().and_then(|context| {
                            snapshot.math_text_composition_geometry(&context.target, anchor, focus)
                        })
                    }
                })
                .unwrap_or_default()
                .into_iter()
                .map(|rectangle| {
                    serde_json::json!({
                        "pageIndex": rectangle.page_index,
                        "x0Pt": rectangle.x0_pt,
                        "y0Pt": rectangle.y0_pt,
                        "x1Pt": rectangle.x1_pt,
                        "y1Pt": rectangle.y1_pt,
                    })
                })
                .collect()
        } else {
            Vec::new()
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "compositionRects".to_owned(),
                serde_json::Value::Array(composition_rectangles),
            );
        let math_context = self.session.math_text_input_context();
        let logical_selection = self.session.selection();
        let (surrounding_text, cursor_position, anchor_position) = if let Some(context) =
            &math_context
        {
            (
                context.surrounding_text.clone(),
                context.focus_utf16,
                context.anchor_utf16,
            )
        } else {
            let context = self
                .last_snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot.paragraph_by_id(&logical_selection.focus.paragraph_id)
                })
                .ok_or_else(|| "ime.unknown_paragraph".to_owned())?;
            let cursor = context
                .utf16_offset(logical_selection.focus.offset)
                .ok_or_else(|| "ime.cursor_position: invalid UTF-8 boundary".to_owned())?;
            let anchor =
                if logical_selection.anchor.paragraph_id == logical_selection.focus.paragraph_id {
                    context
                        .utf16_offset(logical_selection.anchor.offset)
                        .ok_or_else(|| "ime.anchor_position: invalid UTF-8 boundary".to_owned())?
                } else {
                    cursor
                };
            (context.text.to_string(), cursor, anchor)
        };
        let input_method_context = serde_json::json!({
            "surroundingText": surrounding_text,
            "cursorPosition": cursor_position,
            "anchorPosition": anchor_position,
            "enabled": math_input_method_enabled(&self.session),
            "preferLatin": math_input_method_prefers_latin(&self.session),
        });
        let math_context_key = math_context.as_ref().map(|context| MathInteractionContext {
            surrounding_text: context.surrounding_text.clone(),
            target: context.target.clone(),
            anchor: context.anchor.clone(),
            focus: context.focus.clone(),
            anchor_visual_affinity: context.anchor_visual_affinity,
            visual_affinity: context.visual_affinity,
        });
        let math_tree_context_key =
            math_tree_context
                .as_ref()
                .map(|(target, position)| MathTreeInteractionContext {
                    target: target.clone(),
                    position: position.clone(),
                });
        self.last_interaction_context = Some(InteractionContext {
            document_generation: snapshot.document_generation,
            focused_paragraph_id: logical_selection.focus.paragraph_id.clone(),
            command_input_active: false,
            math_context: math_context_key,
            math_tree_context: math_tree_context_key,
        });
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("inputMethodContext".to_owned(), input_method_context);
        serde_json::to_string(&view).map_err(|error| format!("layout.serialize: {error}"))
    }

    /// 只序列化 caret、选区与 IME 上下文，供不改变正文布局的交互更新使用。
    pub fn interaction_state_json(&mut self) -> Result<String, String> {
        let selection = self.session.selection().clone();
        let input_method_enabled = math_input_method_enabled(&self.session);
        let visual_affinity = self.session.selection_visual_affinity();
        let command_input_context = self.session.command_line();
        let math_context = self.session.math_text_input_context();
        let math_tree_context = self.session.math_tree_input_context();
        let math_tree_selection_context = self.session.math_tree_selection_context();
        let math_context_key = math_context.as_ref().map(|context| MathInteractionContext {
            surrounding_text: context.surrounding_text.clone(),
            target: context.target.clone(),
            anchor: context.anchor.clone(),
            focus: context.focus.clone(),
            anchor_visual_affinity: context.anchor_visual_affinity,
            visual_affinity: context.visual_affinity,
        });
        let math_tree_context_key =
            math_tree_context
                .as_ref()
                .map(|(target, position)| MathTreeInteractionContext {
                    target: target.clone(),
                    position: position.clone(),
                });
        let context_key = InteractionContext {
            document_generation: self.session.document_generation(),
            focused_paragraph_id: selection.focus.paragraph_id.clone(),
            command_input_active: command_input_context.is_some(),
            math_context: math_context_key,
            math_tree_context: math_tree_context_key,
        };
        let include_surrounding_text = self.last_interaction_context.as_ref() != Some(&context_key);
        let command_line = command_line_state(&self.session);
        let command_input_context = command_input_context.map(|line| {
            let mut surrounding_text = line.prefix().to_owned();
            surrounding_text.push_str(&line.query);
            (
                surrounding_text,
                line.prefix().encode_utf16().count()
                    + line.query[..line.cursor].encode_utf16().count(),
            )
        });
        let current_structure = current_structure_state(&self.session);
        let input_method_prefer_latin = math_input_method_prefers_latin(&self.session);
        let environment_view = self
            .composition_preview_session
            .as_mut()
            .unwrap_or(&mut self.session)
            .current_environment_view()
            .map_err(|error| format!("layout.inline_environment: {error}"))?;
        let highlight_session = self
            .composition_preview_session
            .as_ref()
            .unwrap_or(&self.session);
        let math_block_target = current_math_block_target(highlight_session);
        let inline_context =
            math_block_target.is_none() && !highlight_session.has_structure_selection();
        let inline_environment_entry = inline_context
            .then(|| environment_view.innermost_inline())
            .flatten();
        let empty_inline_host =
            inline_environment_entry.is_some_and(|entry| entry.empty_inline_host);
        let inline_environment =
            inline_environment_entry.and_then(|entry| entry.inline_range.as_ref());
        let inline_math_tree_context = if empty_inline_host {
            highlight_session.math_tree_input_context()
        } else {
            None
        };
        let pending_inline_slot = inline_context
            && highlight_session.has_pending_inline_style()
            && highlight_session.selection().anchor == highlight_session.selection().focus;
        let inline_focus = highlight_session.selection().focus.clone();
        let inline_visual_affinity = highlight_session.selection_visual_affinity();
        let math_text_highlight_context = math_block_target
            .as_ref()
            .and_then(|_| highlight_session.math_text_input_context());
        let focus_environment_id =
            current_block_environment_id(highlight_session, &environment_view);
        let math_geometry_context = self
            .composition_preview_session
            .as_ref()
            .and_then(caret_engine::Session::math_text_input_context)
            .or_else(|| math_context.clone());
        let math_tree_geometry_context = self
            .composition_preview_session
            .as_ref()
            .and_then(caret_engine::Session::math_tree_input_context)
            .or_else(|| math_tree_context.clone());
        let block_boundary_exit = self.session.block_boundary_exit();
        let has_structure_selection = self.session.has_structure_selection();
        let selected_environment_ids = self.session.selected_block_environments().to_vec();
        let selected_math_block_ids = self.session.selected_math_blocks().to_vec();
        let structure_text_bounds = self.session.structure_selection_text_bounds();
        // 先确保基础快照与当前代际一致，再以不可变借用做同代校验和几何投影。
        self.base_snapshot()?;
        let expected_font_generation = self.layout.font_generation();
        let snapshot = self.current_snapshot_ref()?;
        require_current_snapshot(snapshot, &self.session, expected_font_generation)?;
        let inline_math_caret = inline_math_tree_context
            .and_then(|(target, position)| snapshot.math_tree_caret(&target, &position));
        let inline_environment_rectangles = inline_environment_highlight_rects(
            InlineHighlightContext {
                range: inline_environment,
                empty_inline_host,
                math_caret: inline_math_caret,
            },
            pending_inline_slot,
            &inline_focus,
            inline_visual_affinity,
            snapshot,
        );
        let math_block_rectangles =
            math_block_highlight_rects(math_block_target.as_ref(), snapshot)?;
        let math_text_rectangles =
            math_text_highlight_rects(math_text_highlight_context.as_ref(), snapshot);
        let body_rectangles = selection_rectangles_json(
            &selection,
            structure_text_bounds,
            has_structure_selection,
            &selected_environment_ids,
            &selected_math_block_ids,
            snapshot,
        )?;
        let rectangles = if let Some(context) = &math_geometry_context {
            math_text_selection_json(snapshot, &context.target, &context.anchor, &context.focus)?
        } else if let Some((target, anchor, focus)) = &math_tree_selection_context {
            math_tree_selection_json(snapshot, target, anchor, focus)?
        } else {
            body_rectangles
        };
        let focus = &selection.focus;
        let paragraph = snapshot
            .paragraph_by_id(&focus.paragraph_id)
            .ok_or_else(|| "ime.unknown_paragraph".to_owned())?;
        let (cursor_position, anchor_position) = if let Some(context) = &math_context {
            (context.focus_utf16, context.anchor_utf16)
        } else {
            let cursor = paragraph
                .utf16_offset(focus.offset)
                .ok_or_else(|| "ime.cursor_position: invalid UTF-8 boundary".to_owned())?;
            let anchor = if selection.anchor.paragraph_id == focus.paragraph_id {
                paragraph
                    .utf16_offset(selection.anchor.offset)
                    .ok_or_else(|| "ime.anchor_position: invalid UTF-8 boundary".to_owned())?
            } else {
                cursor
            };
            (cursor, anchor)
        };
        let mut input_method_context = serde_json::Map::new();
        if include_surrounding_text {
            input_method_context.insert(
                "surroundingText".to_owned(),
                serde_json::Value::String(math_context.as_ref().map_or_else(
                    || paragraph.text.to_string(),
                    |context| context.surrounding_text.clone(),
                )),
            );
        }
        input_method_context.insert(
            "cursorPosition".to_owned(),
            serde_json::Value::from(cursor_position),
        );
        input_method_context.insert(
            "anchorPosition".to_owned(),
            serde_json::Value::from(anchor_position),
        );
        input_method_context.insert(
            "enabled".to_owned(),
            serde_json::Value::from(input_method_enabled),
        );
        input_method_context.insert(
            "preferLatin".to_owned(),
            serde_json::Value::from(input_method_prefer_latin),
        );
        if let Some((query, cursor_utf16)) = command_input_context {
            input_method_context.insert(
                "surroundingText".to_owned(),
                serde_json::Value::String(query),
            );
            input_method_context.insert(
                "cursorPosition".to_owned(),
                serde_json::Value::from(cursor_utf16),
            );
            input_method_context.insert(
                "anchorPosition".to_owned(),
                serde_json::Value::from(cursor_utf16),
            );
        }
        let (caret, font_size_centipoints) = if let Some(context) = &math_geometry_context {
            snapshot
                .math_text_caret(&context.target, &context.focus, context.visual_affinity)
                .ok_or_else(|| "layout.invalid_math_caret_geometry".to_owned())?
        } else if let Some((target, position)) = &math_tree_geometry_context {
            snapshot
                .math_tree_caret(target, position)
                .ok_or_else(|| "layout.invalid_math_tree_caret_geometry".to_owned())?
        } else {
            let caret = snapshot
                .caret_geometry(focus, visual_affinity)
                .ok_or_else(|| "layout.invalid_caret_geometry".to_owned())?;
            let caret =
                caret_geometry_with_block_boundary_exit(block_boundary_exit, snapshot, caret);
            let font_size_centipoints =
                snapshot
                    .font_size_centipoints_at(focus, visual_affinity)
                    .ok_or_else(|| "layout.invalid_caret_font_size".to_owned())?;
            (caret, font_size_centipoints)
        };
        let serialized = serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "selectionRects": rectangles,
            "inputMethodContext": input_method_context,
            "commandLine": command_line,
            "currentStructure": current_structure,
            "blockEnvironmentHighlightRects": block_environment_highlight_rects(focus_environment_id.as_ref(), snapshot),
            "mathBlockHighlightRects": math_block_rectangles,
            "mathTextHighlightRects": math_text_rectangles,
            "inlineEnvironmentHighlightRects": inline_environment_rectangles,
            "caretGeometry": {
                "pageIndex": caret.page_index,
                "xPt": caret.x_pt,
                "y0Pt": caret.y0_pt,
                "y1Pt": caret.y1_pt,
                "fontSizeCentipoints": font_size_centipoints,
            },
        }))
        .map_err(|error| format!("layout.interaction_serialize: {error}"))?;
        self.last_interaction_context = Some(context_key);
        Ok(serialized)
    }
}

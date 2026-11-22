//! 序列化完整布局与轻量交互快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::{
    DocumentViewRuntime, InteractionContext, MathInteractionContext, MathTreeInteractionContext,
};
use caret_doc::NodeId;
use caret_engine::{
    Affinity, CommandLineWireField as Field, CompositionRange, DocumentLayoutDeltaView,
    DocumentRangeEndpoint, MathEditTarget, MathPositionHost, MathTextInputContext,
    MathTreePosition, PageCaretGeometry, PageSelectionRect, Pos, Selection, StructureClass,
    StructureKind, StructurePosition,
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
                        (Field::Alias.wire_name()): candidate.alias,
                        (Field::CommandId.wire_name()): candidate.command_id,
                        (Field::Label.wire_name()): candidate.label,
                        (Field::Available.wire_name()): candidate.available,
                        (Field::Value.wire_name()): candidate.value,
                        (Field::HasChildren.wire_name()): candidate.has_children,
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
                (Field::Active.wire_name()): true,
                (Field::Mode.wire_name()): command_line.mode.wire_name(),
                (Field::Prefix.wire_name()): command_line.prefix(),
                (Field::Query.wire_name()): &command_line.query,
                (Field::Cursor.wire_name()): command_line.cursor,
                (Field::CursorUtf16.wire_name()): cursor_utf16,
                (Field::Preedit.wire_name()): &command_line.preedit,
                (Field::PreeditCursorUtf16.wire_name()): command_line.preedit_cursor_utf16,
                (Field::CodeFontFamily.wire_name()): effective_code_font_family,
                (Field::Selected.wire_name()): command_line.selected,
                (Field::Candidates.wire_name()): candidates,
            })
        })
        .unwrap_or_else(|| {
            let (mode, prefix) = caret_engine::CommandLineMode::wire_default();
            json!({
                (Field::Active.wire_name()): false,
                (Field::Mode.wire_name()): mode,
                (Field::Prefix.wire_name()): prefix,
                (Field::Query.wire_name()): "",
                (Field::Cursor.wire_name()): 0,
                (Field::CursorUtf16.wire_name()): prefix.encode_utf16().count(),
                (Field::CodeFontFamily.wire_name()): effective_code_font_family,
                (Field::Selected.wire_name()): 0,
                (Field::Candidates.wire_name()): [],
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
            StructureKind::Table => ("table", None),
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
    Ok(rectangle_values(rectangles))
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
            (x1 > x0 && y1 > y0).then_some(PageSelectionRect {
                page_index: rectangle.page_index,
                x0_pt: x0,
                y0_pt: y0,
                x1_pt: x1,
                y1_pt: y1,
            })
        })
        .collect::<Vec<_>>();
    Ok(rectangle_values(rectangles))
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

/// 页面局部矩形在界面 JSON 中的共享字段；矩形键名、顺序与数值格式的唯一来源。
///
/// 选区、高亮、组合下划线与拖选命中条目都复用这五个字段；需要把矩形并入
/// 更大 JSON 对象时，调用方直接把返回的映射并入目标对象。
pub(super) fn rectangle_fields(rectangle: PageSelectionRect) -> serde_json::Map<String, Value> {
    serde_json::Map::from_iter([
        ("pageIndex".to_owned(), json!(rectangle.page_index)),
        ("x0Pt".to_owned(), json!(rectangle.x0_pt)),
        ("y0Pt".to_owned(), json!(rectangle.y0_pt)),
        ("x1Pt".to_owned(), json!(rectangle.x1_pt)),
        ("y1Pt".to_owned(), json!(rectangle.y1_pt)),
    ])
}

/// 将页面局部矩形序列化为选区/高亮层共享的界面结构。
fn rectangle_values(rectangles: impl IntoIterator<Item = PageSelectionRect>) -> Value {
    Value::Array(
        rectangles
            .into_iter()
            .map(|rectangle| Value::Object(rectangle_fields(rectangle)))
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
    focus: &caret_engine::Pos,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Value {
    let mut rectangles = Vec::new();
    if let Some(environment_id) = environment_id {
        rectangles.extend(
            snapshot
                .environment_ranges
                .iter()
                .filter(|range| &range.environment_id == environment_id)
                .map(|range| PageSelectionRect {
                    page_index: range.page_index,
                    x0_pt: range.x0_pt,
                    y0_pt: range.y0_pt,
                    x1_pt: range.x1_pt,
                    y1_pt: range.y1_pt,
                }),
        );
    }
    // 表格单元格与 Section/Equation 共用同一 block 高亮层；几何来自同一快照网格。
    rectangles.extend(snapshot.table_cell_highlight_rects(focus));
    rectangle_values(rectangles)
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
            (x1 > x0 && y1 > y0).then_some(PageSelectionRect {
                page_index: rectangle.page_index,
                x0_pt: x0,
                y0_pt: y0,
                x1_pt: x1,
                y1_pt: y1,
            })
        })
        .collect::<Vec<_>>();
    rectangle_values(rectangles)
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
            return rectangle_values(insertion_slot_rect_from_caret(caret, font_size, snapshot));
        }
        return rectangle_values(insertion_slot_rect(focus, visual_affinity, snapshot));
    }
    let Some(range) = range else {
        if pending_style {
            return rectangle_values(insertion_slot_rect(focus, visual_affinity, snapshot));
        }
        return Value::Array(Vec::new());
    };
    if range.start_utf8 == range.end_utf8 {
        return rectangle_values(insertion_slot_rect(focus, visual_affinity, snapshot));
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
    let mut rectangles = snapshot.selection_geometry(&selection).unwrap_or_default();
    if pending_style {
        rectangles.extend(insertion_slot_rect(focus, visual_affinity, snapshot));
    }
    rectangle_values(rectangles)
}

/// 以当前行高和字号为零长度格式范围生成可见插入槽高亮。
fn insertion_slot_rect(
    focus: &Pos,
    visual_affinity: caret_engine::VisualAffinity,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Vec<PageSelectionRect> {
    let Some(caret) = snapshot.caret_geometry(focus, visual_affinity) else {
        return Vec::new();
    };
    let Some(font_size) = snapshot.font_size_centipoints_at(focus, visual_affinity) else {
        return Vec::new();
    };
    insertion_slot_rect_from_caret(caret, font_size, snapshot)
}

/// 以给定 caret 几何与字号生成可见插入槽；空宿主公式与零长度格式范围共用。
fn insertion_slot_rect_from_caret(
    caret: caret_engine::PageCaretGeometry,
    font_size: u16,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Vec<PageSelectionRect> {
    let Some(page) = snapshot.pages.get(caret.page_index) else {
        return Vec::new();
    };
    let width = (f64::from(font_size) / 100.0 * 0.5)
        .max(4.0)
        .min(page.width_pt);
    let x = caret.x_pt.clamp(0.0, page.width_pt);
    let (x0, x1) = insertion_slot_horizontal_bounds(x, page.width_pt, width);
    let y0 = caret.y0_pt.clamp(0.0, page.height_pt);
    let y1 = caret.y1_pt.clamp(0.0, page.height_pt);
    if ![x0, y0, x1, y1].into_iter().all(f64::is_finite) || x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    vec![PageSelectionRect {
        page_index: caret.page_index,
        x0_pt: x0,
        y0_pt: y0,
        x1_pt: x1,
        y1_pt: y1,
    }]
}

/// 在字形视觉光标两侧居中放置空槽，并在页面边缘整体内移。
fn insertion_slot_horizontal_bounds(x: f64, page_width: f64, width: f64) -> (f64, f64) {
    let width = width.min(page_width);
    let x0 = (x - width / 2.0).clamp(0.0, page_width - width);
    (x0, x0 + width)
}

/// 布局与交互快照共享的高亮状态；组合预览会话优先于主会话。
///
/// 两个入口此前各自按同一顺序读取选区、视觉侧、公式编辑与结构选择；集中推导后
/// 空宿主、行内环境与待输入格式的判定只有一处来源。
struct HighlightState {
    math_block_target: Option<MathEditTarget>,
    inline_environment: Option<caret_engine::InlineSpan>,
    empty_inline_host: bool,
    inline_math_tree_context: Option<(MathEditTarget, MathTreePosition)>,
    pending_inline_slot: bool,
    inline_focus: Pos,
    inline_visual_affinity: caret_engine::VisualAffinity,
    math_text_context: Option<MathTextInputContext>,
    focus_environment_id: Option<NodeId>,
}

/// 两个快照入口共享的四个高亮层矩形；字段名沿用各自的 JSON 键。
struct HighlightRectangles {
    block_environment: Value,
    math_block: Value,
    math_text: Value,
    inline_environment: Value,
}

impl HighlightState {
    /// 以同一代快照投影四个高亮层，两个入口只在字段插入位置上不同。
    fn rectangles(
        &self,
        snapshot: &caret_engine::DocumentLayoutSnapshot,
    ) -> Result<HighlightRectangles, String> {
        let inline_math_caret = self
            .inline_math_tree_context
            .as_ref()
            .and_then(|(target, position)| snapshot.math_tree_caret(target, position));
        Ok(HighlightRectangles {
            block_environment: block_environment_highlight_rects(
                self.focus_environment_id.as_ref(),
                &self.inline_focus,
                snapshot,
            ),
            math_block: math_block_highlight_rects(self.math_block_target.as_ref(), snapshot)?,
            math_text: math_text_highlight_rects(self.math_text_context.as_ref(), snapshot),
            inline_environment: inline_environment_highlight_rects(
                InlineHighlightContext {
                    range: self.inline_environment.as_ref(),
                    empty_inline_host: self.empty_inline_host,
                    math_caret: inline_math_caret,
                },
                self.pending_inline_slot,
                &self.inline_focus,
                self.inline_visual_affinity,
                snapshot,
            ),
        })
    }
}

/// 从「组合预览优先」的高亮会话推导两个快照入口共享的高亮状态。
fn current_highlight_state(runtime: &mut DocumentViewRuntime) -> Result<HighlightState, String> {
    let highlight_session = runtime
        .composition_preview_session
        .as_mut()
        .unwrap_or(&mut runtime.session);
    let environment_view = highlight_session
        .current_environment_view()
        .map_err(|error| format!("layout.inline_environment: {error}"))?;
    let math_block_target = current_math_block_target(highlight_session);
    let inline_context =
        math_block_target.is_none() && !highlight_session.has_structure_selection();
    let inline_environment_entry = inline_context
        .then(|| environment_view.innermost_inline())
        .flatten();
    let empty_inline_host = inline_environment_entry.is_some_and(|entry| entry.empty_inline_host);
    let inline_environment = inline_environment_entry.and_then(|entry| entry.inline_range.clone());
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
    let focus_environment_id = current_block_environment_id(highlight_session, &environment_view);
    Ok(HighlightState {
        math_block_target,
        inline_environment,
        empty_inline_host,
        inline_math_tree_context,
        pending_inline_slot,
        inline_focus,
        inline_visual_affinity,
        math_text_context,
        focus_environment_id,
    })
}

/// 组合预览优先、主线会话回落的公式几何上下文；两个快照入口共用同一优先级。
fn math_geometry_contexts(
    preview_session: Option<&caret_engine::Session>,
    math_context: &Option<MathTextInputContext>,
    math_tree_context: &Option<(MathEditTarget, MathTreePosition)>,
) -> (
    Option<MathTextInputContext>,
    Option<(MathEditTarget, MathTreePosition)>,
) {
    (
        preview_session
            .and_then(caret_engine::Session::math_text_input_context)
            .or_else(|| math_context.clone()),
        preview_session
            .and_then(caret_engine::Session::math_tree_input_context)
            .or_else(|| math_tree_context.clone()),
    )
}

/// 将公式文本输入上下文投影为交互上下文缓存键。
fn math_interaction_context(context: &MathTextInputContext) -> MathInteractionContext {
    MathInteractionContext {
        surrounding_text: context.surrounding_text.clone(),
        target: context.target.clone(),
        anchor: context.anchor.clone(),
        focus: context.focus.clone(),
        anchor_visual_affinity: context.anchor_visual_affinity,
        visual_affinity: context.visual_affinity,
    }
}

/// 将公式树输入上下文投影为交互上下文缓存键。
fn math_tree_interaction_context(
    (target, position): &(MathEditTarget, MathTreePosition),
) -> MathTreeInteractionContext {
    MathTreeInteractionContext {
        target: target.clone(),
        position: position.clone(),
    }
}

/// 交互上下文缓存键；正文代际与命令态由各入口决定。
fn interaction_context(
    document_generation: u64,
    selection: &Selection,
    command_input_active: bool,
    math_context: Option<MathInteractionContext>,
    math_tree_context: Option<MathTreeInteractionContext>,
) -> InteractionContext {
    InteractionContext {
        document_generation,
        focused_paragraph_id: selection.focus.paragraph_id.clone(),
        command_input_active,
        math_context,
        math_tree_context,
    }
}

/// 公式 caret 几何；两个快照入口共享同一优先级与错误码。
fn math_caret_geometry(
    math_context: Option<&MathTextInputContext>,
    math_tree_context: Option<&(MathEditTarget, MathTreePosition)>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Result<Option<(PageCaretGeometry, u16)>, String> {
    if let Some(context) = math_context {
        return snapshot
            .math_text_caret(&context.target, &context.focus, context.visual_affinity)
            .map(Some)
            .ok_or_else(|| "layout.invalid_math_caret_geometry".to_owned());
    }
    if let Some((target, position)) = math_tree_context {
        if let Some(caret) = snapshot.math_tree_caret(target, position) {
            return Ok(Some(caret));
        }
        if let caret_engine::MathEditTarget::Block { block_id, .. } = target
            && snapshot.is_hidden_table_math(block_id)
        {
            // 被版心裁剪的 cell 公式没有可见光标；降级为空光标，不阻塞编辑快照。
            return Ok(None);
        }
        return Err("layout.invalid_math_tree_caret_geometry".to_owned());
    }
    Ok(None)
}

/// 从结构选区焦点端点生成数学光标，避免反向范围或多公式范围借用另一端派生位置。
fn pointer_selection_caret_geometry(
    session: &caret_engine::Session,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Result<Option<(PageCaretGeometry, u16)>, String> {
    let Some((_, focus)) = session.structure_selection_range() else {
        return Ok(None);
    };
    match focus {
        DocumentRangeEndpoint::InlineMathTree {
            paragraph_id,
            position,
            ..
        } => {
            let MathPositionHost::InlineMath { owner_id } = &position.host else {
                return Err("layout.invalid_inline_math_focus".to_owned());
            };
            let target = MathEditTarget::Inline {
                paragraph_id: paragraph_id.clone(),
                math_id: owner_id.clone(),
            };
            snapshot
                .math_tree_caret(&target, position)
                .map(Some)
                .ok_or_else(|| "layout.invalid_inline_math_tree_focus".to_owned())
        }
        DocumentRangeEndpoint::InlineMathText {
            paragraph_id,
            math_id,
            position,
            ..
        } => {
            let target = MathEditTarget::Inline {
                paragraph_id: paragraph_id.clone(),
                math_id: math_id.clone(),
            };
            snapshot
                .math_text_caret(&target, position, session.selection_visual_affinity())
                .map(Some)
                .ok_or_else(|| "layout.invalid_inline_math_text_focus".to_owned())
        }
        DocumentRangeEndpoint::Text(_) | DocumentRangeEndpoint::BlockEdge { .. } => Ok(None),
    }
}

/// caret 几何的界面 JSON；两个快照入口的字段名共享一处来源。
fn caret_geometry_json(caret: PageCaretGeometry, font_size_centipoints: u16) -> Value {
    json!({
        "pageIndex": caret.page_index,
        "xPt": caret.x_pt,
        "y0Pt": caret.y0_pt,
        "y1Pt": caret.y1_pt,
        "fontSizeCentipoints": font_size_centipoints,
    })
}

/// 数学选区优先、正文结构选区回落的矩形投影。
///
/// `body_rectangles` 以闭包延迟求值，使布局入口在公式分支不额外读取正文选区，而
/// 交互入口可以保持先校验正文矩形的既有顺序。
fn selection_rectangles_with_math(
    mixed: Option<&caret_engine::InlineMathRangeParts>,
    math_context: Option<&MathTextInputContext>,
    math_tree_selection_context: Option<&(MathEditTarget, MathTreePosition, MathTreePosition)>,
    body_rectangles: impl FnOnce() -> Result<Value, String>,
    snapshot: &caret_engine::DocumentLayoutSnapshot,
) -> Result<Value, String> {
    if let Some(parts) = mixed {
        let mut rectangles = body_rectangles()?
            .as_array()
            .cloned()
            .ok_or_else(|| "layout.invalid_selection_rectangles".to_owned())?;
        let math_rectangles = snapshot
            .inline_math_selection_geometry(parts)
            .ok_or_else(|| "layout.invalid_inline_math_selection_geometry".to_owned())?;
        let Value::Array(math_rectangles) = rectangle_values(math_rectangles) else {
            return Err("layout.invalid_inline_math_selection_geometry".to_owned());
        };
        rectangles.extend(math_rectangles);
        return Ok(Value::Array(rectangles));
    }
    if let Some(context) = math_context {
        return math_text_selection_json(
            snapshot,
            &context.target,
            &context.anchor,
            &context.focus,
        );
    }
    if let Some((target, anchor, focus)) = math_tree_selection_context {
        return math_tree_selection_json(snapshot, target, anchor, focus);
    }
    body_rectangles()
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
        let highlight = current_highlight_state(self)?;
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
            .insert(
                Field::CommandLine.wire_name().to_owned(),
                command_line_state(&self.session),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "currentStructure".to_owned(),
                current_structure_state(&self.session),
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "tableGuidelines".to_owned(),
                rectangle_values(snapshot.table_guidelines(&selection.focus)),
            );
        let highlight_rectangles = highlight.rectangles(snapshot)?;
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "blockEnvironmentHighlightRects".to_owned(),
                highlight_rectangles.block_environment,
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "mathBlockHighlightRects".to_owned(),
                highlight_rectangles.math_block,
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "mathTextHighlightRects".to_owned(),
                highlight_rectangles.math_text,
            );
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert(
                "inlineEnvironmentHighlightRects".to_owned(),
                highlight_rectangles.inline_environment,
            );
        if composing && changed_paragraph.is_some() {
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    Field::CompositionPreview.wire_name().to_owned(),
                    serde_json::Value::Bool(true),
                );
        }
        let math_context = self.session.math_text_input_context();
        let mixed = self.session.inline_math_range_parts();
        let math_tree_context = self.session.math_tree_input_context();
        let math_tree_selection_context = self.session.math_tree_selection_context();
        let (math_geometry_context, math_tree_geometry_context) = math_geometry_contexts(
            self.composition_preview_session.as_ref(),
            &math_context,
            &math_tree_context,
        );
        let rectangles = if composing {
            Value::Array(Vec::new())
        } else {
            selection_rectangles_with_math(
                mixed.as_ref(),
                math_context.as_ref(),
                math_tree_selection_context.as_ref(),
                || {
                    selection_rectangles_json(
                        &selection,
                        structure_text_bounds.clone(),
                        has_structure_selection,
                        &selected_environment_ids,
                        &selected_math_block_ids,
                        snapshot,
                    )
                },
                snapshot,
            )?
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("selectionRects".to_owned(), rectangles);
        let pointer_selection_caret = pointer_selection_caret_geometry(&self.session, snapshot)?;
        let active_math_caret = math_caret_geometry(
            math_geometry_context.as_ref(),
            math_tree_geometry_context.as_ref(),
            snapshot,
        )?;
        if let Some((caret, font_size_centipoints)) = pointer_selection_caret.or(active_math_caret)
        {
            view.as_object_mut()
                .ok_or_else(|| "layout.invalid_view_object".to_owned())?
                .insert(
                    "caretGeometry".to_owned(),
                    caret_geometry_json(caret, font_size_centipoints),
                );
        }
        let composition_rectangles = if composing {
            let rectangles = self
                .composition_range
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
                .unwrap_or_default();
            rectangle_values(rectangles)
        } else {
            Value::Array(Vec::new())
        };
        view.as_object_mut()
            .ok_or_else(|| "layout.invalid_view_object".to_owned())?
            .insert("compositionRects".to_owned(), composition_rectangles);
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
        let math_context_key = math_context.as_ref().map(math_interaction_context);
        let math_tree_context_key = math_tree_context
            .as_ref()
            .map(math_tree_interaction_context);
        self.last_interaction_context = Some(interaction_context(
            snapshot.document_generation,
            logical_selection,
            false,
            math_context_key,
            math_tree_context_key,
        ));
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
        let mixed = self.session.inline_math_range_parts();
        let math_tree_context = self.session.math_tree_input_context();
        let math_tree_selection_context = self.session.math_tree_selection_context();
        let math_context_key = math_context.as_ref().map(math_interaction_context);
        let math_tree_context_key = math_tree_context
            .as_ref()
            .map(math_tree_interaction_context);
        let context_key = interaction_context(
            self.session.document_generation(),
            &selection,
            command_input_context.is_some(),
            math_context_key,
            math_tree_context_key,
        );
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
        let highlight = current_highlight_state(self)?;
        let (math_geometry_context, math_tree_geometry_context) = math_geometry_contexts(
            self.composition_preview_session.as_ref(),
            &math_context,
            &math_tree_context,
        );
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
        let highlight_rectangles = highlight.rectangles(snapshot)?;
        let body_rectangles = selection_rectangles_json(
            &selection,
            structure_text_bounds,
            has_structure_selection,
            &selected_environment_ids,
            &selected_math_block_ids,
            snapshot,
        )?;
        let rectangles = selection_rectangles_with_math(
            mixed.as_ref(),
            math_geometry_context.as_ref(),
            math_tree_selection_context.as_ref(),
            || Ok(body_rectangles),
            snapshot,
        )?;
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
        let pointer_selection_caret = pointer_selection_caret_geometry(&self.session, snapshot)?;
        let active_math_caret = math_caret_geometry(
            math_geometry_context.as_ref(),
            math_tree_geometry_context.as_ref(),
            snapshot,
        )?;
        let (caret, font_size_centipoints) =
            if let Some(caret) = pointer_selection_caret.or(active_math_caret) {
                caret
            } else {
                let caret = snapshot
                    .caret_geometry(focus, visual_affinity)
                    .ok_or_else(|| "layout.invalid_caret_geometry".to_owned())?;
                let caret =
                    caret_geometry_with_block_boundary_exit(block_boundary_exit, snapshot, caret);
                let font_size_centipoints = snapshot
                    .font_size_centipoints_at(focus, visual_affinity)
                    .ok_or_else(|| "layout.invalid_caret_font_size".to_owned())?;
                (caret, font_size_centipoints)
            };
        let table_guidelines = rectangle_values(snapshot.table_guidelines(&selection.focus));
        let serialized = serde_json::to_string(&serde_json::json!({
            "documentGeneration": snapshot.document_generation.to_string(),
            "fontGeneration": snapshot.font_generation.to_string(),
            "selectionRects": rectangles,
            "tableGuidelines": table_guidelines,
            "inputMethodContext": input_method_context,
            (Field::CommandLine.wire_name()): command_line,
            "currentStructure": current_structure,
            "blockEnvironmentHighlightRects": highlight_rectangles.block_environment,
            "mathBlockHighlightRects": highlight_rectangles.math_block,
            "mathTextHighlightRects": highlight_rectangles.math_text,
            "inlineEnvironmentHighlightRects": highlight_rectangles.inline_environment,
            "caretGeometry": caret_geometry_json(caret, font_size_centipoints),
        }))
        .map_err(|error| format!("layout.interaction_serialize: {error}"))?;
        self.last_interaction_context = Some(context_key);
        Ok(serialized)
    }
}

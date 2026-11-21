//! 生产带版本信息的文档布局快照，并装配页面与投影索引。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::table_layout::{ShapedParagraphs, ShapedTable, expand_table_fragments};
use super::*;
use caret_structure::{BlockContainerPath, ContainerStep};

impl LaidOutParagraph {
    fn new(input: LaidOutParagraphInput) -> Self {
        let mut utf16_checkpoints = vec![(0, 0)];
        let mut utf16_offset = 0;
        let mut last_checkpoint = 0;
        for (byte_offset, character) in input.text.char_indices() {
            if byte_offset.saturating_sub(last_checkpoint) >= UTF16_CHECKPOINT_STRIDE_BYTES {
                utf16_checkpoints.push((byte_offset, utf16_offset));
                last_checkpoint = byte_offset;
            }
            utf16_offset += character.len_utf16();
        }
        let line_locations = vec![None; input.layout.paragraph.lines.len()];
        Self {
            id: input.source_paragraph.id.clone(),
            story: input.story,
            container: input.container,
            source_paragraph: Arc::new(input.source_paragraph),
            resolved_style: input.resolved_style,
            layout_width_pt: input.layout_width_pt,
            math_caret_height_pt: input.math_caret_height_pt,
            math_caret_baseline_pt: input.math_caret_baseline_pt,
            text: Arc::from(input.text),
            layout: Arc::new(input.layout),
            inline_math_layouts: Arc::from(input.inline_math_layouts),
            inline_math_ids: Arc::from(input.inline_math_ids),
            inline_math_source_ranges: Arc::from(input.inline_math_source_ranges),
            inline_math_text_layouts: Arc::from(input.inline_math_text_layouts),
            utf16_checkpoints: Arc::from(utf16_checkpoints),
            line_locations,
        }
    }

    fn can_reuse(
        &self,
        story: &StoryId,
        container: &BlockContainerPath,
        paragraph: &Paragraph,
        resolved_style: ResolvedParagraphStyle,
        layout_width_pt: f32,
    ) -> bool {
        self.story == *story
            && &self.container == container
            && self.source_paragraph.as_ref() == paragraph
            && self.resolved_style == resolved_style
            && self.layout_width_pt == layout_width_pt
    }

    pub(super) fn reused_for_layout(&self, story: StoryId, container: &BlockContainerPath) -> Self {
        Self {
            id: self.id.clone(),
            story,
            container: container.clone(),
            source_paragraph: Arc::clone(&self.source_paragraph),
            resolved_style: self.resolved_style,
            layout_width_pt: self.layout_width_pt,
            math_caret_height_pt: self.math_caret_height_pt,
            math_caret_baseline_pt: self.math_caret_baseline_pt,
            text: Arc::clone(&self.text),
            layout: Arc::clone(&self.layout),
            inline_math_layouts: Arc::clone(&self.inline_math_layouts),
            inline_math_ids: Arc::clone(&self.inline_math_ids),
            inline_math_source_ranges: Arc::clone(&self.inline_math_source_ranges),
            inline_math_text_layouts: Arc::clone(&self.inline_math_text_layouts),
            utf16_checkpoints: Arc::clone(&self.utf16_checkpoints),
            line_locations: vec![None; self.layout.paragraph.lines.len()],
        }
    }

    /// 将 Rust UTF-8 字节位置映射到 Qt UTF-16 代码单元位置。
    pub fn utf16_offset(&self, byte_offset: usize) -> Option<usize> {
        self.text.get(..byte_offset)?;
        let checkpoint_index = self
            .utf16_checkpoints
            .partition_point(|(byte, _)| *byte <= byte_offset)
            .saturating_sub(1);
        let (checkpoint_byte, checkpoint_utf16) = self
            .utf16_checkpoints
            .get(checkpoint_index)
            .copied()
            .unwrap_or((0, 0));
        let checkpoint_prefix = self.text.get(checkpoint_byte..byte_offset)?;
        Some(checkpoint_utf16 + checkpoint_prefix.encode_utf16().count())
    }
}

const LINE_GRID_LATIN_SAMPLE: char = 'M';
// 零推进行内文本岛使用独立的透明推进，保留可与相邻正文区分的点击槽位。
const EMPTY_INLINE_MATH_CARET_ADVANCE_PT: f32 = 1.5;

/// 可复用塑形器；调用方跨文档代际复用字体目录，避免按段落扫描系统字体。
#[derive(Default)]
pub struct DocumentLayoutService {
    pub(super) layouter: ParagraphLayouter,
    line_grid_metrics_cache: Option<LineGridMetricsCache>,
    pub(super) math_layout_engine: Option<caret_math::layout::MathLayoutEngine>,
}

#[derive(Debug, Clone, PartialEq)]
struct LineGridMetricsCache {
    font_generation: u64,
    settings: ResolvedDocumentTextSettings,
    style: ShapingStyle,
    metrics: caret_layout::FontLineMetrics,
}

pub(super) struct ParagraphShapingSettings<'a> {
    pub(super) base_style: &'a ShapingStyle,
    pub(super) text_settings: &'a ResolvedDocumentTextSettings,
    pub(super) content_width_pt: f32,
}

/// 单个段落的塑形请求；把可选复用、文档与样式设置收束成一条稳定入口。
pub(super) struct ParagraphShapingRequest<'a> {
    pub(super) document: &'a Document,
    pub(super) paragraph: &'a Paragraph,
    pub(super) story: StoryId,
    pub(super) container: &'a BlockContainerPath,
    pub(super) width_pt: f32,
    pub(super) reuse: Option<(&'a DocumentLayoutSnapshot, Option<&'a NodeId>)>,
    pub(super) settings: &'a ParagraphShapingSettings<'a>,
}

impl DocumentLayoutService {
    pub fn new() -> Self {
        Self::default()
    }

    pub(super) fn resolve_line_grid_metrics(
        &mut self,
        catalog: &mut FontCatalog,
        settings: &ResolvedDocumentTextSettings,
        style: &ShapingStyle,
        language: Option<&str>,
    ) -> Result<caret_layout::FontLineMetrics, DocumentLayoutError> {
        let font_generation = catalog.generation();
        if let Some(cache) = &self.line_grid_metrics_cache
            && cache.font_generation == font_generation
            && cache.settings == *settings
            && cache.style == *style
        {
            return Ok(cache.metrics);
        }
        let metrics = self
            .layouter
            .measure_font_metrics(
                catalog,
                settings,
                style,
                language,
                Some(LINE_GRID_LATIN_SAMPLE),
            )
            .map_err(|error| DocumentLayoutError::Shaping(error.to_string()))?;
        self.line_grid_metrics_cache = Some(LineGridMetricsCache {
            font_generation,
            settings: settings.clone(),
            style: style.clone(),
            metrics,
        });
        Ok(metrics)
    }

    /// 从 Session 同步读取文档与代际，避免调用方把旧/新文档与错误 generation 配对。
    pub fn layout_session(
        &mut self,
        session: &Session,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let mut snapshot = self.layout_with_reuse(
            session.document(),
            session.document_generation(),
            catalog,
            base_style,
            None,
            false,
        )?;
        snapshot.session_id = Some(session.session_id().clone());
        Ok(snapshot)
    }

    /// 重排一个发生暂态变化的段落，复用同一文档其他段落的已塑形结果。
    pub fn layout_session_reusing(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        self.layout_session_reusing_paragraphs(
            session,
            previous,
            Some(changed_paragraph_id),
            catalog,
            base_style,
        )
    }

    /// 复用所有正文段落塑形结果，并重算含块级公式的页面几何。
    pub fn layout_session_reusing_all_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        self.layout_session_reusing_paragraphs(session, previous, None, catalog, base_style)
    }

    fn layout_session_reusing_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: Option<&NodeId>,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let current_settings = ResolvedDocumentTextSettings::from_document_settings(
            &session.document().global_settings,
        )
        .map_err(|error| DocumentLayoutError::FontPreferences(error.to_string()))?;
        let current_content_width_pt = session
            .document()
            .page_sections
            .first()
            .and_then(|section| section.geometry().ok())
            .and_then(|geometry| geometry.content_size_pt().ok())
            .map(|(width, _)| width as f32);
        if previous.session_id.as_ref() != Some(session.session_id())
            || previous.font_generation != catalog.generation()
            || previous.resolved_text_settings != current_settings
            || previous.base_style != *base_style
            || Some(previous.content_width_pt) != current_content_width_pt
        {
            return self.layout_session(session, catalog, base_style);
        }
        let mut snapshot = self.layout_with_reuse(
            session.document(),
            session.document_generation(),
            catalog,
            base_style,
            Some((previous, changed_paragraph_id)),
            false,
        )?;
        snapshot.session_id = Some(session.session_id().clone());
        Ok(snapshot)
    }

    /// 根据单一 PageSection 布局正文。未知块先拒绝整次布局，避免静默漏画内容。
    pub fn layout(
        &mut self,
        document: &Document,
        document_generation: u64,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        self.layout_with_reuse(
            document,
            document_generation,
            catalog,
            base_style,
            None,
            true,
        )
    }

    fn shape_paragraphs(
        &mut self,
        document: &Document,
        ordered_inputs: &[LayoutInput<'_>],
        reuse: Option<(&DocumentLayoutSnapshot, Option<&NodeId>)>,
        catalog: &mut FontCatalog,
        settings: ParagraphShapingSettings<'_>,
    ) -> Result<ShapedParagraphs, DocumentLayoutError> {
        let paragraph_count = ordered_inputs
            .iter()
            .filter(|item| matches!(item, LayoutInput::Paragraph { .. }))
            .count();
        let mut paragraphs = Vec::with_capacity(paragraph_count);
        let mut resolved_styles = Vec::with_capacity(paragraph_count);
        let mut shaped_tables = Vec::new();
        for item in ordered_inputs {
            match item {
                LayoutInput::Paragraph {
                    paragraph,
                    story,
                    container,
                    width_pt,
                } => {
                    let (laid_out, style) = self.shape_paragraph(
                        catalog,
                        ParagraphShapingRequest {
                            document,
                            paragraph,
                            story: story.clone(),
                            container,
                            width_pt: *width_pt,
                            reuse,
                            settings: &settings,
                        },
                    )?;
                    paragraphs.push(laid_out);
                    resolved_styles.push(style);
                }
                LayoutInput::Table {
                    table,
                    container,
                    space_before_pt,
                    space_after_pt,
                } => {
                    let (laid_out, styles, shaped) = self.shape_table(
                        catalog,
                        super::table_layout::TableShapingRequest {
                            document,
                            table,
                            container,
                            space_before_pt: *space_before_pt,
                            space_after_pt: *space_after_pt,
                            reuse,
                            settings: &settings,
                        },
                    )?;
                    for (paragraph, style) in laid_out.into_iter().zip(styles) {
                        paragraphs.push(paragraph);
                        resolved_styles.push(style);
                    }
                    shaped_tables.extend(shaped);
                }
                LayoutInput::ExplicitBreak | LayoutInput::MathBlock { .. } => {}
            }
        }

        Ok((paragraphs, resolved_styles, shaped_tables))
    }

    pub(super) fn shape_paragraph(
        &mut self,
        catalog: &mut FontCatalog,
        request: ParagraphShapingRequest<'_>,
    ) -> Result<(LaidOutParagraph, ResolvedParagraphStyle), DocumentLayoutError> {
        let ParagraphShapingRequest {
            document,
            paragraph,
            story,
            container,
            width_pt,
            reuse,
            settings,
        } = request;
        let ParagraphShapingSettings {
            base_style,
            text_settings,
            ..
        } = settings;
        let style = resolve_paragraph_style(&document.global_settings, paragraph.layout.as_ref())
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        if let Some((previous, changed_paragraph_id)) = reuse
            && changed_paragraph_id.is_none_or(|changed_id| &paragraph.id != changed_id)
            && let Some(previous_paragraph) = previous.paragraph_by_id(&paragraph.id)
            && previous_paragraph.can_reuse(&story, container, paragraph, style, width_pt)
        {
            return Ok((
                previous_paragraph.reused_for_layout(story, container),
                style,
            ));
        }
        let line_grid_metrics = self.resolve_line_grid_metrics(
            catalog,
            text_settings,
            base_style,
            text_settings.default_language(),
        )?;
        let paragraph_caret_metrics =
            calculate_line_metrics_with_grid(line_grid_metrics, &[], &[], style.line_spacing)
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let needs_mark_metrics = paragraph_has_empty_line(paragraph);
        if needs_mark_metrics && !paragraph.mark_style.is_empty() {
            return Err(DocumentLayoutError::UnsupportedParagraphMarkStyle);
        }
        let inline_layouts = if let Some(engine) = self.math_layout_engine.as_ref() {
            let mut inline_context = MathLayoutContext {
                engine: Some(engine),
                catalog,
                settings: text_settings,
                base_style,
                text_layouter: &mut self.layouter,
                font_size_centipoints: base_style.font_size_centipoints,
                content_width_pt: width_pt,
                line_grid_metrics,
                line_spacing: style.line_spacing,
                space_before_pt: style.space_before_pt,
                space_after_pt: style.space_after_pt,
                caret_metrics: paragraph_caret_metrics,
            };
            let previous_paragraph =
                reuse.and_then(|(previous, _)| previous.paragraph_by_id(&paragraph.id));
            collect_inline_math_layouts(paragraph, &mut inline_context, previous_paragraph)?
        } else {
            InlineMathLayouts {
                objects: Vec::new(),
                layouts: Vec::new(),
                ids: Vec::new(),
                text_layouts: Vec::new(),
            }
        };
        let InlineMathLayouts {
            objects: inline_object_layouts,
            layouts: inline_math_layouts,
            ids: inline_math_ids,
            text_layouts: inline_math_text_layouts,
        } = inline_layouts;
        let (text, layout) = self
            .layouter
            .layout_with_inline_objects(
                catalog,
                paragraph,
                text_settings,
                base_style,
                ParagraphLayoutOptions {
                    width_pt,
                    spacing: style.line_spacing,
                    line_grid_metrics,
                    line_grid_font_size_centipoints: base_style.font_size_centipoints,
                    first_line_indent_chars: style.first_line_indent_chars,
                },
                &inline_object_layouts,
            )
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        Ok((
            LaidOutParagraph::new(LaidOutParagraphInput {
                source_paragraph: paragraph.clone(),
                story,
                container: container.clone(),
                resolved_style: style,
                layout_width_pt: width_pt,
                math_caret_height_pt: paragraph_caret_metrics.height_pt,
                math_caret_baseline_pt: paragraph_caret_metrics.baseline_pt,
                text,
                layout,
                inline_math_layouts,
                inline_math_ids,
                inline_math_source_ranges: inline_object_layouts
                    .iter()
                    .map(|object| object.source_bytes.clone())
                    .collect(),
                inline_math_text_layouts,
            }),
            style,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_with_reuse(
        &mut self,
        document: &Document,
        document_generation: u64,
        catalog: &mut FontCatalog,
        base_style: &ShapingStyle,
        reuse: Option<(&DocumentLayoutSnapshot, Option<&NodeId>)>,
        validate_document: bool,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        // Session 创建时已校验文档，后续命令维持结构不变量，避免每次排版重复遍历整树。
        if validate_document {
            document
                .validate()
                .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        }
        let section = document
            .page_sections
            .first()
            .ok_or_else(|| DocumentLayoutError::InvalidDocument("missing PageSection".into()))?;
        let geometry = section
            .geometry()
            .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        let (page_width_pt, page_height_pt) = geometry.page_size_pt();
        let (content_width_pt, content_height_pt) = geometry
            .content_size_pt()
            .map_err(|error| DocumentLayoutError::InvalidDocument(error.to_string()))?;
        let text_settings =
            ResolvedDocumentTextSettings::from_document_settings(&document.global_settings)
                .map_err(|error| DocumentLayoutError::FontPreferences(error.to_string()))?;
        let math_font_diagnostic = document_contains_math(&document.body)
            .then(|| text_settings.font_preferences().math_family())
            .flatten()
            .filter(|family| {
                !family.eq_ignore_ascii_case(caret_font_roles::DEFAULT_MATH_FONT_FAMILY)
            })
            .map(|requested_family| MathFontDiagnostic {
                code: if catalog
                    .families()
                    .iter()
                    .any(|family| family.name.eq_ignore_ascii_case(requested_family))
                {
                    "math.font.unsupported_family"
                } else {
                    "math.font.family_missing"
                },
                requested_family: requested_family.to_owned(),
            });
        if document_contains_math(&document.body) && self.math_layout_engine.is_none() {
            self.math_layout_engine = Some(
                caret_math::layout::MathLayoutEngine::new()
                    .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
            );
        }
        let default_paragraph_style = resolve_paragraph_style(&document.global_settings, None)
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let default_line_spacing = default_paragraph_style.line_spacing;
        let line_grid_metrics = self.resolve_line_grid_metrics(
            catalog,
            &text_settings,
            base_style,
            text_settings.default_language(),
        )?;
        let default_caret_metrics =
            calculate_line_metrics_with_grid(line_grid_metrics, &[], &[], default_line_spacing)
                .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let mut ordered_inputs = Vec::new();
        {
            let mut math_context = MathLayoutContext {
                engine: self.math_layout_engine.as_ref(),
                catalog,
                settings: &text_settings,
                base_style,
                text_layouter: &mut self.layouter,
                font_size_centipoints: base_style.font_size_centipoints,
                content_width_pt: content_width_pt as f32,
                line_grid_metrics,
                line_spacing: default_line_spacing,
                space_before_pt: default_paragraph_style.space_before_pt,
                space_after_pt: default_paragraph_style.space_after_pt,
                caret_metrics: default_caret_metrics,
            };
            collect_blocks(
                &document.body,
                &mut ordered_inputs,
                &mut BlockContainerPath::default(),
                &mut math_context,
                reuse.map(|(previous, _)| previous),
            )?;
        }

        let (mut paragraphs, resolved_styles, shaped_tables) = self.shape_paragraphs(
            document,
            &ordered_inputs,
            reuse,
            catalog,
            ParagraphShapingSettings {
                base_style,
                text_settings: &text_settings,
                content_width_pt: content_width_pt as f32,
            },
        )?;

        let page_geometries = PageGeometries::paginate_and_index(
            &ordered_inputs,
            &shaped_tables,
            &mut paragraphs,
            &resolved_styles,
            PaginationGeometry {
                page_width_pt,
                page_height_pt,
                margins_pt: [
                    geometry.margins.left,
                    geometry.margins.top,
                    geometry.margins.right,
                    geometry.margins.bottom,
                ],
                content_height_pt,
            },
        )?;
        // 消费布局输入，单次派生全部块级公式映射；值字段直接移动，只有三张映射的键
        // 需要克隆两次。
        let mut math_blocks = HashMap::new();
        let mut math_block_text_layouts = HashMap::new();
        let mut math_block_sources = HashMap::new();
        for item in ordered_inputs {
            let LayoutInput::MathBlock {
                math_id,
                source,
                layout,
                text_layouts,
                ..
            } = item
            else {
                continue;
            };
            math_blocks.insert(math_id.clone(), layout);
            math_block_text_layouts.insert(math_id.clone(), text_layouts);
            math_block_sources.insert(math_id, *source);
        }
        for table in &shaped_tables {
            for row in &table.rows {
                for cell in &row.cells {
                    for math in &cell.math_blocks {
                        math_blocks
                            .entry(math.math_id.clone())
                            .or_insert_with(|| std::sync::Arc::clone(&math.layout));
                        math_block_text_layouts
                            .entry(math.math_id.clone())
                            .or_insert_with(|| math.text_layouts.clone());
                        math_block_sources
                            .entry(math.math_id.clone())
                            .or_insert_with(|| math.source.clone());
                    }
                }
            }
        }
        let mut math_hosts = HashMap::new();
        collect_math_hosts(&document.body, &mut math_hosts);
        let math_projections = collect_math_projections(
            &page_geometries.pages,
            &paragraphs,
            &page_geometries.paragraph_lookup,
            &math_blocks,
        );
        let math_projection_index = MathProjectionIndex::new(math_projections);
        let text_diagnostics = collect_text_layout_diagnostics(&paragraphs, &math_blocks);
        let table_layouts = shaped_tables
            .iter()
            .map(|table| (table.table_id.clone(), std::sync::Arc::clone(&table.grid)))
            .collect();
        let table_math_ids = shaped_tables
            .iter()
            .flat_map(|table| &table.rows)
            .flat_map(|row| &row.cells)
            .flat_map(|cell| &cell.math_blocks)
            .map(|math| math.math_id.clone())
            .collect();
        Ok(DocumentLayoutSnapshot {
            session_id: None,
            document_generation,
            font_generation: catalog.generation(),
            math_font_diagnostic,
            text_diagnostics,
            default_font_size_centipoints: base_style.font_size_centipoints,
            content_width_pt: content_width_pt as f32,
            resolved_text_settings: text_settings,
            base_style: base_style.clone(),
            paragraph_lookup: page_geometries.paragraph_lookup,
            paragraphs,
            pages: page_geometries.pages,
            environment_ranges: page_geometries.environment_ranges,
            math_blocks,
            table_layouts,
            table_math_ids,
            math_block_text_layouts,
            math_block_sources,
            math_hosts,
            math_projection_index,
        })
    }
}

struct PaginationGeometry {
    page_width_pt: f64,
    page_height_pt: f64,
    margins_pt: [f64; 4],
    content_height_pt: f64,
}

impl PageGeometries {
    fn paginate_and_index(
        ordered_inputs: &[LayoutInput<'_>],
        shaped_tables: &[ShapedTable],
        paragraphs: &mut [LaidOutParagraph],
        resolved_styles: &[ResolvedParagraphStyle],
        geometry: PaginationGeometry,
    ) -> Result<Self, DocumentLayoutError> {
        let PaginationGeometry {
            page_width_pt,
            page_height_pt,
            margins_pt,
            content_height_pt,
        } = geometry;
        let content_width_pt = page_width_pt - margins_pt[0] - margins_pt[2];
        let mut math_block_environment_ids = HashMap::new();
        let page_layouts = {
            let mut pagination_items = Vec::with_capacity(ordered_inputs.len());
            let mut paragraph_index = 0;
            let mut table_index = 0;
            for item in ordered_inputs {
                match item {
                    LayoutInput::ExplicitBreak => {
                        pagination_items.push(PaginationItem::ExplicitPageBreak);
                    }
                    LayoutInput::Table { .. } => {
                        let shaped = shaped_tables.get(table_index).ok_or_else(|| {
                            DocumentLayoutError::InvalidDocument("missing laid out table".into())
                        })?;
                        table_index += 1;
                        let width_pt = shaped.grid.width_pt as f32;
                        let x_offset_pt = ((content_width_pt as f32 - width_pt) / 2.0).max(0.0);
                        pagination_items.push(PaginationItem::Table(
                            caret_layout::TablePaginationInput {
                                table_id: &shaped.table_id,
                                x_offset_pt,
                                layout: shaped.grid.as_ref(),
                                space_before_pt: shaped.space_before_pt,
                                space_after_pt: shaped.space_after_pt,
                            },
                        ));
                    }
                    LayoutInput::Paragraph { .. } => {
                        let paragraph = paragraphs.get(paragraph_index).ok_or_else(|| {
                            DocumentLayoutError::InvalidDocument(
                                "missing laid out paragraph".into(),
                            )
                        })?;
                        let style = resolved_styles.get(paragraph_index).ok_or_else(|| {
                            DocumentLayoutError::InvalidDocument("missing paragraph style".into())
                        })?;
                        pagination_items.push(PaginationItem::Paragraph(
                            ParagraphPaginationInput::with_style(
                                &paragraph.id,
                                &paragraph.layout.paragraph,
                                *style,
                            ),
                        ));
                        paragraph_index += 1;
                    }
                    LayoutInput::MathBlock {
                        math_id,
                        environment_ids,
                        x_offset_pt,
                        width_pt,
                        height_pt,
                        baseline_pt,
                        caret_baseline_pt,
                        caret_height_pt,
                        clip_ink_to_line_height,
                        space_before_pt,
                        space_after_pt,
                        ..
                    } => {
                        math_block_environment_ids.insert(math_id, environment_ids.as_slice());
                        pagination_items.push(PaginationItem::MathBlock(
                            caret_layout::MathBlockPaginationInput {
                                math_id,
                                x_offset_pt: *x_offset_pt,
                                width_pt: *width_pt,
                                height_pt: *height_pt,
                                baseline_pt: *baseline_pt,
                                caret_baseline_pt: *caret_baseline_pt,
                                caret_height_pt: *caret_height_pt,
                                clip_ink_to_line_height: *clip_ink_to_line_height,
                                space_before_pt: *space_before_pt,
                                space_after_pt: *space_after_pt,
                            },
                        ));
                    }
                }
            }
            paginate_paragraphs(&pagination_items, content_height_pt as f32)
                .map_err(|error| DocumentLayoutError::Pagination(error.to_string()))?
        };
        let mut pages: Vec<_> = page_layouts
            .into_iter()
            .map(|layout| PageLayoutSnapshot {
                width_pt: page_width_pt,
                height_pt: page_height_pt,
                margin_left_pt: margins_pt[0],
                margin_top_pt: margins_pt[1],
                margin_right_pt: margins_pt[2],
                margin_bottom_pt: margins_pt[3],
                layout,
                table_math_placements: Vec::new(),
            })
            .collect();
        let paragraph_lookup: HashMap<_, _> = paragraphs
            .iter()
            .enumerate()
            .map(|(index, paragraph)| (paragraph.id.clone(), index))
            .collect();
        for (page_index, page) in pages.iter().enumerate() {
            for fragment in &page.layout.fragments {
                let Some(&paragraph_index) = paragraph_lookup.get(&fragment.paragraph_id) else {
                    continue;
                };
                let Some(paragraph) = paragraphs.get_mut(paragraph_index) else {
                    continue;
                };
                let mut y0_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
                let x0_pt = page.margin_left_pt + f64::from(fragment.x_offset_pt);
                let clip = fragment.clip.map(|clip| {
                    PageClip::from_array([
                        page.margin_left_pt + f64::from(clip.x0_pt),
                        page.margin_top_pt + f64::from(clip.y0_pt),
                        page.margin_left_pt + f64::from(clip.x1_pt),
                        page.margin_top_pt + f64::from(clip.y1_pt),
                    ])
                });
                for line_index in fragment.line_range.clone() {
                    let Some(line) = paragraph.layout.paragraph.lines.get(line_index) else {
                        continue;
                    };
                    if let Some(location) = paragraph.line_locations.get_mut(line_index) {
                        *location = Some(CaretLineLocation {
                            page_index,
                            x0_pt,
                            y0_pt,
                            clip,
                        });
                    }
                    y0_pt += f64::from(line.metrics.height_pt);
                }
            }
        }
        expand_table_fragments(&mut pages, shaped_tables, paragraphs)?;
        let environment_ranges = collect_environment_ranges(
            &pages,
            paragraphs,
            &paragraph_lookup,
            &math_block_environment_ids,
        );
        Ok(Self {
            pages,
            paragraph_lookup,
            environment_ranges,
        })
    }
}

impl MathProjectionIndex {
    fn new(projections: Vec<MathPageProjection>) -> Self {
        let by_page_and_id = projections
            .iter()
            .enumerate()
            .map(|(index, projection)| ((projection.page_index, projection.math_id.clone()), index))
            .collect();
        let mut by_id: HashMap<NodeId, Vec<usize>> = HashMap::new();
        for (index, projection) in projections.iter().enumerate() {
            by_id
                .entry(projection.math_id.clone())
                .or_default()
                .push(index);
        }
        Self {
            projections,
            by_page_and_id,
            by_id,
        }
    }
}

fn collect_math_hosts(blocks: &[Block], hosts: &mut HashMap<NodeId, MathTextLayoutHost>) {
    fn inline(
        nodes: &[ParagraphContent],
        paragraph_id: &NodeId,
        hosts: &mut HashMap<NodeId, MathTextLayoutHost>,
    ) {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    hosts.insert(
                        math.id.clone(),
                        MathTextLayoutHost::inline(
                            paragraph_id.clone(),
                            math.id.clone(),
                            math.math_tree.root.id.clone(),
                        ),
                    );
                }
                FlowContent::Environment(environment) => {
                    inline(&environment.children, paragraph_id, hosts)
                }
                _ => {}
            }
        }
    }
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => inline(&paragraph.inlines, &paragraph.id, hosts),
            Block::Environment(environment) => {
                if let Some(title) = &environment.title {
                    inline(&title.inlines, &title.id, hosts);
                }
                collect_math_hosts(&environment.children, hosts);
            }
            Block::Equation(equation) => {
                hosts.insert(
                    equation.id.clone(),
                    MathTextLayoutHost::equation(
                        equation.id.clone(),
                        equation.math_tree.root.id.clone(),
                    ),
                );
            }
            Block::Align(align) => {
                hosts.insert(
                    align.id.clone(),
                    MathTextLayoutHost::align(align.id.clone(), align),
                );
            }
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        collect_math_hosts(&cell.blocks, hosts);
                    }
                }
            }
            _ => {}
        }
    }
}

fn collect_math_projections(
    pages: &[PageLayoutSnapshot],
    paragraphs: &[LaidOutParagraph],
    paragraph_lookup: &HashMap<NodeId, usize>,
    math_blocks: &HashMap<NodeId, Arc<caret_math::layout::MathLayout>>,
) -> Vec<MathPageProjection> {
    let mut projections = Vec::new();
    for (page_index, page) in pages.iter().enumerate() {
        for fragment in &page.layout.math_fragments {
            if math_blocks.contains_key(&fragment.math_id) {
                projections.push(MathPageProjection {
                    math_id: fragment.math_id.clone(),
                    page_index,
                    object_x_pt: page.margin_left_pt + f64::from(fragment.x_offset_pt),
                    clip_x_pt: None,
                    baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + f64::from(fragment.baseline_pt),
                    caret_line_top_pt: page.margin_top_pt + f64::from(fragment.y_offset_pt),
                    caret_baseline_y_pt: page.margin_top_pt
                        + f64::from(fragment.y_offset_pt)
                        + f64::from(fragment.caret_baseline_pt),
                    caret_height_pt: f64::from(fragment.caret_height_pt),
                    clip_y_pt: fragment.clip_ink_to_line_height.then_some([
                        page.margin_top_pt + f64::from(fragment.y_offset_pt),
                        page.margin_top_pt
                            + f64::from(fragment.y_offset_pt)
                            + f64::from(fragment.height_pt),
                    ]),
                    row_clips: math_blocks
                        .get(&fragment.math_id)
                        .into_iter()
                        .flat_map(|layout| &layout.row_clips)
                        .filter_map(|clip| {
                            Some(MathPageRowClip {
                                row_id: clip.row_id.clone()?,
                                y0_pt: page.margin_top_pt
                                    + f64::from(fragment.y_offset_pt)
                                    + f64::from(fragment.baseline_pt)
                                    + clip.y0,
                                y1_pt: page.margin_top_pt
                                    + f64::from(fragment.y_offset_pt)
                                    + f64::from(fragment.baseline_pt)
                                    + clip.y1,
                            })
                        })
                        .collect(),
                    row_baselines: math_blocks
                        .get(&fragment.math_id)
                        .into_iter()
                        .flat_map(|layout| &layout.row_baselines)
                        .map(|row| MathPageRowBaseline {
                            baseline_y_pt: page.margin_top_pt
                                + f64::from(fragment.y_offset_pt)
                                + f64::from(fragment.baseline_pt)
                                + row.baseline,
                        })
                        .collect(),
                    inline_index: None,
                });
            }
        }
        for placement in &page.table_math_placements {
            if !math_blocks.contains_key(&placement.math_id) {
                continue;
            }
            projections.push(MathPageProjection {
                math_id: placement.math_id.clone(),
                page_index,
                object_x_pt: placement.object_x_pt,
                baseline_y_pt: placement.baseline_y_pt,
                caret_line_top_pt: placement.caret_line_top_pt,
                caret_baseline_y_pt: placement.caret_baseline_y_pt,
                caret_height_pt: placement.caret_height_pt,
                clip_y_pt: placement.clip_y_pt,
                clip_x_pt: placement.clip_x_pt,
                row_clips: Vec::new(),
                row_baselines: Vec::new(),
                inline_index: None,
            });
        }
        for fragment in &page.layout.fragments {
            let Some(paragraph) = paragraph_lookup
                .get(&fragment.paragraph_id)
                .and_then(|index| paragraphs.get(*index))
            else {
                continue;
            };
            for line_index in fragment.line_range.clone() {
                let Some(line_box) = paragraph.layout.paragraph.lines.get(line_index) else {
                    continue;
                };
                let Some(location) = paragraph
                    .line_locations
                    .get(line_index)
                    .and_then(Option::as_ref)
                else {
                    continue;
                };
                let line_top = location.y0_pt;
                let line_x = location.x0_pt;
                let height = f64::from(line_box.metrics.height_pt);
                if let Some(line) = paragraph.layout.layout.get(line_index) {
                    for item in line.items() {
                        let PositionedGlyphLayoutItem::InlineBox(inline_box) = item else {
                            continue;
                        };
                        let Ok(index) = usize::try_from(inline_box.id) else {
                            continue;
                        };
                        let Some(math_id) = paragraph.inline_math_ids.get(index) else {
                            continue;
                        };
                        let Some(baseline_y_pt) = paragraph.layout.page_local_baseline_pt(
                            line_index,
                            line_top,
                            inline_box.y + inline_box.baseline.unwrap_or(inline_box.height),
                        ) else {
                            continue;
                        };
                        projections.push(MathPageProjection {
                            math_id: math_id.clone(),
                            page_index,
                            object_x_pt: line_x + f64::from(inline_box.x),
                            clip_x_pt: None,
                            baseline_y_pt,
                            caret_line_top_pt: line_top,
                            caret_baseline_y_pt: line_top
                                + f64::from(paragraph.math_caret_baseline_pt),
                            caret_height_pt: f64::from(paragraph.math_caret_height_pt),
                            clip_y_pt: line_box
                                .metrics
                                .clip_ink_to_line_height
                                .then_some([line_top, line_top + height])
                                .or_else(|| {
                                    location.clip.map(|clip| {
                                        [
                                            clip.y0_pt.max(line_top),
                                            clip.y1_pt.min(line_top + height),
                                        ]
                                    })
                                }),
                            row_clips: Vec::new(),
                            row_baselines: Vec::new(),
                            inline_index: Some(index),
                        });
                    }
                }
            }
        }
    }
    projections
}

fn collect_environment_ranges(
    pages: &[PageLayoutSnapshot],
    paragraphs: &[LaidOutParagraph],
    paragraph_lookup: &HashMap<NodeId, usize>,
    math_block_environment_ids: &HashMap<&NodeId, &[NodeId]>,
) -> Vec<PageEnvironmentRange> {
    let mut ranges = HashMap::<(NodeId, usize), (f64, f64)>::new();
    for (page_index, page) in pages.iter().enumerate() {
        for fragment in &page.layout.fragments {
            let Some(&paragraph_index) = paragraph_lookup.get(&fragment.paragraph_id) else {
                continue;
            };
            let Some(paragraph) = paragraphs.get(paragraph_index) else {
                continue;
            };
            let Some(first_line) = paragraph
                .line_locations
                .get(fragment.line_range.start)
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(last_line) = fragment
                .line_range
                .end
                .checked_sub(1)
                .and_then(|index| paragraph.line_locations.get(index))
                .and_then(Option::as_ref)
            else {
                continue;
            };
            let Some(line) = paragraph
                .layout
                .paragraph
                .lines
                .get(fragment.line_range.end.saturating_sub(1))
            else {
                continue;
            };
            let y0_pt = first_line.y0_pt;
            let y1_pt = last_line.y0_pt + f64::from(line.metrics.height_pt);
            for environment_id in paragraph.container.environment_ids().iter() {
                ranges
                    .entry((environment_id.clone(), page_index))
                    .and_modify(|range| {
                        range.0 = range.0.min(y0_pt);
                        range.1 = range.1.max(y1_pt);
                    })
                    .or_insert((y0_pt, y1_pt));
            }
        }
        for fragment in &page.layout.math_fragments {
            let Some(environment_ids) = math_block_environment_ids.get(&fragment.math_id) else {
                continue;
            };
            let y0_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            let y1_pt = y0_pt + f64::from(fragment.height_pt);
            for environment_id in *environment_ids {
                ranges
                    .entry((environment_id.clone(), page_index))
                    .and_modify(|range| {
                        range.0 = range.0.min(y0_pt);
                        range.1 = range.1.max(y1_pt);
                    })
                    .or_insert((y0_pt, y1_pt));
            }
        }
    }
    let mut result = ranges
        .into_iter()
        .filter_map(|((environment_id, page_index), (y0_pt, y1_pt))| {
            let page = pages.get(page_index)?;
            Some(PageEnvironmentRange {
                environment_id,
                page_index,
                x0_pt: page.margin_left_pt,
                y0_pt,
                x1_pt: page.width_pt - page.margin_right_pt,
                y1_pt,
            })
        })
        .collect::<Vec<_>>();
    result.sort_by_key(|range| (range.page_index, range.environment_id.clone()));
    result
}

/// 持有系统字体目录与复用布局器的会话级布局运行时。
pub struct DocumentLayoutRuntime {
    catalog: FontCatalog,
    service: DocumentLayoutService,
}

impl DocumentLayoutRuntime {
    /// 扫描系统字体一次，并复用目录与布局器处理后续文档修订。
    pub fn new_system() -> Result<Self, DocumentLayoutError> {
        Ok(Self {
            catalog: FontCatalog::system()?,
            service: DocumentLayoutService::new(),
        })
    }

    /// 使用当前文档和字体代际生成布局快照。
    pub fn layout_session(
        &mut self,
        session: &Session,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service
            .layout_session(session, &mut self.catalog, &base_style)
    }

    /// 复用未变段落的塑形结果，仅重新布局指定段落并计算新的分页。
    pub fn layout_session_reusing(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
        changed_paragraph_id: &NodeId,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service.layout_session_reusing(
            session,
            previous,
            changed_paragraph_id,
            &mut self.catalog,
            &base_style,
        )
    }

    /// 复用全部正文段落塑形结果，并按当前公式重新计算分页和页面投影。
    pub fn layout_session_reusing_all_paragraphs(
        &mut self,
        session: &Session,
        previous: &DocumentLayoutSnapshot,
    ) -> Result<DocumentLayoutSnapshot, DocumentLayoutError> {
        let base_style = ShapingStyle::new(
            session
                .document()
                .global_settings
                .default_font_size_centipoints,
        );
        self.service.layout_session_reusing_all_paragraphs(
            session,
            previous,
            &mut self.catalog,
            &base_style,
        )
    }

    /// 当前字体目录代际，用于丢弃过期异步结果。
    pub const fn font_generation(&self) -> u64 {
        self.catalog.generation()
    }

    /// 复制已排序的本机字体族名称，供命令选择器创建短生命周期候选快照。
    pub fn font_family_names(&self) -> Vec<String> {
        self.catalog
            .families()
            .iter()
            .map(|family| family.name.clone())
            .collect()
    }

    /// 当前 ICU Unicode 数据定义的脚本组，供文档默认字体选择器展示。
    pub fn font_script_choices(&self) -> Vec<(String, String)> {
        caret_layout::font_selection::unicode_script_choices()
    }
}

fn contains_inline_math(nodes: &[ParagraphContent]) -> bool {
    nodes.iter().any(|node| match node {
        FlowContent::Math(_) => true,
        FlowContent::Environment(environment) => contains_inline_math(&environment.children),
        FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => false,
    })
}

/// 判断公式根流是否为空宿主：内容为空或仅一个根占位符。
///
/// 与 caret-math `is_empty_host_flow` 的判据保持一致；空宿主在行内流中不占推进，
/// 只保留 MathNodeBox 作为公式 caret/命中锚点。
pub(crate) fn math_flow_is_empty_host(flow: &MathFlow) -> bool {
    flow.content.is_empty()
        || matches!(
            flow.content.as_slice(),
            [MathContent::Math(node)] if node.kind == caret_doc::MathNodeKind::Placeholder
        )
}

pub(super) fn math_flow_contains_text(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::Text(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_text),
        MathContent::Environment(environment) => math_inline_contains_text(&environment.children),
        MathContent::SoftBreak(_) | MathContent::Opaque(_) => true,
    })
}

fn math_inline_contains_text(nodes: &[FlowContent<caret_doc::MathNode>]) -> bool {
    nodes.iter().any(|node| match node {
        FlowContent::Text(_) => true,
        FlowContent::Math(math) => math.children.iter().any(math_flow_contains_text),
        FlowContent::Environment(environment) => math_inline_contains_text(&environment.children),
        FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => true,
    })
}

struct EngineMathTextShaper<'a> {
    catalog: &'a mut FontCatalog,
    settings: &'a ResolvedDocumentTextSettings,
    base_style: &'a ShapingStyle,
    text_layouter: &'a mut ParagraphLayouter,
    host: MathTextLayoutHost,
    next_token: u64,
    sidecars: HashMap<u64, MathTextSidecar>,
}

impl<'a> EngineMathTextShaper<'a> {
    fn new(
        catalog: &'a mut FontCatalog,
        settings: &'a ResolvedDocumentTextSettings,
        base_style: &'a ShapingStyle,
        text_layouter: &'a mut ParagraphLayouter,
        host: MathTextLayoutHost,
    ) -> Self {
        Self {
            catalog,
            settings,
            base_style,
            text_layouter,
            host,
            next_token: 0,
            sidecars: HashMap::new(),
        }
    }

    fn into_sidecars(self) -> HashMap<u64, MathTextSidecar> {
        self.sidecars
    }
}

impl caret_math::layout::MathTextShaper for EngineMathTextShaper<'_> {
    fn shape(
        &mut self,
        request: caret_math::layout::MathTextRequest,
    ) -> Result<caret_math::layout::MathTextShape, caret_math::layout::MathTextShapeError> {
        let font_size_pt = request.font_size_pt as f32;
        if !font_size_pt.is_finite() || font_size_pt <= 0.0 {
            return Err(caret_math::layout::MathTextShapeError::InvalidMetrics);
        }
        let spans = request
            .runs
            .iter()
            .map(|run| math_text_span(run, self.base_style, self.settings))
            .collect::<Vec<_>>();
        let layout = self
            .text_layouter
            .shape_text_island_at_font_size_pt(self.catalog, self.settings, &spans, font_size_pt)
            .map_err(|error| match error {
                caret_layout::TextIslandError::InvalidFontSize => {
                    caret_math::layout::MathTextShapeError::InvalidMetrics
                }
                caret_layout::TextIslandError::InconsistentShaping => {
                    caret_math::layout::MathTextShapeError::Failed
                }
            })?;
        let metrics = layout.metrics;
        let requested_family_missing = layout.requested_family_missing;
        let missing_requested_families = layout.missing_requested_families.clone();
        let has_missing_glyph = layout.has_missing_glyph;
        let empty_caret_metrics = if layout.text.is_empty() {
            let style = spans
                .first()
                .map(|span| span.style.clone())
                .unwrap_or_else(|| self.base_style.clone());
            let metrics_span = [caret_layout::TextIslandSpan {
                text: "M",
                style,
                underline: false,
            }];
            let metrics_layout = self
                .text_layouter
                .shape_text_island_at_font_size_pt(
                    self.catalog,
                    self.settings,
                    &metrics_span,
                    font_size_pt,
                )
                .map_err(|_| caret_math::layout::MathTextShapeError::InvalidMetrics)?;
            Some((
                metrics_layout.metrics.ascent_pt,
                metrics_layout.metrics.descent_pt,
            ))
        } else {
            None
        };
        let fallback_font_size_centipoints = visible_font_size_centipoints(font_size_pt)
            .ok_or(caret_math::layout::MathTextShapeError::InvalidMetrics)?;
        let (runs, containers) = math_text_run_boxes(&request, &layout, metrics);
        let scope_id = request
            .runs
            .iter()
            .find(|run| run.is_text_flow)
            .map(|run| &run.scope_id)
            .or_else(|| {
                request
                    .containers
                    .first()
                    .map(|container| &container.scope_id)
            })
            // SoftBreak/Opaque 可能触发塑形，但不会产生 TextFlow run 或容器。
            .or_else(|| request.runs.first().map(|run| &run.scope_id))
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        let scope_host = self
            .host
            .for_scope(scope_id)
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        let source_spans = math_text_source_spans(&request, scope_host.align_cell)?;
        let token = self.next_token;
        self.next_token = self
            .next_token
            .checked_add(1)
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        if self
            .sidecars
            .insert(
                token,
                MathTextSidecar {
                    layout: Arc::new(layout),
                    color: request.color,
                    target: self.host.target.clone(),
                    host: scope_host.host,
                    source_spans,
                    empty_caret_metrics,
                    fallback_font_size_centipoints,
                    structural_depth: request.structural_depth,
                },
            )
            .is_some()
        {
            return Err(caret_math::layout::MathTextShapeError::Failed);
        }
        Ok(caret_math::layout::MathTextShape {
            token,
            width: f64::from(metrics.advance_pt),
            ascent: f64::from(metrics.ascent_pt),
            descent: f64::from(metrics.descent_pt),
            requested_family_missing,
            missing_requested_families,
            has_missing_glyph,
            runs,
            containers,
        })
    }
}

pub(super) fn math_text_span<'a>(
    run: &'a caret_math::layout::MathTextRun,
    base_style: &ShapingStyle,
    settings: &ResolvedDocumentTextSettings,
) -> caret_layout::TextIslandSpan<'a> {
    let mut style = run
        .run
        .as_ref()
        .map_or_else(|| base_style.clone(), |run| base_style.with_run(run));
    // 数学 Font 环境已在 request.style 中解析；Run.props 不保存局部字体。
    style.requested_family = run
        .style
        .font_family
        .clone()
        .or_else(|| base_style.requested_family.clone());
    style.font_role = caret_layout::FontRole::Body;
    style.bold = run.style.bold.unwrap_or(style.bold);
    style.italic = run.style.italic.unwrap_or(style.italic);
    style.language = style
        .language
        .or_else(|| settings.default_language().map(str::to_owned));
    caret_layout::TextIslandSpan {
        text: &run.text,
        style,
        underline: run
            .run
            .as_ref()
            .is_some_and(caret_layout::run_has_underline),
    }
}

fn math_text_run_boxes(
    request: &caret_math::layout::MathTextRequest,
    shaped: &TextIslandLayout,
    metrics: caret_layout::TextIslandMetrics,
) -> (
    Vec<caret_math::layout::MathTextRunBox>,
    Vec<caret_math::layout::MathTextRunBox>,
) {
    if shaped.layout.lines().next().is_none() {
        let runs = request
            .runs
            .iter()
            .map(|run| caret_math::layout::MathTextRunBox {
                id: None,
                scope_id: run.scope_id.clone(),
                path: run.path.clone(),
                run_index: run.run_index,
                is_text_flow: run.is_text_flow,
                is_flow: false,
                is_container: false,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            })
            .collect();
        let containers = request
            .containers
            .iter()
            .map(|container| caret_math::layout::MathTextRunBox {
                id: container.id.clone(),
                scope_id: container.scope_id.clone(),
                path: container.path.clone(),
                run_index: None,
                is_text_flow: false,
                is_flow: container.is_flow,
                is_container: container.is_container,
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
                baseline: 0.0,
            })
            .collect();
        return (runs, containers);
    }
    let mut spans = Vec::with_capacity(request.runs.len());
    let mut byte_offset = 0_usize;
    for run in &request.runs {
        let end = byte_offset.saturating_add(run.text.len());
        spans.push(byte_offset..end);
        byte_offset = end;
    }
    let first_baseline = shaped
        .layout
        .lines()
        .next()
        .map_or(0.0, |line| line.metrics().baseline);
    let island_top = first_baseline - metrics.ascent_pt;
    let mut clusters = Vec::<(std::ops::Range<usize>, f32, f32, f32, f32, f32, bool)>::new();
    for line in shaped.layout.lines() {
        let mut x = line.metrics().inline_min_coord + line.metrics().offset;
        let y0 = line.metrics().content_block_min_coord - island_top;
        let y1 = line.metrics().content_block_max_coord - island_top;
        let baseline = line.metrics().baseline - island_top;
        for run in line.runs() {
            for cluster in run.visual_clusters() {
                let advance = cluster.advance();
                let range = cluster.text_range();
                if !x.is_finite() || !advance.is_finite() || advance < 0.0 {
                    continue;
                }
                clusters.push((range, x, x + advance, y0, y1, baseline, cluster.is_rtl()));
                x += advance;
            }
        }
    }
    clusters.sort_unstable_by_key(|(range, ..)| (range.start, range.end));
    let ends_with_hard_break = shaped.text.chars().next_back().is_some_and(|character| {
        matches!(
            character,
            '\n' | '\r' | '\u{000b}' | '\u{000c}' | '\u{0085}' | '\u{2028}' | '\u{2029}'
        )
    });
    let bounds_for_range = |span: &std::ops::Range<usize>| {
        let first = clusters.partition_point(|(range, ..)| range.end <= span.start);
        if span.is_empty() {
            if let Some(bounds) =
                shaped.cursor_geometry_at(span.start, VisualAffinity::Downstream, 0.0)
            {
                let midpoint = ((bounds.y0 + bounds.y1) * 0.5) as f32;
                let baseline = shaped
                    .layout
                    .lines()
                    .find_map(|line| {
                        let line_metrics = line.metrics();
                        (line_metrics.block_min_coord <= midpoint
                            && midpoint <= line_metrics.block_max_coord)
                            .then_some(line_metrics.baseline - island_top)
                    })
                    .unwrap_or(metrics.ascent_pt);
                return Some((
                    bounds.x0 as f32,
                    bounds.y0 as f32 - island_top,
                    bounds.x1 as f32,
                    bounds.y1 as f32 - island_top,
                    baseline,
                ));
            }
            let cluster = clusters
                .get(first.wrapping_sub(1))
                .filter(|(range, ..)| range.end == span.start)
                .map(|(_, x0, x1, y0, y1, baseline, is_rtl)| {
                    let x = if *is_rtl { *x0 } else { *x1 };
                    (x, *y0, x, *y1, *baseline)
                })
                .or_else(|| {
                    clusters
                        .get(first)
                        .filter(|(range, ..)| range.start == span.start)
                        .map(|(_, x0, x1, y0, y1, baseline, is_rtl)| {
                            let x = if *is_rtl { *x1 } else { *x0 };
                            (x, *y0, x, *y1, *baseline)
                        })
                });
            return cluster;
        }
        let mut x0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y0 = f32::INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        let mut first_baseline = None;
        for (range, cluster_x0, cluster_x1, cluster_y0, cluster_y1, baseline, _) in
            clusters.iter().skip(first)
        {
            if range.start >= span.end {
                break;
            }
            if span.start < range.end {
                x0 = x0.min(*cluster_x0);
                x1 = x1.max(*cluster_x1);
                y0 = y0.min(*cluster_y0);
                y1 = y1.max(*cluster_y1);
                first_baseline.get_or_insert(*baseline);
            }
        }
        // 最末换行后的空行没有 cluster，但末尾 caret 仍属于结束于此的 Run 和环境。
        if ends_with_hard_break
            && span.end == shaped.text.len()
            && let Some(bounds) =
                shaped.cursor_geometry_at(span.end, VisualAffinity::Downstream, 0.0)
        {
            let caret_x = bounds.x0 as f32;
            let caret_y0 = bounds.y0 as f32 - island_top;
            let caret_y1 = bounds.y1 as f32 - island_top;
            if caret_x.is_finite() && caret_y0.is_finite() && caret_y1.is_finite() {
                x0 = x0.min(caret_x);
                x1 = x1.max(caret_x);
                y0 = y0.min(caret_y0);
                y1 = y1.max(caret_y1);
                if first_baseline.is_none() {
                    let midpoint = ((bounds.y0 + bounds.y1) * 0.5) as f32;
                    first_baseline = shaped.layout.lines().find_map(|line| {
                        let metrics = line.metrics();
                        (metrics.block_min_coord <= midpoint && midpoint <= metrics.block_max_coord)
                            .then_some(metrics.baseline - island_top)
                    });
                }
            }
        }
        if !x0.is_finite() {
            Some((0.0, 0.0, 0.0, 0.0, 0.0))
        } else {
            Some((x0, y0, x1, y1, first_baseline.unwrap_or(y0)))
        }
    };
    let make_box = |id: Option<caret_doc::NodeId>,
                    scope_id: caret_doc::NodeId,
                    path: Vec<caret_doc::MathPathStep>,
                    run_index: Option<usize>,
                    is_text_flow: bool,
                    is_flow: bool,
                    is_container: bool,
                    span: &std::ops::Range<usize>| {
        let (x0, y0, x1, y1, baseline) =
            bounds_for_range(span).unwrap_or((0.0, 0.0, 0.0, 0.0, 0.0));
        caret_math::layout::MathTextRunBox {
            id,
            scope_id,
            path,
            run_index,
            is_text_flow,
            is_flow,
            is_container,
            x: f64::from(x0),
            y: f64::from(y0),
            width: f64::from((x1 - x0).max(0.0)),
            height: f64::from((y1 - y0).max(0.0)),
            baseline: f64::from(baseline - y0),
        }
    };
    let runs = request
        .runs
        .iter()
        .zip(spans)
        .map(|(run, span)| {
            make_box(
                None,
                run.scope_id.clone(),
                run.path.clone(),
                run.run_index,
                run.is_text_flow,
                false,
                false,
                &span,
            )
        })
        .collect();
    let containers = request
        .containers
        .iter()
        .map(|container| {
            make_box(
                container.id.clone(),
                container.scope_id.clone(),
                container.path.clone(),
                None,
                false,
                container.is_flow,
                container.is_container,
                &(container.start_byte..container.end_byte),
            )
        })
        .collect();
    (runs, containers)
}

fn math_text_source_spans(
    request: &caret_math::layout::MathTextRequest,
    align_cell: Option<(usize, usize)>,
) -> Result<Vec<MathTextSourceSpan>, caret_math::layout::MathTextShapeError> {
    let mut spans = Vec::new();
    let mut byte_offset = 0_usize;
    let mut flow_key: Option<(NodeId, Vec<MathPathStep>)> = None;
    let mut flow_offset = 0_usize;
    for run in &request.runs {
        let end = byte_offset
            .checked_add(run.text.len())
            .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        if run.is_text_flow {
            let key = (run.scope_id.clone(), run.path.clone());
            if flow_key.as_ref() != Some(&key) {
                flow_key = Some(key);
                flow_offset = 0;
            }
            spans.push(MathTextSourceSpan {
                range: byte_offset..end,
                flow_offset,
                path: MathTextFlowPath::from_layout_path(run.path.clone(), align_cell),
            });
            flow_offset = flow_offset
                .checked_add(run.text.len())
                .ok_or(caret_math::layout::MathTextShapeError::Failed)?;
        }
        byte_offset = end;
    }
    Ok(spans)
}

fn collect_text_layout_diagnostics(
    paragraphs: &[LaidOutParagraph],
    math_blocks: &HashMap<NodeId, Arc<caret_math::layout::MathLayout>>,
) -> Vec<TextLayoutDiagnostic> {
    let mut diagnostics = Vec::new();
    for paragraph in paragraphs {
        for family in &paragraph.layout.missing_requested_families {
            diagnostics.push(TextLayoutDiagnostic {
                node_id: paragraph.id.clone(),
                node_kind: "paragraph",
                code: "text.font.family_missing",
                detail: family.clone(),
            });
        }
        if paragraph.layout.has_missing_glyph {
            diagnostics.push(TextLayoutDiagnostic {
                node_id: paragraph.id.clone(),
                node_kind: "paragraph",
                code: "text.glyph.missing",
                detail: "paragraph contains an unavailable glyph".to_owned(),
            });
        }
        for (math_id, layout) in paragraph
            .inline_math_ids
            .iter()
            .zip(paragraph.inline_math_layouts.iter())
        {
            diagnostics.extend(
                layout
                    .diagnostics
                    .iter()
                    .map(|diagnostic| TextLayoutDiagnostic {
                        node_id: math_id.clone(),
                        node_kind: "math",
                        code: diagnostic.code,
                        detail: diagnostic.detail.clone(),
                    }),
            );
        }
    }
    diagnostics.extend(math_blocks.iter().flat_map(|(math_id, layout)| {
        layout
            .diagnostics
            .iter()
            .map(|diagnostic| TextLayoutDiagnostic {
                node_id: math_id.clone(),
                node_kind: "math",
                code: diagnostic.code,
                detail: diagnostic.detail.clone(),
            })
    }));
    diagnostics.sort_by(|left, right| {
        left.node_id
            .to_string()
            .cmp(&right.node_id.to_string())
            .then_with(|| left.node_kind.cmp(right.node_kind))
            .then_with(|| left.code.cmp(right.code))
            .then_with(|| left.detail.cmp(&right.detail))
    });
    diagnostics
}

fn document_contains_math(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph(paragraph) => contains_inline_math(&paragraph.inlines),
        Block::Environment(environment) => {
            environment
                .title
                .as_ref()
                .is_some_and(|paragraph| contains_inline_math(&paragraph.inlines))
                || document_contains_math(&environment.children)
        }
        Block::Equation(_) | Block::Align(_) => true,
        Block::Table(table) => table.rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| document_contains_math(&cell.blocks))
        }),
        Block::ExplicitBreak(_) | Block::Opaque(_) => false,
    })
}

struct InlineMathLayouts {
    objects: Vec<InlineObjectLayout>,
    layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    ids: Vec<NodeId>,
    text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
}

struct InlineMathLayoutBuilder<'c, 'a> {
    context: &'c mut MathLayoutContext<'a>,
    base_style: &'c ShapingStyle,
    paragraph_id: NodeId,
    previous: Option<&'c LaidOutParagraph>,
    previous_inline_math: HashMap<NodeId, (usize, &'c caret_doc::InlineMath)>,
    font_size_pt: f64,
    offset: usize,
    objects: Vec<InlineObjectLayout>,
    layouts: Vec<Arc<caret_math::layout::MathLayout>>,
    ids: Vec<NodeId>,
    text_layouts: Vec<HashMap<u64, MathTextSidecar>>,
}

impl InlineMathLayoutBuilder<'_, '_> {
    fn visit(&mut self, nodes: &[ParagraphContent]) -> Result<(), DocumentLayoutError> {
        for node in nodes {
            match node {
                FlowContent::Text(text) => {
                    for run in &text.runs {
                        self.offset = self.offset.checked_add(run.text.len()).ok_or_else(|| {
                            DocumentLayoutError::Math("inline text offset overflow".to_owned())
                        })?;
                    }
                }
                FlowContent::Math(math) => {
                    let start = self.offset;
                    self.offset =
                        self.offset
                            .checked_add('\u{fffc}'.len_utf8())
                            .ok_or_else(|| {
                                DocumentLayoutError::Math("inline math offset overflow".to_owned())
                            })?;
                    let engine = self
                        .context
                        .engine
                        .ok_or(DocumentLayoutError::UnsupportedContent)?;
                    let reused = self.previous.and_then(|previous| {
                        let (index, source) = self.previous_inline_math.get(&math.id)?;
                        if *source != math {
                            return None;
                        }
                        Some((
                            Arc::clone(previous.inline_math_layouts.get(*index)?),
                            previous.inline_math_text_layouts.get(*index)?.clone(),
                        ))
                    });
                    let (layout, sidecars) = if let Some(reused) = reused {
                        reused
                    } else if math_flow_contains_text(&math.math_tree.root) {
                        let mut shaper = EngineMathTextShaper::new(
                            self.context.catalog,
                            self.context.settings,
                            self.base_style,
                            self.context.text_layouter,
                            MathTextLayoutHost::inline(
                                self.paragraph_id.clone(),
                                math.id.clone(),
                                math.math_tree.root.id.clone(),
                            ),
                        );
                        let layout = engine
                            .layout_with_text_shaper_and_templates(
                                &math.math_tree.root,
                                self.font_size_pt,
                                caret_math::layout::MathStyle::Inline,
                                self.context.settings.inline_environment_templates(),
                                &mut shaper,
                            )
                            .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                        (layout, shaper.into_sidecars())
                    } else if !self
                        .context
                        .settings
                        .inline_environment_templates()
                        .is_empty()
                    {
                        (
                            Arc::new(
                                caret_math::layout::layout_math_with_templates(
                                    engine.font(),
                                    &math.math_tree.root,
                                    self.font_size_pt,
                                    caret_math::layout::MathStyle::Inline,
                                    self.context.settings.inline_environment_templates(),
                                )
                                .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                            ),
                            HashMap::new(),
                        )
                    } else {
                        (
                            engine
                                .layout(
                                    &math.math_tree.root,
                                    self.font_size_pt,
                                    caret_math::layout::MathStyle::Inline,
                                )
                                .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                            HashMap::new(),
                        )
                    };
                    let as_f32 = |value: f64| {
                        let value = value as f32;
                        value.is_finite().then_some(value)
                    };
                    // 空宿主只保留节点盒供 caret/命中使用，不占正文推进；
                    // 零推进文本岛仍用透明最小推进保留点击槽位。
                    let advance_pt = as_f32(layout.inline_advance)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math advance".to_owned())
                        })?
                        .max(0.0);
                    let advance_pt = if advance_pt == 0.0
                        && sidecars
                            .values()
                            .any(|sidecar| sidecar.layout.metrics.advance_pt <= 0.0)
                    {
                        EMPTY_INLINE_MATH_CARET_ADVANCE_PT
                    } else {
                        advance_pt
                    };
                    let ascent_pt = as_f32(layout.ascent)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math ascent".to_owned())
                        })?
                        .max(0.0);
                    let descent_pt = as_f32(layout.descent)
                        .ok_or_else(|| {
                            DocumentLayoutError::Math("invalid inline math descent".to_owned())
                        })?
                        .max(0.0);
                    let id = u64::try_from(self.layouts.len()).map_err(|_| {
                        DocumentLayoutError::Math("too many inline math objects".to_owned())
                    })?;
                    self.objects.push(InlineObjectLayout {
                        id,
                        source_bytes: start..self.offset,
                        advance_pt,
                        ascent_pt,
                        descent_pt,
                    });
                    self.layouts.push(layout);
                    self.ids.push(math.id.clone());
                    self.text_layouts.push(sidecars);
                }
                FlowContent::Environment(environment) => self.visit(&environment.children)?,
                FlowContent::SoftBreak(_) => {
                    self.offset = self.offset.checked_add('\n'.len_utf8()).ok_or_else(|| {
                        DocumentLayoutError::Math("inline break offset overflow".to_owned())
                    })?;
                }
                FlowContent::Opaque(_) => {
                    self.offset =
                        self.offset
                            .checked_add('\u{fffc}'.len_utf8())
                            .ok_or_else(|| {
                                DocumentLayoutError::Math(
                                    "opaque inline offset overflow".to_owned(),
                                )
                            })?;
                }
            }
        }
        Ok(())
    }
}

fn index_previous_inline_math(
    previous: &LaidOutParagraph,
) -> HashMap<NodeId, (usize, &caret_doc::InlineMath)> {
    fn collect<'a>(
        nodes: &'a [ParagraphContent],
        sources: &mut HashMap<NodeId, &'a caret_doc::InlineMath>,
    ) {
        for node in nodes {
            match node {
                FlowContent::Math(math) => {
                    sources.insert(math.id.clone(), math);
                }
                FlowContent::Environment(environment) => collect(&environment.children, sources),
                FlowContent::Text(_) | FlowContent::SoftBreak(_) | FlowContent::Opaque(_) => {}
            }
        }
    }

    let mut sources = HashMap::new();
    collect(&previous.source_paragraph.inlines, &mut sources);
    previous
        .inline_math_ids
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            sources
                .remove(id)
                .map(|source| (id.clone(), (index, source)))
        })
        .collect()
}

fn collect_inline_math_layouts(
    paragraph: &Paragraph,
    context: &mut MathLayoutContext<'_>,
    previous: Option<&LaidOutParagraph>,
) -> Result<InlineMathLayouts, DocumentLayoutError> {
    let font_size_pt =
        DocumentUnits::centipoints_to_points(u32::from(context.font_size_centipoints));
    let math_base_style = resolved_math_base_style(
        context.base_style,
        context.settings,
        paragraph.language.as_deref(),
    );
    let previous =
        previous.filter(|previous| previous.source_paragraph.language == paragraph.language);
    let previous_inline_math = previous.map(index_previous_inline_math).unwrap_or_default();
    let mut builder = InlineMathLayoutBuilder {
        context,
        base_style: &math_base_style,
        paragraph_id: paragraph.id.clone(),
        previous,
        previous_inline_math,
        font_size_pt,
        offset: 0,
        objects: Vec::new(),
        layouts: Vec::new(),
        ids: Vec::new(),
        text_layouts: Vec::new(),
    };
    builder.visit(&paragraph.inlines)?;
    Ok(InlineMathLayouts {
        objects: builder.objects,
        layouts: builder.layouts,
        ids: builder.ids,
        text_layouts: builder.text_layouts,
    })
}

pub(super) fn resolved_math_base_style(
    base_style: &ShapingStyle,
    settings: &ResolvedDocumentTextSettings,
    paragraph_language: Option<&str>,
) -> ShapingStyle {
    let mut style = base_style.clone();
    style.language = paragraph_language
        .map(str::to_owned)
        .or_else(|| settings.default_language().map(str::to_owned))
        .or_else(|| base_style.language.clone());
    style
}

pub(super) struct MathLayoutContext<'a> {
    pub(super) engine: Option<&'a caret_math::layout::MathLayoutEngine>,
    pub(super) catalog: &'a mut FontCatalog,
    pub(super) settings: &'a ResolvedDocumentTextSettings,
    pub(super) base_style: &'a ShapingStyle,
    pub(super) text_layouter: &'a mut ParagraphLayouter,
    pub(super) font_size_centipoints: u16,
    pub(super) content_width_pt: f32,
    pub(super) line_grid_metrics: FontLineMetrics,
    pub(super) line_spacing: LineSpacing,
    pub(super) space_before_pt: f32,
    pub(super) space_after_pt: f32,
    pub(super) caret_metrics: LineMetrics,
}

enum LayoutInput<'a> {
    Paragraph {
        paragraph: &'a Paragraph,
        story: StoryId,
        container: BlockContainerPath,
        /// 该段落可用的版心宽度；正文为全局版心，单元格为列内容宽度。
        width_pt: f32,
    },
    Table {
        table: &'a caret_doc::Table,
        container: BlockContainerPath,
        space_before_pt: f32,
        space_after_pt: f32,
    },
    ExplicitBreak,
    MathBlock {
        math_id: NodeId,
        source: Box<MathBlockSource>,
        layout: Arc<caret_math::layout::MathLayout>,
        text_layouts: HashMap<u64, MathTextSidecar>,
        environment_ids: Vec<NodeId>,
        x_offset_pt: f32,
        width_pt: f32,
        height_pt: f32,
        baseline_pt: f32,
        caret_baseline_pt: f32,
        caret_height_pt: f32,
        clip_ink_to_line_height: bool,
        space_before_pt: f32,
        space_after_pt: f32,
    },
}

fn reuse_math_block(
    previous: Option<&DocumentLayoutSnapshot>,
    math_id: &NodeId,
    source: &MathBlockSource,
) -> Option<(
    Arc<caret_math::layout::MathLayout>,
    HashMap<u64, MathTextSidecar>,
)> {
    let previous = previous?;
    if previous.math_block_sources.get(math_id) != Some(source) {
        return None;
    }
    Some((
        Arc::clone(previous.math_blocks.get(math_id)?),
        previous.math_block_text_layouts.get(math_id)?.clone(),
    ))
}

fn collect_blocks<'a>(
    blocks: &'a [Block],
    ordered: &mut Vec<LayoutInput<'a>>,
    container: &mut BlockContainerPath,
    context: &mut MathLayoutContext<'_>,
    previous: Option<&DocumentLayoutSnapshot>,
) -> Result<(), DocumentLayoutError> {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => ordered.push(LayoutInput::Paragraph {
                paragraph,
                story: StoryId::Body,
                container: container.clone(),
                width_pt: context.content_width_pt,
            }),
            Block::Environment(environment) => {
                container.push(ContainerStep::Environment(environment.id.clone()));
                if let Some(title) = &environment.title {
                    ordered.push(LayoutInput::Paragraph {
                        paragraph: title,
                        story: StoryId::EnvironmentTitle(environment.id.clone()),
                        container: container.clone(),
                        width_pt: context.content_width_pt,
                    });
                }
                collect_blocks(&environment.children, ordered, container, context, previous)?;
                container.pop();
            }
            Block::ExplicitBreak(page_break) => {
                if !page_break.kind.is_empty() && page_break.kind != "page" {
                    return Err(DocumentLayoutError::UnknownBreakKind(
                        page_break.kind.clone(),
                    ));
                }
                ordered.push(LayoutInput::ExplicitBreak);
            }
            Block::Equation(equation) => {
                let source = MathBlockSource::Equation(equation.clone());
                let (layout, text_layouts) = measure_math_block(&source, context, previous)?;
                push_math_block(
                    ordered,
                    source,
                    layout,
                    text_layouts,
                    container.environment_ids(),
                    context,
                )?;
            }
            Block::Align(align) => {
                let source = MathBlockSource::Align(align.clone());
                let (layout, text_layouts) = measure_math_block(&source, context, previous)?;
                push_math_block(
                    ordered,
                    source,
                    layout,
                    text_layouts,
                    container.environment_ids(),
                    context,
                )?;
            }
            Block::Table(table) => ordered.push(LayoutInput::Table {
                table,
                container: container.clone(),
                space_before_pt: context.space_before_pt,
                space_after_pt: context.space_after_pt,
            }),
            Block::Opaque(_) => return Err(DocumentLayoutError::UnsupportedContent),
        }
    }
    Ok(())
}

/// 测量显示公式（Equation/Align）；正文与单元格共用同一塑形/复用路径。
pub(super) fn measure_math_block(
    source: &MathBlockSource,
    context: &mut MathLayoutContext<'_>,
    previous: Option<&DocumentLayoutSnapshot>,
) -> Result<
    (
        Arc<caret_math::layout::MathLayout>,
        HashMap<u64, MathTextSidecar>,
    ),
    DocumentLayoutError,
> {
    let math_id = match source {
        MathBlockSource::Equation(equation) => &equation.id,
        MathBlockSource::Align(align) => &align.id,
    };
    if let Some(reused) = reuse_math_block(previous, math_id, source) {
        return Ok(reused);
    }
    let engine = context
        .engine
        .ok_or(DocumentLayoutError::UnsupportedContent)?;
    match source {
        MathBlockSource::Equation(equation) => {
            if math_flow_contains_text(&equation.math_tree.root) {
                let math_base_style =
                    resolved_math_base_style(context.base_style, context.settings, None);
                let mut shaper = EngineMathTextShaper::new(
                    context.catalog,
                    context.settings,
                    &math_base_style,
                    context.text_layouter,
                    MathTextLayoutHost::equation(
                        equation.id.clone(),
                        equation.math_tree.root.id.clone(),
                    ),
                );
                let layout = engine
                    .layout_with_text_shaper_and_templates(
                        &equation.math_tree.root,
                        DocumentUnits::centipoints_to_points(u32::from(
                            context.font_size_centipoints,
                        )),
                        caret_math::layout::MathStyle::Display,
                        context.settings.inline_environment_templates(),
                        &mut shaper,
                    )
                    .map_err(|error| DocumentLayoutError::Math(error.to_string()))?;
                Ok((layout, shaper.into_sidecars()))
            } else if !context.settings.inline_environment_templates().is_empty() {
                Ok((
                    Arc::new(
                        caret_math::layout::layout_math_with_templates(
                            engine.font(),
                            &equation.math_tree.root,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            caret_math::layout::MathStyle::Display,
                            context.settings.inline_environment_templates(),
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                    ),
                    HashMap::new(),
                ))
            } else {
                Ok((
                    engine
                        .layout(
                            &equation.math_tree.root,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            caret_math::layout::MathStyle::Display,
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                    HashMap::new(),
                ))
            }
        }
        MathBlockSource::Align(align) => {
            let contains_text = align
                .rows
                .iter()
                .flat_map(|row| &row.cells)
                .any(|cell| math_flow_contains_text(&cell.root));
            let host_row_grid = math_host_row_grid(
                context.line_grid_metrics,
                context.caret_metrics,
                context.line_spacing,
            )?;
            if contains_text {
                let math_base_style =
                    resolved_math_base_style(context.base_style, context.settings, None);
                let mut shaper = EngineMathTextShaper::new(
                    context.catalog,
                    context.settings,
                    &math_base_style,
                    context.text_layouter,
                    MathTextLayoutHost::align(align.id.clone(), align),
                );
                Ok((
                    Arc::new(
                        caret_math::layout::layout_align_with_host_row_grid(
                            engine.font(),
                            align,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            context.settings.inline_environment_templates(),
                            Some(&mut shaper),
                            host_row_grid,
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                    ),
                    shaper.into_sidecars(),
                ))
            } else {
                Ok((
                    Arc::new(
                        caret_math::layout::layout_align_with_host_row_grid(
                            engine.font(),
                            align,
                            DocumentUnits::centipoints_to_points(u32::from(
                                context.font_size_centipoints,
                            )),
                            context.settings.inline_environment_templates(),
                            None,
                            host_row_grid,
                        )
                        .map_err(|error| DocumentLayoutError::Math(error.to_string()))?,
                    ),
                    HashMap::new(),
                ))
            }
        }
    }
}

fn push_math_block<'a>(
    ordered: &mut Vec<LayoutInput<'a>>,
    source: MathBlockSource,
    layout: Arc<caret_math::layout::MathLayout>,
    text_layouts: HashMap<u64, MathTextSidecar>,
    environment_ids: Vec<NodeId>,
    context: &MathLayoutContext<'_>,
) -> Result<(), DocumentLayoutError> {
    let math_id = match &source {
        MathBlockSource::Equation(equation) => equation.id.clone(),
        MathBlockSource::Align(align) => align.id.clone(),
    };
    let width_pt = layout.width as f32;
    let host_metrics = math_block_host_metrics(&source, &layout, context)?;
    let height_pt = host_metrics.height_pt;
    if !context.content_width_pt.is_finite()
        || context.content_width_pt <= 0.0
        || !width_pt.is_finite()
        || width_pt < 0.0
        || !height_pt.is_finite()
        || height_pt < 0.0
    {
        return Err(DocumentLayoutError::Math(
            "invalid display math geometry".to_owned(),
        ));
    }
    ordered.push(LayoutInput::MathBlock {
        math_id,
        source: Box::new(source),
        layout,
        text_layouts,
        environment_ids,
        x_offset_pt: (context.content_width_pt - width_pt) / 2.0,
        width_pt,
        height_pt,
        baseline_pt: host_metrics.baseline_pt,
        caret_baseline_pt: context.caret_metrics.baseline_pt,
        caret_height_pt: context.caret_metrics.height_pt,
        clip_ink_to_line_height: host_metrics.clip_ink_to_line_height,
        space_before_pt: context.space_before_pt,
        space_after_pt: context.space_after_pt,
    });
    Ok(())
}

/// 显示公式宿主行框度量；Equation 与 Align 共用同一入口。
pub(super) fn math_block_host_metrics(
    source: &MathBlockSource,
    layout: &caret_math::layout::MathLayout,
    context: &MathLayoutContext<'_>,
) -> Result<LineMetrics, DocumentLayoutError> {
    match source {
        MathBlockSource::Equation(equation)
            if !math_flow_contains_soft_break(&equation.math_tree.root) =>
        {
            calculate_math_host_metrics(layout, context.line_grid_metrics, context.line_spacing)
        }
        MathBlockSource::Equation(_) | MathBlockSource::Align(_) => {
            calculate_math_align_host_metrics(layout)
        }
    }
}

pub(super) fn calculate_math_host_metrics(
    layout: &caret_math::layout::MathLayout,
    line_grid_metrics: FontLineMetrics,
    line_spacing: LineSpacing,
) -> Result<LineMetrics, DocumentLayoutError> {
    let width_pt = layout.width as f32;
    let ascent_pt = layout.ascent as f32;
    let descent_pt = layout.descent as f32;
    if !width_pt.is_finite() || !ascent_pt.is_finite() || !descent_pt.is_finite() || width_pt < 0.0
    {
        return Err(DocumentLayoutError::Math(
            "invalid display math line metrics".to_owned(),
        ));
    }
    calculate_content_line_metrics_with_grid(line_grid_metrics, ascent_pt, descent_pt, line_spacing)
        .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))
}

pub(super) fn calculate_math_align_host_metrics(
    layout: &caret_math::layout::MathLayout,
) -> Result<LineMetrics, DocumentLayoutError> {
    let height_pt = (layout.ascent + layout.descent) as f32;
    let baseline_pt = layout.ascent as f32;
    if !height_pt.is_finite() || height_pt <= 0.0 || !baseline_pt.is_finite() {
        return Err(DocumentLayoutError::Math(
            "invalid Align grid metrics".to_owned(),
        ));
    }
    Ok(LineMetrics {
        height_pt,
        baseline_pt,
        content_metrics_overflow_line_box: false,
        object_ink_overflows_line_box: false,
        // Exactly 行距的逐行裁切已附在 MathLayout 的行项范围上。
        clip_ink_to_line_height: false,
    })
}

pub(super) fn math_host_row_grid(
    line_grid: FontLineMetrics,
    body_metrics: LineMetrics,
    spacing: LineSpacing,
) -> Result<caret_math::layout::MathHostRowGrid, DocumentLayoutError> {
    Ok(caret_math::layout::MathHostRowGrid {
        ascent: f64::from(line_grid.ascent_pt),
        descent: f64::from(line_grid.descent_pt),
        leading: f64::from(line_grid.leading_pt),
        spacing: caret_layout::row_spacing_for_content_line_spacing(
            spacing,
            body_metrics.height_pt,
            body_metrics.baseline_pt,
        )
        .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?,
    })
}

fn math_flow_contains_soft_break(flow: &MathFlow) -> bool {
    flow.content.iter().any(|content| match content {
        MathContent::SoftBreak(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_soft_break),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_contains_soft_break),
        MathContent::Text(_) | MathContent::Opaque(_) => false,
    })
}

fn math_content_contains_soft_break(content: &MathContent) -> bool {
    match content {
        MathContent::SoftBreak(_) => true,
        MathContent::Math(node) => node.children.iter().any(math_flow_contains_soft_break),
        MathContent::Environment(environment) => environment
            .children
            .iter()
            .any(math_content_contains_soft_break),
        MathContent::Text(_) | MathContent::Opaque(_) => false,
    }
}

//! 表格布局测量、分页展开与单元格内容放置。
//!
//! 表格的纯几何装配在 `caret-layout`；本模块负责把文档单元格内容测量成
//! 行/列输入、在引擎快照中登记网格几何，并把分页片段展开为单元格段落的
//! 同一份页面行放置。新增单元格块类型时扩展此处的测量分支，而不是在
//! engine 其他位置复制表格逻辑。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::produce::{
    DocumentLayoutService, MathLayoutContext, ParagraphShapingRequest, ParagraphShapingSettings,
    math_block_host_metrics, measure_math_block,
};
use super::{
    Block, CaretLineLocation, Document, DocumentLayoutError, DocumentLayoutSnapshot, FontCatalog,
    LaidOutParagraph, MathBlockSource, NodeId, PageLayoutSnapshot, ResolvedParagraphStyle, StoryId,
    TableMathPlacement,
};
use caret_structure::{BlockContainerPath, ContainerStep, TableCellPath};

/// 一次表格布局中已测量的单元格段落索引与网格几何。
pub(super) struct ShapedTable {
    pub(super) table_id: NodeId,
    pub(super) grid: std::sync::Arc<caret_layout::TableGridLayout>,
    pub(super) rows: Vec<ShapedTableRow>,
    pub(super) space_before_pt: f32,
    pub(super) space_after_pt: f32,
}

pub(super) struct ShapedTableRow {
    pub(super) cells: Vec<ShapedTableCell>,
}

/// 单元格内块的测量顺序；与网格块的放置索引一一对应。
pub(super) enum ShapedCellBlock {
    Paragraph(usize),
    Math(usize),
}

pub(super) type ShapedParagraphs = (
    Vec<LaidOutParagraph>,
    Vec<ResolvedParagraphStyle>,
    Vec<ShapedTable>,
);

pub(super) struct ShapedTableCell {
    /// 与单元格块序列等长的内容引用；段落/公式按文档顺序排列。
    pub(super) blocks: Vec<ShapedCellBlock>,
    /// 指向本次布局 `paragraphs` 向量的索引。
    paragraphs: Vec<usize>,
    pub(super) math_blocks: Vec<ShapedCellMathBlock>,
}

pub(super) struct ShapedCellMathBlock {
    pub(super) math_id: NodeId,
    pub(super) source: super::MathBlockSource,
    pub(super) layout: std::sync::Arc<caret_math::layout::MathLayout>,
    pub(super) text_layouts: std::collections::HashMap<u64, super::MathTextSidecar>,
    pub(super) baseline_pt: f64,
    pub(super) caret_height_pt: f64,
    pub(super) x_offset_pt: f32,
}

pub(super) struct TableShapingRequest<'a> {
    pub(super) document: &'a Document,
    pub(super) table: &'a caret_doc::Table,
    pub(super) container: &'a BlockContainerPath,
    pub(super) space_before_pt: f32,
    pub(super) space_after_pt: f32,
    pub(super) reuse: Option<(&'a DocumentLayoutSnapshot, Option<&'a NodeId>)>,
    pub(super) settings: &'a ParagraphShapingSettings<'a>,
}

fn math_id_for_source(source: &MathBlockSource) -> NodeId {
    match source {
        MathBlockSource::Equation(equation) => equation.id.clone(),
        MathBlockSource::Align(align) => align.id.clone(),
    }
}

impl DocumentLayoutService {
    pub(super) fn shape_table(
        &mut self,
        catalog: &mut FontCatalog,
        request: TableShapingRequest<'_>,
    ) -> Result<ShapedParagraphs, DocumentLayoutError> {
        let TableShapingRequest {
            document,
            table,
            container,
            space_before_pt,
            space_after_pt,
            reuse,
            settings,
        } = request;
        let column_count = table.rows.first().map_or(0, |row| row.cells.len());
        if column_count == 0 {
            return Err(DocumentLayoutError::UnsupportedContent);
        }
        let line_grid_metrics = self.resolve_line_grid_metrics(
            catalog,
            settings.text_settings,
            settings.base_style,
            settings.text_settings.default_language(),
        )?;
        let options = caret_layout::TableLayoutOptions::text_defaults(
            f64::from(settings.content_width_pt),
            column_count,
            f64::from(line_grid_metrics.ascent_pt),
            f64::from(line_grid_metrics.descent_pt),
        );
        let column_width_pt = caret_layout::table_column_width_pt(&options)
            .map_err(|error| DocumentLayoutError::Pagination(error.to_string()))?;
        let default_style = caret_layout::resolve_paragraph_style(&document.global_settings, None)
            .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let default_caret_metrics = caret_layout::calculate_line_metrics_with_grid(
            line_grid_metrics,
            &[],
            &[],
            default_style.line_spacing,
        )
        .map_err(|error| DocumentLayoutError::Paragraph(error.to_string()))?;
        let table_id = table.id.clone();
        let mut paragraphs = Vec::new();
        let mut styles = Vec::new();
        let mut measured_rows = Vec::with_capacity(table.rows.len());
        let mut shaped_rows = Vec::with_capacity(table.rows.len());
        for row in &table.rows {
            let mut measured_cells = Vec::with_capacity(row.cells.len());
            let mut shaped_cells = Vec::with_capacity(row.cells.len());
            for (cell_index, cell) in row.cells.iter().enumerate() {
                let story = StoryId::TableCell {
                    table_id: table_id.clone(),
                    row_id: row.id.clone(),
                    cell_id: cell.id.clone(),
                };
                let mut cell_container = container.clone();
                cell_container.push(ContainerStep::TableCell(TableCellPath::new(
                    table_id.clone(),
                    row.id.clone(),
                    cell.id.clone(),
                    shaped_rows.len(),
                    cell_index,
                )));
                let mut cell_paragraphs = Vec::new();
                let mut measured_blocks = Vec::new();
                let mut math_blocks = Vec::new();
                let mut cell_blocks = Vec::new();
                for block in &cell.blocks {
                    let math_source = match block {
                        Block::Equation(equation) => {
                            Some(MathBlockSource::Equation(equation.clone()))
                        }
                        Block::Align(align) => Some(MathBlockSource::Align(align.clone())),
                        _ => None,
                    };
                    if let Some(source) = math_source {
                        let (layout, text_layouts, host_metrics) = {
                            let mut math_context = MathLayoutContext {
                                engine: self.math_layout_engine.as_ref(),
                                catalog,
                                settings: settings.text_settings,
                                base_style: settings.base_style,
                                text_layouter: &mut self.layouter,
                                font_size_centipoints: settings.base_style.font_size_centipoints,
                                content_width_pt: column_width_pt as f32,
                                line_grid_metrics,
                                line_spacing: default_style.line_spacing,
                                space_before_pt: default_style.space_before_pt,
                                space_after_pt: default_style.space_after_pt,
                                caret_metrics: default_caret_metrics,
                            };
                            let (layout, text_layouts) = measure_math_block(
                                &source,
                                &mut math_context,
                                reuse.map(|(previous, _)| previous),
                            )?;
                            let host_metrics =
                                math_block_host_metrics(&source, &layout, &math_context)?;
                            (layout, text_layouts, host_metrics)
                        };
                        math_blocks.push(ShapedCellMathBlock {
                            math_id: math_id_for_source(&source),
                            source,
                            x_offset_pt: (column_width_pt as f32 - layout.width as f32) / 2.0,
                            layout,
                            text_layouts,
                            baseline_pt: f64::from(host_metrics.baseline_pt),
                            caret_height_pt: f64::from(default_caret_metrics.height_pt),
                        });
                        cell_blocks.push(ShapedCellBlock::Math(math_blocks.len() - 1));
                        measured_blocks.push(caret_layout::MeasuredCellBlock {
                            height_pt: f64::from(host_metrics.height_pt),
                            first_baseline_pt: f64::from(host_metrics.baseline_pt),
                            space_before_pt: f64::from(default_style.space_before_pt),
                            space_after_pt: f64::from(default_style.space_after_pt),
                        });
                        continue;
                    }
                    match block {
                        Block::Paragraph(paragraph) => {
                            let (laid_out, style) = self.shape_paragraph(
                                catalog,
                                ParagraphShapingRequest {
                                    document,
                                    paragraph,
                                    story: story.clone(),
                                    container: &cell_container,
                                    width_pt: column_width_pt as f32,
                                    reuse,
                                    settings,
                                },
                            )?;
                            let index = paragraphs.len();
                            let paragraph_height_pt = laid_out
                                .layout
                                .paragraph
                                .lines
                                .iter()
                                .map(|line| f64::from(line.metrics.height_pt))
                                .sum::<f64>();
                            let first_baseline_pt = laid_out
                                .layout
                                .paragraph
                                .lines
                                .first()
                                .map_or(0.0, |line| f64::from(line.metrics.baseline_pt));
                            measured_blocks.push(caret_layout::MeasuredCellBlock {
                                height_pt: paragraph_height_pt,
                                first_baseline_pt,
                                space_before_pt: f64::from(style.space_before_pt),
                                space_after_pt: f64::from(style.space_after_pt),
                            });
                            paragraphs.push(laid_out);
                            styles.push(style);
                            cell_paragraphs.push(index);
                            cell_blocks.push(ShapedCellBlock::Paragraph(cell_paragraphs.len() - 1));
                        }
                        Block::Environment(_)
                        | Block::Table(_)
                        | Block::ExplicitBreak(_)
                        | Block::Opaque(_) => {
                            // 环境/嵌套表/分页符随后续切片接入；当前显式拒绝。
                            return Err(DocumentLayoutError::UnsupportedContent);
                        }
                        Block::Equation(_) | Block::Align(_) => {}
                    }
                }
                if cell_paragraphs.is_empty() && math_blocks.is_empty() {
                    return Err(DocumentLayoutError::UnsupportedContent);
                }
                measured_cells.push(caret_layout::TableCellMeasurement {
                    blocks: measured_blocks,
                });
                shaped_cells.push(ShapedTableCell {
                    blocks: cell_blocks,
                    paragraphs: cell_paragraphs,
                    math_blocks,
                });
            }
            measured_rows.push(caret_layout::TableRowMeasurement {
                cells: measured_cells,
            });
            shaped_rows.push(ShapedTableRow {
                cells: shaped_cells,
            });
        }
        let grid = std::sync::Arc::new(
            caret_layout::layout_table_grid(&measured_rows, options)
                .map_err(|error| DocumentLayoutError::Pagination(error.to_string()))?,
        );
        Ok((
            paragraphs,
            styles,
            vec![ShapedTable {
                table_id,
                grid,
                rows: shaped_rows,
                space_before_pt,
                space_after_pt,
            }],
        ))
    }
}

/// 把表格片段展开为单元格段落的页面行位置与绘制片段，并写入同一几何来源。
pub(super) fn expand_table_fragments(
    pages: &mut [PageLayoutSnapshot],
    shaped_tables: &[ShapedTable],
    paragraphs: &mut [LaidOutParagraph],
) -> Result<(), DocumentLayoutError> {
    for (page_index, page) in pages.iter_mut().enumerate() {
        let fragments = page.layout.table_fragments.clone();
        for fragment in fragments {
            let Some(shaped) = shaped_tables
                .iter()
                .find(|table| table.table_id == fragment.table_id)
            else {
                return Err(DocumentLayoutError::InvalidDocument("missing table".into()));
            };
            let origin_x_pt = page.margin_left_pt + f64::from(fragment.x_offset_pt);
            let origin_y_pt = page.margin_top_pt + f64::from(fragment.y_offset_pt);
            for row_index in fragment.row_range.clone() {
                let Some(shaped_row) = shaped.rows.get(row_index) else {
                    continue;
                };
                let Some(grid_row) = shaped.grid.rows.get(row_index) else {
                    continue;
                };
                for (cell_index, shaped_cell) in shaped_row.cells.iter().enumerate() {
                    let Some(grid_cell) = grid_row.cells.get(cell_index) else {
                        continue;
                    };
                    let content_x_pt = origin_x_pt + grid_cell.content_x_pt;
                    let content_y_pt = origin_y_pt + grid_cell.content_y_pt;
                    let clip = [
                        content_x_pt,
                        origin_y_pt + grid_row.top_pt,
                        content_x_pt + grid_cell.content_width_pt,
                        origin_y_pt + grid_row.top_pt + grid_row.height_pt,
                    ];
                    let host = caret_layout::FragmentHost::TableCell {
                        table_id: fragment.table_id.clone(),
                        row_index,
                        cell_index,
                    };
                    for (block_offset, block_ref) in shaped_cell.blocks.iter().enumerate() {
                        let Some(grid_block) = grid_cell.blocks.get(block_offset) else {
                            continue;
                        };
                        match block_ref {
                            ShapedCellBlock::Paragraph(paragraph_offset) => {
                                let Some(paragraph_index) =
                                    shaped_cell.paragraphs.get(*paragraph_offset)
                                else {
                                    continue;
                                };
                                let Some(paragraph) = paragraphs.get_mut(*paragraph_index) else {
                                    continue;
                                };
                                let paragraph_y_pt = content_y_pt + grid_block.top_pt;
                                let mut y0_pt = paragraph_y_pt;
                                for (line_index, line) in
                                    paragraph.layout.paragraph.lines.iter().enumerate()
                                {
                                    if let Some(location) =
                                        paragraph.line_locations.get_mut(line_index)
                                    {
                                        *location = Some(CaretLineLocation {
                                            page_index,
                                            x0_pt: content_x_pt,
                                            y0_pt,
                                            clip: Some(super::PageClip::from_array(clip)),
                                        });
                                    }
                                    y0_pt += f64::from(line.metrics.height_pt);
                                }
                                page.layout.fragments.push(caret_layout::ParagraphFragment {
                                    paragraph_id: paragraph.id.clone(),
                                    line_range: 0..paragraph.layout.paragraph.lines.len(),
                                    x_offset_pt: (content_x_pt - page.margin_left_pt) as f32,
                                    y_offset_pt: (paragraph_y_pt - page.margin_top_pt) as f32,
                                    host: host.clone(),
                                    clip: Some(caret_layout::FragmentClip {
                                        x0_pt: (clip[0] - page.margin_left_pt) as f32,
                                        y0_pt: (clip[1] - page.margin_top_pt) as f32,
                                        x1_pt: (clip[2] - page.margin_left_pt) as f32,
                                        y1_pt: (clip[3] - page.margin_top_pt) as f32,
                                    }),
                                });
                            }
                            ShapedCellBlock::Math(math_offset) => {
                                let Some(math) = shaped_cell.math_blocks.get(*math_offset) else {
                                    continue;
                                };
                                let block_top_pt = content_y_pt + grid_block.top_pt;
                                page.table_math_placements.push(TableMathPlacement {
                                    math_id: math.math_id.clone(),
                                    object_x_pt: content_x_pt + f64::from(math.x_offset_pt),
                                    baseline_y_pt: block_top_pt + math.baseline_pt,
                                    caret_line_top_pt: block_top_pt,
                                    caret_baseline_y_pt: block_top_pt + math.baseline_pt,
                                    caret_height_pt: math.caret_height_pt.max(1.0),
                                    clip_y_pt: Some([clip[1], clip[3]]),
                                });
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

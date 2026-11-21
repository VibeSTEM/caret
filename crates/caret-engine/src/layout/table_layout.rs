//! 表格布局测量、分页展开与单元格内容放置。
//!
//! 表格的纯几何装配在 `caret-layout`；本模块负责把文档单元格内容测量成
//! 行/列输入、在引擎快照中登记网格几何，并把分页片段展开为单元格段落的
//! 同一份页面行放置。新增单元格块类型时扩展此处的测量分支，而不是在
//! engine 其他位置复制表格逻辑。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::produce::{DocumentLayoutService, ParagraphShapingRequest, ParagraphShapingSettings};
use super::{
    Block, CaretLineLocation, Document, DocumentLayoutError, DocumentLayoutSnapshot, FontCatalog,
    LaidOutParagraph, NodeId, PageLayoutSnapshot, ResolvedParagraphStyle, StoryId,
};
use caret_structure::{BlockContainerPath, ContainerStep, TableCellPath};

/// 一次表格布局中已测量的单元格段落索引与网格几何。
pub(super) struct ShapedTable {
    pub(super) table_id: NodeId,
    pub(super) grid: std::sync::Arc<caret_layout::TableGridLayout>,
    rows: Vec<ShapedTableRow>,
    pub(super) space_before_pt: f32,
    pub(super) space_after_pt: f32,
}

pub(super) struct ShapedTableRow {
    cells: Vec<ShapedTableCell>,
}

pub(super) type ShapedParagraphs = (
    Vec<LaidOutParagraph>,
    Vec<ResolvedParagraphStyle>,
    Vec<ShapedTable>,
);

pub(super) struct ShapedTableCell {
    /// 指向本次布局 `paragraphs` 向量的索引。
    paragraphs: Vec<usize>,
    /// 每个段落内容顶边相对单元格内容区顶边的偏移，单位为 pt。
    paragraph_offsets: Vec<f64>,
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

/// 段落（含行内环境嵌套）是否含行内公式；表格布局 v1 暂不支持 cell 内行内公式。
fn paragraph_has_inline_math(paragraph: &caret_doc::Paragraph) -> bool {
    fn inlines_contain(nodes: &[caret_doc::ParagraphContent]) -> bool {
        nodes.iter().any(|node| match node {
            caret_doc::ParagraphContent::Math(_) => true,
            caret_doc::ParagraphContent::Environment(environment) => {
                inlines_contain(&environment.children)
            }
            _ => false,
        })
    }
    inlines_contain(&paragraph.inlines)
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
                let mut paragraph_offsets = Vec::new();
                let mut content_height_pt = 0.0_f64;
                let mut previous_space_after_pt = 0.0_f64;
                for block in &cell.blocks {
                    let Block::Paragraph(paragraph) = block else {
                        // 单元格内公式/环境/嵌套表的布局随 046 后续切片接入；
                        // 当前明确拒绝而不是静默漏画。
                        return Err(DocumentLayoutError::UnsupportedContent);
                    };
                    if paragraph_has_inline_math(paragraph) {
                        // 行内公式与显示公式定位、裁剪与命中需先统一容器路径；
                        // 与编辑层门禁保持一致，布局阶段显式拒绝。
                        return Err(DocumentLayoutError::UnsupportedContent);
                    }
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
                    let space_before_pt = f64::from(style.space_before_pt);
                    if !cell_paragraphs.is_empty() {
                        content_height_pt += previous_space_after_pt.max(space_before_pt);
                    }
                    paragraph_offsets.push(content_height_pt);
                    content_height_pt += paragraph_height_pt;
                    previous_space_after_pt = f64::from(style.space_after_pt);
                    paragraphs.push(laid_out);
                    styles.push(style);
                    cell_paragraphs.push(index);
                }
                if cell_paragraphs.is_empty() {
                    return Err(DocumentLayoutError::UnsupportedContent);
                }
                let first_baseline_pt = paragraph_offsets.first().copied().unwrap_or(0.0)
                    + f64::from(
                        paragraphs[*cell_paragraphs.first().unwrap_or(&0)]
                            .layout
                            .paragraph
                            .lines
                            .first()
                            .map_or(0.0, |line| line.metrics.baseline_pt),
                    );
                measured_cells.push(caret_layout::TableCellMeasurement {
                    content_height_pt,
                    first_baseline_pt,
                });
                shaped_cells.push(ShapedTableCell {
                    paragraphs: cell_paragraphs,
                    paragraph_offsets,
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
                    for (paragraph_offset, paragraph_index) in
                        shaped_cell.paragraphs.iter().enumerate()
                    {
                        let Some(paragraph) = paragraphs.get_mut(*paragraph_index) else {
                            continue;
                        };
                        let paragraph_y_pt = content_y_pt
                            + shaped_cell
                                .paragraph_offsets
                                .get(paragraph_offset)
                                .copied()
                                .unwrap_or(0.0);
                        let mut y0_pt = paragraph_y_pt;
                        for (line_index, line) in
                            paragraph.layout.paragraph.lines.iter().enumerate()
                        {
                            if let Some(location) = paragraph.line_locations.get_mut(line_index) {
                                *location = Some(CaretLineLocation {
                                    page_index,
                                    x0_pt: content_x_pt,
                                    y0_pt,
                                    clip: Some(clip),
                                });
                            }
                            y0_pt += f64::from(line.metrics.height_pt);
                        }
                        page.layout.fragments.push(caret_layout::ParagraphFragment {
                            paragraph_id: paragraph.id.clone(),
                            line_range: 0..paragraph.layout.paragraph.lines.len(),
                            x_offset_pt: (content_x_pt - page.margin_left_pt) as f32,
                            y_offset_pt: (paragraph_y_pt - page.margin_top_pt) as f32,
                            clip: Some(caret_layout::FragmentClip {
                                x0_pt: (clip[0] - page.margin_left_pt) as f32,
                                y0_pt: (clip[1] - page.margin_top_pt) as f32,
                                x1_pt: (clip[2] - page.margin_left_pt) as f32,
                                y1_pt: (clip[3] - page.margin_top_pt) as f32,
                            }),
                        });
                    }
                }
            }
        }
    }
    Ok(())
}

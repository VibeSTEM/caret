//! 按页面命中、几何与公式文本导航查询布局快照。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::*;

impl DocumentLayoutSnapshot {
    /// 返回该页公式投影所属的文档单元格。目标不匹配或公式在本页没有格内投影时返回空。
    pub fn math_target_cell_id(
        &self,
        page_index: usize,
        target: &MathEditTarget,
    ) -> Option<&NodeId> {
        let math_id = match target {
            MathEditTarget::Inline { math_id, .. } => math_id,
            MathEditTarget::Block { block_id, .. } => block_id,
        };
        if &self.math_host(math_id)?.target != target {
            return None;
        }
        self.math_projection(page_index, math_id)?.cell_id.as_ref()
    }

    pub(crate) fn math_projection(
        &self,
        page_index: usize,
        math_id: &NodeId,
    ) -> Option<&MathPageProjection> {
        self.math_host(math_id)?;
        let index = self
            .math_projection_index
            .by_page_and_id
            .get(&(page_index, math_id.clone()))?;
        self.math_projection_index.projections.get(*index)
    }

    pub(crate) fn math_host(&self, math_id: &NodeId) -> Option<&MathTextLayoutHost> {
        self.math_hosts.get(math_id)
    }

    pub(crate) fn math_text_layouts_for_block(
        &self,
        block_id: &NodeId,
    ) -> Option<&HashMap<u64, MathTextSidecar>> {
        self.math_block_text_layouts.get(block_id)
    }

    #[cfg(test)]
    pub(crate) fn math_page_projections(&self) -> &[MathPageProjection] {
        &self.math_projection_index.projections
    }

    pub(super) fn math_text_row_id<'a>(
        &'a self,
        target: &MathEditTarget,
        position: &MathTextPosition,
    ) -> Option<&'a NodeId> {
        let MathEditTarget::Block {
            block_id,
            align: true,
        } = target
        else {
            return None;
        };
        let (row_index, _) = position.path.align_cell()?;
        let MathBlockSource::Align(align) = self.math_block_sources.get(block_id)? else {
            return None;
        };
        Some(&align.rows.get(row_index)?.id)
    }
    pub(super) fn math_projection_items(
        &self,
        projection: &MathPageProjection,
    ) -> Option<(
        &caret_math::layout::MathLayout,
        &HashMap<u64, MathTextSidecar>,
    )> {
        match &self.math_host(&projection.math_id)?.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                let paragraph = self.paragraph_by_id(paragraph_id)?;
                let index = projection.inline_index?;
                if paragraph.inline_math_ids.get(index)? != math_id {
                    return None;
                }
                Some((
                    paragraph.inline_math_layouts.get(index)?.as_ref(),
                    paragraph.inline_math_text_layouts.get(index)?,
                ))
            }
            MathEditTarget::Block { block_id, .. } => Some((
                self.math_blocks.get(block_id)?.as_ref(),
                self.math_block_text_layouts.get(block_id)?,
            )),
        }
    }

    pub(super) fn math_scope_root(
        &self,
        projection: &MathPageProjection,
        scope_id: &NodeId,
    ) -> Option<&MathFlow> {
        let host = self.math_host(&projection.math_id)?;
        host.for_scope(scope_id)?;
        let root = match &host.target {
            MathEditTarget::Inline {
                paragraph_id,
                math_id,
            } => {
                &find_inline_math_by_id(
                    &self.paragraph_by_id(paragraph_id)?.source_paragraph.inlines,
                    math_id,
                )?
                .math_tree
                .root
            }
            MathEditTarget::Block {
                block_id,
                align: false,
            } => match self.math_block_sources.get(block_id)? {
                MathBlockSource::Equation(equation) => &equation.math_tree.root,
                MathBlockSource::Align(_) => return None,
            },
            MathEditTarget::Block {
                block_id,
                align: true,
            } => match self.math_block_sources.get(block_id)? {
                MathBlockSource::Align(align) => align
                    .rows
                    .iter()
                    .flat_map(|row| &row.cells)
                    .find(|cell| &cell.root.id == scope_id)
                    .map(|cell| &cell.root)?,
                MathBlockSource::Equation(_) => return None,
            },
        };
        (root.id == *scope_id).then_some(root)
    }

    /// 返回光标位置当前塑形 Run 的字号；Run 边界按视觉亲和性选择，空段回退到文档默认字号。
    pub fn font_size_centipoints_at(
        &self,
        position: &Pos,
        visual_affinity: VisualAffinity,
    ) -> Option<u16> {
        let paragraph = self.paragraph_by_id(&position.paragraph_id)?;
        if paragraph.story != position.story {
            return None;
        }
        let cursor = paragraph
            .layout
            .cursor_at(position.offset, visual_affinity)?;
        let (_, line_index) = paragraph.layout.cursor_geometry_with_line(cursor, 0.0)?;
        let line = paragraph.layout.layout.get(line_index)?;
        let mut upstream_size = None;
        let mut downstream_size = None;
        for item in line.items() {
            let PositionedGlyphLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let range = run.text_range();
            let font_size = run.font_size();
            if range.start < position.offset && position.offset < range.end {
                return font_size_centipoints(font_size);
            }
            if range.end == position.offset {
                upstream_size = font_size_centipoints(font_size);
            }
            if range.start == position.offset {
                downstream_size = font_size_centipoints(font_size);
            }
        }
        let size = match visual_affinity {
            VisualAffinity::Upstream => upstream_size.or(downstream_size),
            VisualAffinity::Downstream => downstream_size.or(upstream_size),
        };
        Some(size.unwrap_or(self.default_font_size_centipoints))
    }

    pub fn paragraph_by_id(&self, id: &NodeId) -> Option<&LaidOutParagraph> {
        self.paragraphs.get(*self.paragraph_lookup.get(id)?)
    }

    pub fn paragraph_index_by_id(&self, id: &NodeId) -> Option<usize> {
        self.paragraph_lookup.get(id).copied()
    }

    /// 判断此布局结果是否仍对应当前文档与字体输入。
    ///
    /// 异步调用方应在安装布局前调用，避免已替换的会话、已编辑的文档或已刷新的字体目录发布过期几何。
    pub fn is_current_for(&self, session: &Session, font_generation: u64) -> bool {
        self.session_id.as_ref() == Some(session.session_id())
            && self.document_generation == session.document_generation()
            && self.font_generation == font_generation
    }
}

impl LaidOutParagraph {
    /// 返回行内公式布局及其同代文本数据，供绘制使用。
    pub(crate) fn inline_math_glyph_data(
        &self,
        index: usize,
    ) -> Option<(
        &caret_math::layout::MathLayout,
        &HashMap<u64, MathTextSidecar>,
        &NodeId,
    )> {
        Some((
            self.inline_math_layouts.get(index)?.as_ref(),
            self.inline_math_text_layouts.get(index)?,
            self.inline_math_ids.get(index)?,
        ))
    }
}

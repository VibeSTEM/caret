//! 创建块环境、解除环境外壳并提交对应文档事务。

use super::*;
use crate::editing::paragraph_ops;

impl Session {
    /// 将当前段落按容器语义原子转换为 Section 或可命名的 Custom 环境。
    pub(in crate::editing::session) fn create_block_environment(
        &mut self,
        kind: BlockEnvironmentKind,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() || self.command_line.is_some() {
            return Ok(Effect::default());
        }
        if self.selection.anchor != self.selection.focus {
            return Err(EditError::InvalidStructureSelection);
        }
        let position = self.selection.focus.clone();
        if position.story != StoryId::Body {
            return Err(EditError::WrongStory);
        }
        let location = self
            .paragraph_index
            .get(&position.paragraph_id)
            .cloned()
            .ok_or_else(|| EditError::UnknownParagraph(position.paragraph_id.to_string()))?;
        let paragraph = paragraph_at_location(&self.document.body, &location)
            .ok_or_else(|| EditError::UnknownParagraph(position.paragraph_id.to_string()))?
            .clone();
        let (parent_environment_ids, sibling_index) =
            history::parent_environment_path(&self.document.body, &location)
                .ok_or(EditError::InvalidPosition)?;
        if kind == BlockEnvironmentKind::Section && !TextMap::new(&paragraph)?.text.is_empty() {
            return self.insert_section_after_content(
                parent_environment_ids,
                sibling_index.saturating_add(1),
            );
        }
        let environment_id = NodeId::generate();
        let environment = BlockEnvironment {
            id: environment_id.clone(),
            kind,
            definition_id: None,
            props: BTreeMap::new(),
            title: (kind == BlockEnvironmentKind::Section).then(|| paragraph.clone()),
            children: if kind == BlockEnvironmentKind::Custom {
                vec![Block::Paragraph(paragraph.clone())]
            } else {
                Vec::new()
            },
            extra: BTreeMap::new(),
        };
        let replacement = Block::Environment(environment.clone());
        if !replace_block_at_location(&mut self.document.body, &location, replacement.clone()) {
            return Err(EditError::UnknownParagraph(
                position.paragraph_id.to_string(),
            ));
        }
        if let Err(error) = self.document.validate() {
            replace_block_at_location(
                &mut self.document.body,
                &location,
                Block::Paragraph(paragraph.clone()),
            );
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let position_map =
            (kind == BlockEnvironmentKind::Section).then(|| PositionMap::StoryChange {
                paragraph_id: position.paragraph_id.clone(),
                from: StoryId::Body,
                to: StoryId::EnvironmentTitle(environment_id.clone()),
            });
        let next_position = if kind == BlockEnvironmentKind::Section {
            Pos {
                story: StoryId::EnvironmentTitle(environment_id.clone()),
                ..position
            }
        } else {
            position
        };
        self.selection = Selection {
            anchor: next_position.clone(),
            focus: next_position,
        };
        let selection_changed = before_selection != self.selection;
        self.selection_visual_affinity = match self.selection.focus.affinity {
            Affinity::Upstream => VisualAffinity::Upstream,
            Affinity::Downstream => VisualAffinity::Downstream,
        };
        let paragraph_estimated_bytes = history::paragraph_estimated_bytes(&paragraph);
        let patch = DocumentPatch::EnvironmentCreation {
            parent_environment_ids,
            sibling_index,
            paragraph: Box::new(paragraph),
            paragraph_estimated_bytes,
            environment_id,
            kind,
        };
        self.finalize_structure_edit(
            paragraph_ops::build_paragraph_index(&self.document.body),
            patch,
            before_selection,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed,
            position_map,
            ..Effect::default()
        })
    }

    /// 在已有文本段落后插入空标题 Section，避免命令把原段落改成标题。
    fn insert_section_after_content(
        &mut self,
        parent_environment_ids: Vec<NodeId>,
        sibling_index: usize,
    ) -> Result<Effect, EditError> {
        let environment = BlockEnvironment {
            id: NodeId::generate(),
            kind: BlockEnvironmentKind::Section,
            definition_id: None,
            props: BTreeMap::new(),
            title: Some(Paragraph::empty(NodeId::generate())),
            children: Vec::new(),
            extra: BTreeMap::new(),
        };
        let Some(siblings) =
            block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        if sibling_index > siblings.len() {
            return Err(EditError::InvalidPosition);
        }
        siblings.insert(sibling_index, Block::Environment(environment.clone()));
        if let Err(error) = self.document.validate() {
            if let Some(siblings) =
                block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
            {
                siblings.remove(sibling_index);
            }
            return Err(EditError::InvalidDocument(error.to_string()));
        }

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let title = environment
            .title
            .as_ref()
            .ok_or(EditError::InvalidPosition)?;
        let position = Pos {
            story: StoryId::EnvironmentTitle(environment.id.clone()),
            paragraph_id: title.id.clone(),
            offset: 0,
            affinity: Affinity::Downstream,
        };
        self.selection = Selection {
            anchor: position.clone(),
            focus: position,
        };
        self.selection_visual_affinity = VisualAffinity::Downstream;
        let environment_estimated_bytes = history::environment_estimated_bytes(&environment);
        self.finalize_structure_edit(
            paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::EnvironmentInsertion {
                parent_environment_ids,
                sibling_index,
                environment: Box::new(environment),
                environment_estimated_bytes,
            },
            before_selection,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Effect {
            document_changed: true,
            selection_changed: true,
            ..Effect::default()
        })
    }

    /// 移除当前块环境外壳并按标题、正文顺序提升内容。
    pub(in crate::editing::session) fn unwrap_focused_block_environment(
        &mut self,
        only_empty: bool,
    ) -> Result<Option<Effect>, EditError> {
        if self.selection.anchor != self.selection.focus {
            return Ok(None);
        }
        let position = StructurePosition {
            paragraph_id: self.selection.focus.paragraph_id.clone(),
            offset_utf8: self.selection.focus.offset,
        };
        let environment_id = self
            .structure_index
            .at(&position)
            .iter()
            .rev()
            .find(|entry| entry.kind.is_block_environment())
            .map(|entry| entry.id.clone());
        let Some(environment_id) = environment_id else {
            return Ok(None);
        };
        let Some((parent_environment_ids, sibling_index)) =
            environment_parent_location(&self.document.body, &environment_id)
        else {
            return Err(EditError::InvalidPosition);
        };
        let siblings = block_list_by_environment_ids(&self.document.body, &parent_environment_ids)
            .ok_or(EditError::InvalidPosition)?;
        let Some(Block::Environment(environment)) = siblings.get(sibling_index) else {
            return Err(EditError::InvalidPosition);
        };
        if only_empty && environment_has_content(environment)? {
            return Ok(None);
        }
        let environment = environment.clone();
        let environment_estimated_bytes = history::environment_estimated_bytes(&environment);
        let title_id = environment.title.as_ref().map(|title| title.id.clone());
        let Some(siblings) =
            block_list_by_environment_ids_mut(&mut self.document.body, &parent_environment_ids)
        else {
            return Err(EditError::InvalidPosition);
        };
        let mut promoted = Vec::with_capacity(environment.children.len() + 1);
        if let Some(title) = &environment.title {
            promoted.push(Block::Paragraph(title.clone()));
        }
        promoted.extend(environment.children.iter().cloned());
        siblings.splice(sibling_index..=sibling_index, promoted);

        let before_selection = self.selection.clone();
        let before_visual_affinity = self.selection_visual_affinity;
        let before_state_id = self.history_state_id;
        let position_map = title_id.map(|paragraph_id| PositionMap::StoryChange {
            paragraph_id,
            from: StoryId::EnvironmentTitle(environment.id.clone()),
            to: StoryId::Body,
        });
        if let Some(position_map) = &position_map {
            self.selection.anchor = position_map.map(&self.selection.anchor);
            self.selection.focus = position_map.map(&self.selection.focus);
        }
        let selection_changed = before_selection != self.selection;
        self.finalize_structure_edit(
            paragraph_ops::build_paragraph_index(&self.document.body),
            DocumentPatch::EnvironmentUnwrap {
                parent_environment_ids,
                sibling_index,
                environment: Box::new(environment),
                environment_estimated_bytes,
            },
            before_selection,
            before_visual_affinity,
            before_state_id,
        );
        Ok(Some(Effect {
            document_changed: true,
            selection_changed,
            position_map,
            ..Effect::default()
        }))
    }
}

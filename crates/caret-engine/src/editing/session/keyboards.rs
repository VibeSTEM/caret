//! 将带上下文优先级的编辑器按键意图分派到独立行为模块。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod backslash;
mod backspace;
mod delete;
mod dollar;
mod down_arrow;
mod end;
mod enter;
mod escape;
mod home;
mod left_arrow;
mod mod_backspace;
mod mod_c;
mod mod_space;
mod mod_v;
mod mod_z;
mod right_arrow;
mod tab;
mod up_arrow;

use super::Session;
use crate::editing::{
    Command, Direction, EditError, EditorKey, Effect, MathEditBranch, NavigationCandidate,
    NavigationGeneration, NavigationIntent, Selection,
};

impl Session {
    /// 在具体按键处理器内核对焦点所有者并提交候选，不暴露可被调用方绕过的路由查询。
    pub(super) fn route_navigation_key(
        &mut self,
        intent: NavigationIntent,
        direction: Option<Direction>,
        candidate: Option<NavigationCandidate>,
        extend: bool,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Ok(Effect::default());
        }
        self.ensure_math_edit_revision_current()?;
        if self.command_line.is_some() {
            let command = match intent {
                NavigationIntent::Horizontal => {
                    let direction = direction.ok_or(EditError::InvalidPosition)?;
                    let _ = extend;
                    Command::MoveCommandLineCursor(direction)
                }
                NavigationIntent::Vertical => {
                    Command::MoveCommandLineSelection(direction.ok_or(EditError::InvalidPosition)?)
                }
                NavigationIntent::VisualLineEdge => {
                    let _ = (direction, extend);
                    return Ok(Effect::default());
                }
            };
            return self.run(command);
        }
        if let Some(edit) = &self.math_edit {
            return match &edit.branch {
                MathEditBranch::Text { .. } => match candidate {
                    Some(NavigationCandidate::MathText {
                        focus,
                        visual_affinity,
                        generation,
                    }) => {
                        self.validate_navigation_generation(&generation)?;
                        let context = self
                            .math_text_input_context()
                            .ok_or(EditError::InvalidPosition)?;
                        let (anchor, anchor_visual_affinity) = if extend {
                            (context.anchor, context.anchor_visual_affinity)
                        } else {
                            (focus.clone(), visual_affinity)
                        };
                        self.set_math_text_selection_positions_with_visual_affinities(
                            anchor,
                            focus,
                            anchor_visual_affinity,
                            visual_affinity,
                        )
                    }
                    Some(NavigationCandidate::MathTextHit { hit, generation }) => {
                        self.validate_navigation_generation(&generation)?;
                        if extend || intent != NavigationIntent::Vertical {
                            return Err(EditError::InvalidPosition);
                        }
                        self.focus_math_text_hit(&hit, generation.font_generation)
                    }
                    None if intent == NavigationIntent::Horizontal && !extend => {
                        let context = self
                            .math_text_input_context()
                            .ok_or(EditError::InvalidPosition)?;
                        let text_len = context.surrounding_text.encode_utf16().count();
                        let direction = direction.ok_or(EditError::InvalidPosition)?;
                        let is_exit_edge = match direction {
                            Direction::Backward => context.focus_utf16 == 0,
                            Direction::Forward => context.focus_utf16 == text_len,
                        };
                        if is_exit_edge {
                            self.run(Command::MoveCaret(direction))
                        } else {
                            Ok(Effect::default())
                        }
                    }
                    None => Ok(Effect::default()),
                    Some(NavigationCandidate::Document { .. }) => Err(EditError::InvalidPosition),
                },
                MathEditBranch::Tree { .. } | MathEditBranch::Source => {
                    match candidate {
                        Some(NavigationCandidate::Document { generation, .. }) => {
                            self.validate_navigation_generation(&generation)?;
                        }
                        Some(NavigationCandidate::MathText { .. })
                        | Some(NavigationCandidate::MathTextHit { .. }) => {
                            return Err(EditError::InvalidPosition);
                        }
                        None => {}
                    }
                    if intent == NavigationIntent::Horizontal && !extend {
                        self.run(Command::MoveCaret(
                            direction.ok_or(EditError::InvalidPosition)?,
                        ))
                    } else {
                        Ok(Effect::default())
                    }
                }
            };
        }
        match candidate {
            Some(NavigationCandidate::Document {
                focus,
                visual_affinity,
                generation,
            }) => {
                self.validate_navigation_generation(&generation)?;
                let anchor = if extend {
                    self.structure_selection
                        .as_ref()
                        .map(|selection| selection.anchor.clone())
                        .unwrap_or_else(|| self.selection.anchor.clone())
                } else {
                    focus.clone()
                };
                return self.set_navigation_selection_with_direction(
                    Selection { anchor, focus },
                    visual_affinity,
                    intent,
                    if intent == NavigationIntent::Horizontal {
                        direction
                    } else {
                        None
                    },
                );
            }
            Some(NavigationCandidate::MathText { .. })
            | Some(NavigationCandidate::MathTextHit { .. }) => {
                return Err(EditError::InvalidPosition);
            }
            None => {}
        }
        if intent == NavigationIntent::Horizontal
            && let Some(direction) = direction
        {
            if extend
                && let Some(environment_id) =
                    self.keyboards_block_environment_at(&self.selection.focus)
            {
                self.select_block_environment(environment_id)?;
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            if self.enter_adjacent_math_if_present(direction)? {
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            return self.run(Command::ExitBlockEnvironment(direction));
        }
        Ok(Effect::default())
    }

    /// 候选只能来自当前 Session 和当前文档代际；字体代际由 app 布局服务同步核验。
    fn validate_navigation_generation(
        &self,
        generation: &NavigationGeneration,
    ) -> Result<(), EditError> {
        if generation.session_id != self.session_id
            || generation.document_generation != self.document_generation
        {
            return Err(EditError::InvalidPosition);
        }
        Ok(())
    }

    fn keyboards_block_environment_at(
        &self,
        position: &crate::editing::Pos,
    ) -> Option<caret_doc::NodeId> {
        let ordinal = self
            .structure_index()
            .paragraph_ordinal(&position.paragraph_id)?;
        self.structure_index()
            .at(&caret_structure::StructurePosition {
                paragraph_id: position.paragraph_id.clone(),
                offset_utf8: position.offset,
            })
            .into_iter()
            .rev()
            .find(|entry| {
                entry.kind.is_block_environment()
                    && entry.range.paragraph_ordinals.contains(&ordinal)
            })
            .map(|entry| entry.id.clone())
    }

    /// 在单一入口根据按键类型分派；每种按键的状态优先级独立维护。
    pub(super) fn run_editor_key(&mut self, key: EditorKey) -> Result<Effect, EditError> {
        match key {
            EditorKey::Escape => self.handle_escape(),
            EditorKey::Enter => self.handle_enter(false),
            EditorKey::Tab => self.handle_tab(),
            EditorKey::ShiftEnter => self.handle_enter(true),
            EditorKey::Backspace => self.handle_backspace(),
            EditorKey::Delete => self.handle_delete(),
            EditorKey::Backslash => self.handle_backslash(),
            EditorKey::Dollar => self.handle_dollar(),
            EditorKey::ModC => self.handle_mod_c(),
            EditorKey::ModV => self.handle_mod_v(),
            EditorKey::ModZ { redo } => self.handle_mod_z(redo),
            EditorKey::ModSpace => self.handle_mod_space(),
            EditorKey::ModBackspace => self.handle_mod_backspace(),
            EditorKey::LeftArrow { candidate, extend } => self.handle_left_arrow(candidate, extend),
            EditorKey::RightArrow { candidate, extend } => {
                self.handle_right_arrow(candidate, extend)
            }
            EditorKey::UpArrow {
                candidate,
                extend,
                structure,
            } => self.handle_up_arrow(candidate, extend, structure),
            EditorKey::DownArrow {
                candidate,
                extend,
                structure,
            } => self.handle_down_arrow(candidate, extend, structure),
            EditorKey::Home { candidate, extend } => self.handle_home(candidate, extend),
            EditorKey::End { candidate, extend } => self.handle_end(candidate, extend),
        }
    }
}

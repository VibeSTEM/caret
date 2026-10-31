//! 按 Parley 视觉几何移动 Rust Session 中的光标和选区。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_doc::NodeId;
use caret_engine::{
    Affinity, Command, Direction, DocumentLayoutSnapshot, HitAffinity, Pos, Selection,
    StructureIndex, VisualAffinity,
};

impl DocumentViewRuntime {
    /// 按 Rust 结构索引移动段落停靠点，或按 Esc 语义收起选区/跳出环境。
    pub fn move_structure_json(&mut self, action: &str) -> Result<String, String> {
        let command = match action {
            "next" => Command::MoveStructure(Direction::Forward),
            "previous" => Command::MoveStructure(Direction::Backward),
            "expand-selection" => Command::ExpandStructureSelection,
            "exit" => Command::ExitStructure,
            _ => return Err("navigation.unknown_structure_action".to_owned()),
        };
        let effect = self
            .session
            .run(command)
            .map_err(|error| format!("navigation.structure: {error}"))?;
        if effect.selection_changed {
            self.anchor_visual_affinity = self.session.selection_visual_affinity();
            self.preferred_visual_x_pt = None;
        }
        if effect.document_changed {
            return self.layout_snapshot_json();
        }
        self.interaction_state_json()
    }

    /// 按 Parley 的视觉顺序移动焦点；非扩展选区时先折叠到对应视觉边缘。
    pub fn move_visual_caret_json(
        &mut self,
        forward: bool,
        extend: bool,
    ) -> Result<String, String> {
        let current = self.session.selection().clone();
        let selection_anchor = self
            .session
            .structure_selection_anchor()
            .cloned()
            .unwrap_or_else(|| current.anchor.clone());
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.anchor_visual_affinity;
        let target = {
            let snapshot = self.current_snapshot()?;
            if !extend && current.anchor != current.focus {
                let anchor_key = visual_caret_key(snapshot, &current.anchor, current_anchor_visual);
                let focus_key = visual_caret_key(snapshot, &current.focus, current_visual);
                match (anchor_key, focus_key) {
                    (Some(anchor_key), Some(focus_key)) => {
                        let pick_anchor = if forward {
                            anchor_key > focus_key
                        } else {
                            anchor_key < focus_key
                        };
                        Some((
                            if pick_anchor {
                                current.anchor.clone()
                            } else {
                                current.focus.clone()
                            },
                            if pick_anchor {
                                current_anchor_visual
                            } else {
                                current_visual
                            },
                            true,
                        ))
                    }
                    _ => None,
                }
            } else {
                let Some(paragraph_index) =
                    snapshot.paragraph_index_by_id(&current.focus.paragraph_id)
                else {
                    return Err("navigation.unknown_paragraph".to_owned());
                };
                let paragraph = &snapshot.paragraphs[paragraph_index];
                let cursor = paragraph
                    .layout
                    .cursor_at(current.focus.offset, current_visual)
                    .ok_or_else(|| "navigation.invalid_caret".to_owned())?;
                let moved = paragraph.layout.move_visual(cursor, forward);
                let adjacent = if paragraph.text.is_empty() {
                    Some((forward, if forward { 0 } else { usize::MAX }))
                } else if current.focus.offset == 0 {
                    Some((false, usize::MAX))
                } else if current.focus.offset == paragraph.text.len() {
                    Some((true, 0))
                } else {
                    None
                };
                moved
                    .map(|cursor| {
                        (
                            Pos {
                                offset: cursor.byte_index(),
                                affinity: current.focus.affinity,
                                ..current.focus.clone()
                            },
                            cursor.affinity(),
                            false,
                        )
                    })
                    .or_else(|| {
                        let (adjacent_forward, adjacent_offset) = adjacent?;
                        let adjacent_index =
                            snapshot.adjacent_paragraph_index(paragraph_index, adjacent_forward)?;
                        let adjacent = snapshot.paragraphs.get(adjacent_index)?;
                        let offset = if adjacent_offset == usize::MAX {
                            adjacent.text.len()
                        } else {
                            adjacent_offset
                        };
                        let visual_affinity = if offset == 0 {
                            VisualAffinity::Downstream
                        } else {
                            VisualAffinity::Upstream
                        };
                        let cursor = adjacent.layout.cursor_at(offset, visual_affinity)?;
                        Some((
                            Pos {
                                story: adjacent.story.clone(),
                                paragraph_id: adjacent.id.clone(),
                                offset,
                                affinity: if offset == 0 {
                                    Affinity::Downstream
                                } else {
                                    Affinity::Upstream
                                },
                            },
                            cursor.affinity(),
                            false,
                        ))
                    })
            }
        };
        if !extend && current.anchor == current.focus {
            let exit_affinity = self
                .session
                .navigation_inline_scope_exit_affinity(
                    target.as_ref().map(|(candidate, _, _)| candidate),
                )
                .map_err(|error| format!("navigation.inline_scope: {error}"))?;
            if let Some(affinity) = exit_affinity {
                let position = Pos {
                    affinity,
                    ..current.focus
                };
                self.session
                    .set_selection_with_visual_affinity(
                        Some(Selection {
                            anchor: position.clone(),
                            focus: position,
                        }),
                        current_visual,
                    )
                    .map_err(|error| format!("selection.set: {error}"))?;
                self.anchor_visual_affinity = current_visual;
                self.preferred_visual_x_pt = None;
                return self.interaction_state_json();
            }
        }
        let Some((focus, visual_affinity, collapsed_selection)) = target else {
            if extend
                && let Some(environment_id) =
                    block_environment_at(self.session.structure_index(), &current.focus)
            {
                self.session
                    .select_block_environment(environment_id)
                    .map_err(|error| format!("selection.environment: {error}"))?;
                return self.interaction_state_json();
            }
            if extend || current.anchor != current.focus {
                return self.interaction_state_json();
            }
            let Some(paragraph) = self
                .current_snapshot()?
                .paragraph_by_id(&current.focus.paragraph_id)
            else {
                return Err("navigation.unknown_paragraph".to_owned());
            };
            let exit_direction = if paragraph.text.is_empty() {
                if forward {
                    Direction::Forward
                } else {
                    Direction::Backward
                }
            } else if current.focus.offset == paragraph.text.len() {
                Direction::Forward
            } else if current.focus.offset == 0 {
                Direction::Backward
            } else {
                return self.interaction_state_json();
            };
            let effect = self
                .session
                .run(Command::ExitBlockEnvironment(exit_direction))
                .map_err(|error| format!("navigation.exit_block: {error}"))?;
            if effect.document_changed {
                self.anchor_visual_affinity = self.session.selection_visual_affinity();
                self.preferred_visual_x_pt = None;
                return self.layout_snapshot_json();
            }
            return self.interaction_state_json();
        };
        if !extend
            && current.anchor == current.focus
            && focus.paragraph_id != current.focus.paragraph_id
        {
            let paragraph = self
                .current_snapshot()?
                .paragraph_by_id(&current.focus.paragraph_id)
                .ok_or_else(|| "navigation.unknown_paragraph".to_owned())?;
            let current_text_len = paragraph.text.len();
            if let Some(direction) = environment_exit_direction(
                self.session.structure_index(),
                &current.focus,
                &focus,
                current_text_len,
                forward,
            ) {
                let effect = self
                    .session
                    .run(Command::ExitBlockEnvironment(direction))
                    .map_err(|error| format!("navigation.exit_block: {error}"))?;
                if effect.document_changed {
                    self.anchor_visual_affinity = self.session.selection_visual_affinity();
                    self.preferred_visual_x_pt = None;
                    return self.layout_snapshot_json();
                }
                return self.interaction_state_json();
            }
        }
        let selection = if extend && !collapsed_selection {
            Selection {
                anchor: selection_anchor,
                focus,
            }
        } else {
            Selection {
                anchor: focus.clone(),
                focus,
            }
        };
        self.session
            .set_navigation_selection(selection, visual_affinity)
            .map_err(|error| format!("selection.set: {error}"))?;
        self.anchor_visual_affinity = if extend && !collapsed_selection {
            current_anchor_visual
        } else {
            visual_affinity
        };
        self.preferred_visual_x_pt = None;
        self.interaction_state_json()
    }

    /// 按 Parley 视觉行和保留的目标 x 上下移动，并按需扩展现有选区。
    pub fn move_vertical_caret_json(
        &mut self,
        line_delta: isize,
        extend: bool,
    ) -> Result<String, String> {
        let current = self.session.selection().clone();
        let selection_anchor = self
            .session
            .structure_selection_anchor()
            .cloned()
            .unwrap_or_else(|| current.anchor.clone());
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.anchor_visual_affinity;
        let preferred_visual_x = self.preferred_visual_x_pt;
        let target = {
            let snapshot = self.current_snapshot()?;
            let paragraph_index = snapshot
                .paragraph_index_by_id(&current.focus.paragraph_id)
                .ok_or_else(|| "navigation.unknown_paragraph".to_owned())?;
            let paragraph = &snapshot.paragraphs[paragraph_index];
            let cursor = paragraph
                .layout
                .cursor_at(current.focus.offset, current_visual)
                .ok_or_else(|| "navigation.invalid_caret".to_owned())?;
            let moved = paragraph
                .layout
                .move_vertical(cursor, line_delta, preferred_visual_x);
            moved
                .map(|(cursor, preferred_x)| {
                    (
                        Pos {
                            offset: cursor.byte_index(),
                            affinity: current.focus.affinity,
                            ..current.focus.clone()
                        },
                        cursor.affinity(),
                        preferred_x,
                    )
                })
                .or_else(|| {
                    let adjacent_index =
                        snapshot.adjacent_paragraph_index(paragraph_index, line_delta > 0)?;
                    let adjacent = snapshot.paragraphs.get(adjacent_index)?;
                    let line_index = if line_delta < 0 {
                        adjacent.layout.paragraph.lines.len().checked_sub(1)?
                    } else {
                        0
                    };
                    let preferred_x = preferred_visual_x
                        .unwrap_or_else(|| paragraph.layout.cursor_geometry(cursor, 0.0).x0 as f32);
                    let hit = adjacent
                        .layout
                        .cursor_from_point_on_line(preferred_x, line_index)?;
                    let position_affinity = match hit.logical_affinity {
                        HitAffinity::Upstream => Affinity::Upstream,
                        HitAffinity::Downstream => Affinity::Downstream,
                    };
                    Some((
                        Pos {
                            story: adjacent.story.clone(),
                            paragraph_id: adjacent.id.clone(),
                            offset: hit.cursor.byte_index(),
                            affinity: position_affinity,
                        },
                        hit.cursor.affinity(),
                        preferred_x,
                    ))
                })
        };
        let Some((focus, visual_affinity, preferred_x)) = target else {
            return self.interaction_state_json();
        };
        let selection = if extend {
            Selection {
                anchor: selection_anchor,
                focus,
            }
        } else {
            Selection {
                anchor: focus.clone(),
                focus,
            }
        };
        self.session
            .set_navigation_selection(selection, visual_affinity)
            .map_err(|error| format!("selection.set: {error}"))?;
        self.anchor_visual_affinity = if extend {
            current_anchor_visual
        } else {
            visual_affinity
        };
        self.preferred_visual_x_pt = Some(preferred_x);
        self.interaction_state_json()
    }

    /// 将焦点移到当前视觉行首尾，Shift 扩展时保留原锚点。
    pub fn move_to_visual_line_edge_json(
        &mut self,
        end: bool,
        extend: bool,
    ) -> Result<String, String> {
        let current = self.session.selection().clone();
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.anchor_visual_affinity;
        let target = {
            let snapshot = self.current_snapshot()?;
            let paragraph = snapshot
                .paragraph_by_id(&current.focus.paragraph_id)
                .ok_or_else(|| "navigation.unknown_paragraph".to_owned())?;
            let cursor = paragraph
                .layout
                .cursor_at(current.focus.offset, current_visual)
                .ok_or_else(|| "navigation.invalid_caret".to_owned())?;
            paragraph
                .layout
                .move_to_visual_line_edge(cursor, end)
                .map(|cursor| {
                    (
                        Pos {
                            offset: cursor.byte_index(),
                            affinity: current.focus.affinity,
                            ..current.focus.clone()
                        },
                        cursor.affinity(),
                    )
                })
        };
        let Some((focus, visual_affinity)) = target else {
            return self.interaction_state_json();
        };
        let selection = if extend {
            Selection {
                anchor: current.anchor,
                focus,
            }
        } else {
            Selection {
                anchor: focus.clone(),
                focus,
            }
        };
        self.session
            .set_navigation_selection(selection, visual_affinity)
            .map_err(|error| format!("selection.set: {error}"))?;
        self.anchor_visual_affinity = if extend {
            current_anchor_visual
        } else {
            visual_affinity
        };
        self.preferred_visual_x_pt = None;
        self.interaction_state_json()
    }
}

pub(super) fn block_environment_at(
    structure_index: &StructureIndex,
    position: &Pos,
) -> Option<NodeId> {
    let ordinal = structure_index.paragraph_ordinal(&position.paragraph_id)?;
    structure_index
        .at(&caret_engine::StructurePosition {
            paragraph_id: position.paragraph_id.clone(),
            offset_utf8: position.offset,
        })
        .into_iter()
        .rev()
        .find(|entry| {
            entry.kind.is_block_environment() && entry.range.paragraph_ordinals.contains(&ordinal)
        })
        .map(|entry| entry.id.clone())
}

fn logical_boundary_direction(
    offset: usize,
    text_len: usize,
    visual_forward: bool,
) -> Option<Direction> {
    if text_len == 0 {
        Some(if visual_forward {
            Direction::Forward
        } else {
            Direction::Backward
        })
    } else if offset == text_len {
        Some(Direction::Forward)
    } else if offset == 0 {
        Some(Direction::Backward)
    } else {
        None
    }
}

fn environment_exit_direction(
    structure_index: &StructureIndex,
    current: &Pos,
    target: &Pos,
    current_text_len: usize,
    visual_forward: bool,
) -> Option<Direction> {
    let current_environment_ids = structure_index
        .at(&caret_engine::StructurePosition {
            paragraph_id: current.paragraph_id.clone(),
            offset_utf8: current.offset,
        })
        .into_iter()
        .filter(|entry| entry.kind.is_block_environment())
        .map(|entry| &entry.id)
        .collect::<Vec<_>>();
    if current_environment_ids.is_empty() {
        return None;
    }
    let target_environment_ids = structure_index
        .at(&caret_engine::StructurePosition {
            paragraph_id: target.paragraph_id.clone(),
            offset_utf8: target.offset,
        })
        .into_iter()
        .filter(|entry| entry.kind.is_block_environment())
        .map(|entry| &entry.id)
        .collect::<Vec<_>>();
    let leaves_environment = current_environment_ids
        .iter()
        .any(|current_id| !target_environment_ids.contains(current_id));
    if !leaves_environment {
        return None;
    }
    logical_boundary_direction(current.offset, current_text_len, visual_forward)
}

fn visual_caret_key(
    snapshot: &DocumentLayoutSnapshot,
    position: &Pos,
    affinity: VisualAffinity,
) -> Option<(usize, f64, f64)> {
    let caret = snapshot.caret_geometry(position, affinity)?;
    Some((caret.page_index, caret.y0_pt, caret.x_pt))
}

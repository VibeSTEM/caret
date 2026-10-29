//! 按 Parley 视觉几何移动 Rust Session 中的光标和选区。

use super::DocumentViewRuntime;
use caret_engine::{
    Affinity, Command, Direction, DocumentLayoutSnapshot, HitAffinity, Pos, Selection,
    VisualAffinity,
};

impl DocumentViewRuntime {
    /// 按 Rust 结构索引移动段落停靠点，或按 Esc 语义收起选区/跳出环境。
    pub fn move_structure_json(&mut self, action: &str) -> Result<String, String> {
        let command = match action {
            "next" => Command::MoveStructure(Direction::Forward),
            "previous" => Command::MoveStructure(Direction::Backward),
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
        self.interaction_state_json()
    }

    /// 按 Parley 的视觉顺序移动焦点；非扩展选区时先折叠到对应视觉边缘。
    pub fn move_visual_caret_json(
        &mut self,
        forward: bool,
        extend: bool,
    ) -> Result<String, String> {
        let current = self.session.selection().clone();
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
                let (adjacent_index, adjacent_offset) = if paragraph.text.is_empty() {
                    if forward {
                        (paragraph_index.checked_add(1), 0)
                    } else {
                        (paragraph_index.checked_sub(1), usize::MAX)
                    }
                } else if current.focus.offset == 0 {
                    (paragraph_index.checked_sub(1), usize::MAX)
                } else if current.focus.offset == paragraph.text.len() {
                    (paragraph_index.checked_add(1), 0)
                } else {
                    (None, 0)
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
                        let adjacent = snapshot.paragraphs.get(adjacent_index?)?;
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
                                paragraph_id: adjacent.id.clone(),
                                offset,
                                affinity: if offset == 0 {
                                    Affinity::Downstream
                                } else {
                                    Affinity::Upstream
                                },
                                ..current.focus.clone()
                            },
                            cursor.affinity(),
                            false,
                        ))
                    })
            }
        };
        let Some((focus, visual_affinity, collapsed_selection)) = target else {
            return self.interaction_state_json();
        };
        let selection = if extend && !collapsed_selection {
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
                    let adjacent_index = if line_delta < 0 {
                        paragraph_index.checked_sub(1)
                    } else {
                        paragraph_index.checked_add(1)
                    }?;
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
                            paragraph_id: adjacent.id.clone(),
                            offset: hit.cursor.byte_index(),
                            affinity: position_affinity,
                            ..current.focus.clone()
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

fn visual_caret_key(
    snapshot: &DocumentLayoutSnapshot,
    position: &Pos,
    affinity: VisualAffinity,
) -> Option<(usize, f64, f64)> {
    let caret = snapshot.caret_geometry(position, affinity)?;
    Some((caret.page_index, caret.y0_pt, caret.x_pt))
}

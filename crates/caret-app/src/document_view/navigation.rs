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
        if let Some(result) = move_math_visual(self, forward, extend) {
            return result;
        }
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
        commit_text_navigation(
            self,
            selection_anchor,
            focus,
            TextNavigationState {
                current_anchor_visual,
                focus_visual: visual_affinity,
                extend: extend && !collapsed_selection,
                preferred_x: None,
            },
            |session, anchor, focus, _, focus_visual| {
                session
                    .set_navigation_selection(Selection { anchor, focus }, focus_visual)
                    .map(|_| ())
                    .map_err(|error| format!("selection.set: {error}"))
            },
        )
    }

    /// 按 Parley 视觉行和保留的目标 x 上下移动，并按需扩展现有选区。
    pub fn move_vertical_caret_json(
        &mut self,
        line_delta: isize,
        extend: bool,
    ) -> Result<String, String> {
        if let Some(result) = move_math_vertical(self, line_delta, extend) {
            return result;
        }
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
        commit_text_navigation(
            self,
            selection_anchor,
            focus,
            TextNavigationState {
                current_anchor_visual,
                focus_visual: visual_affinity,
                extend,
                preferred_x: Some(preferred_x),
            },
            |session, anchor, focus, _, focus_visual| {
                session
                    .set_navigation_selection(Selection { anchor, focus }, focus_visual)
                    .map(|_| ())
                    .map_err(|error| format!("selection.set: {error}"))
            },
        )
    }

    /// 将焦点移到当前视觉行首尾，Shift 扩展时保留原锚点。
    pub fn move_to_visual_line_edge_json(
        &mut self,
        end: bool,
        extend: bool,
    ) -> Result<String, String> {
        if let Some(result) = move_math_line_edge(self, end, extend) {
            return result;
        }
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
        commit_text_navigation(
            self,
            current.anchor,
            focus,
            TextNavigationState {
                current_anchor_visual,
                focus_visual: visual_affinity,
                extend,
                preferred_x: None,
            },
            |session, anchor, focus, _, focus_visual| {
                session
                    .set_navigation_selection(Selection { anchor, focus }, focus_visual)
                    .map(|_| ())
                    .map_err(|error| format!("selection.set: {error}"))
            },
        )
    }
}

fn move_math_visual(
    runtime: &mut DocumentViewRuntime,
    forward: bool,
    extend: bool,
) -> Option<Result<String, String>> {
    if runtime.session.math_tree_input_context().is_some() {
        let direction = if forward {
            Direction::Forward
        } else {
            Direction::Backward
        };
        let result = if extend {
            Ok(caret_engine::Effect::default())
        } else {
            runtime
                .session
                .run(Command::MoveCaret(direction))
                .map_err(|error| format!("navigation.math_tree: {error}"))
        };
        return Some(result.and_then(|effect| {
            if effect.selection_changed {
                runtime.anchor_visual_affinity = runtime.session.selection_visual_affinity();
                runtime.preferred_visual_x_pt = None;
            }
            runtime.interaction_state_json()
        }));
    }
    let context = runtime.session.math_text_input_context()?;
    let anchor_visual_affinity = context.anchor_visual_affinity;
    let (position, visual_affinity, collapsed_selection) = if !extend
        && context.anchor_utf16 != context.focus_utf16
    {
        let snapshot = match runtime.current_snapshot() {
            Ok(snapshot) => snapshot,
            Err(error) => return Some(Err(error)),
        };
        let anchor_key = snapshot.math_text_caret_geometry_for_visual_order(
            &context.target,
            &context.anchor,
            anchor_visual_affinity,
        );
        let focus_key = snapshot.math_text_caret_geometry_for_visual_order(
            &context.target,
            &context.focus,
            context.visual_affinity,
        );
        match (anchor_key, focus_key) {
            (Some(anchor), Some(focus)) => {
                let anchor_key = (anchor.page_index, anchor.y0_pt, anchor.x_pt);
                let focus_key = (focus.page_index, focus.y0_pt, focus.x_pt);
                let pick_anchor = if forward {
                    anchor_key > focus_key
                } else {
                    anchor_key < focus_key
                };
                if pick_anchor {
                    (context.anchor.clone(), anchor_visual_affinity, true)
                } else {
                    (context.focus.clone(), context.visual_affinity, true)
                }
            }
            _ => return Some(Err("navigation.invalid_math_caret".to_owned())),
        }
    } else {
        let movement = runtime.current_snapshot().and_then(|snapshot| {
            snapshot
                .move_math_text_visual(
                    &context.target,
                    &context.focus,
                    context.visual_affinity,
                    forward,
                )
                .ok_or_else(|| "navigation.math_text_edge".to_owned())
        });
        match movement {
            Ok((position, visual_affinity)) => (position, visual_affinity, false),
            Err(error) if error == "navigation.math_text_edge" && !extend => {
                let direction = if context.focus_utf16 == 0 {
                    Direction::Backward
                } else if context.focus_utf16 == context.surrounding_text.encode_utf16().count() {
                    Direction::Forward
                } else {
                    return Some(runtime.interaction_state_json());
                };
                return Some(
                    runtime
                        .session
                        .run(Command::MoveCaret(direction))
                        .map_err(|error| format!("navigation.math_text: {error}"))
                        .and_then(|effect| {
                            if effect.selection_changed {
                                runtime.anchor_visual_affinity =
                                    runtime.session.selection_visual_affinity();
                                runtime.preferred_visual_x_pt = None;
                            }
                            runtime.interaction_state_json()
                        }),
                );
            }
            Err(error) if error == "navigation.math_text_edge" => {
                return Some(runtime.interaction_state_json());
            }
            Err(error) => return Some(Err(error)),
        }
    };
    Some(commit_text_navigation(
        runtime,
        context.anchor,
        position,
        TextNavigationState {
            current_anchor_visual: anchor_visual_affinity,
            focus_visual: visual_affinity,
            extend: extend && !collapsed_selection,
            preferred_x: None,
        },
        |session, anchor, focus, anchor_visual, focus_visual| {
            session
                .set_math_text_selection_positions_with_visual_affinities(
                    anchor,
                    focus,
                    anchor_visual,
                    focus_visual,
                )
                .map(|_| ())
                .map_err(|error| format!("selection.math_text: {error}"))
        },
    ))
}

fn move_math_vertical(
    runtime: &mut DocumentViewRuntime,
    line_delta: isize,
    extend: bool,
) -> Option<Result<String, String>> {
    let context = runtime.session.math_text_input_context()?;
    let anchor_visual_affinity = context.anchor_visual_affinity;
    let preferred_x = runtime.preferred_visual_x_pt;
    let moved = runtime.current_snapshot().and_then(|snapshot| {
        snapshot
            .move_math_text_vertical_target(
                &context.target,
                &context.focus,
                context.visual_affinity,
                line_delta,
                preferred_x,
            )
            .ok_or_else(|| "navigation.math_text_edge".to_owned())
    });
    let moved = match moved {
        Ok(moved) => moved,
        Err(error) if error == "navigation.math_text_edge" => {
            return Some(runtime.interaction_state_json());
        }
        Err(error) => return Some(Err(error)),
    };
    let (position, visual_affinity, preferred_x) = match moved {
        caret_engine::MathTextVerticalTarget::SameTextIsland {
            position,
            visual_affinity,
            preferred_x_pt,
        } => (position, visual_affinity, preferred_x_pt),
        caret_engine::MathTextVerticalTarget::DifferentTextIsland(hit) if !extend => {
            let preferred_x = preferred_x.unwrap_or_else(|| {
                runtime
                    .current_snapshot()
                    .ok()
                    .and_then(|snapshot| {
                        snapshot.math_text_caret_geometry(
                            &context.target,
                            &context.focus,
                            context.visual_affinity,
                        )
                    })
                    .map(|caret| caret.x_pt as f32)
                    .unwrap_or(hit.caret.x_pt as f32)
            });
            let focused = runtime
                .session
                .focus_math_text_hit(&hit, hit.font_generation)
                .map_err(|error| format!("navigation.math_text_focus: {error}"));
            return Some(focused.and_then(|_| {
                runtime.anchor_visual_affinity = hit.visual_affinity;
                runtime.preferred_visual_x_pt = Some(preferred_x);
                runtime.interaction_state_json()
            }));
        }
        caret_engine::MathTextVerticalTarget::DifferentTextIsland(_) => {
            return Some(runtime.interaction_state_json());
        }
    };
    Some(commit_text_navigation(
        runtime,
        context.anchor,
        position,
        TextNavigationState {
            current_anchor_visual: anchor_visual_affinity,
            focus_visual: visual_affinity,
            extend,
            preferred_x: Some(preferred_x),
        },
        |session, anchor, focus, anchor_visual, focus_visual| {
            session
                .set_math_text_selection_positions_with_visual_affinities(
                    anchor,
                    focus,
                    anchor_visual,
                    focus_visual,
                )
                .map(|_| ())
                .map_err(|error| format!("selection.math_text: {error}"))
        },
    ))
}

fn move_math_line_edge(
    runtime: &mut DocumentViewRuntime,
    end: bool,
    extend: bool,
) -> Option<Result<String, String>> {
    let context = runtime.session.math_text_input_context()?;
    let anchor_visual_affinity = context.anchor_visual_affinity;
    let moved = runtime.current_snapshot().and_then(|snapshot| {
        snapshot
            .move_math_text_line_edge(
                &context.target,
                &context.focus,
                context.visual_affinity,
                end,
            )
            .ok_or_else(|| "navigation.math_text_edge".to_owned())
    });
    let (position, visual_affinity) = match moved {
        Ok(moved) => moved,
        Err(error) if error == "navigation.math_text_edge" => {
            return Some(runtime.interaction_state_json());
        }
        Err(error) => return Some(Err(error)),
    };
    Some(commit_text_navigation(
        runtime,
        context.anchor,
        position,
        TextNavigationState {
            current_anchor_visual: anchor_visual_affinity,
            focus_visual: visual_affinity,
            extend,
            preferred_x: None,
        },
        |session, anchor, focus, anchor_visual, focus_visual| {
            session
                .set_math_text_selection_positions_with_visual_affinities(
                    anchor,
                    focus,
                    anchor_visual,
                    focus_visual,
                )
                .map(|_| ())
                .map_err(|error| format!("selection.math_text: {error}"))
        },
    ))
}

/// 正文和公式文本共用 Shift 锚点、光标亲和侧及垂直目标 x 的提交规则。
struct TextNavigationState {
    current_anchor_visual: VisualAffinity,
    focus_visual: VisualAffinity,
    extend: bool,
    preferred_x: Option<f32>,
}

fn commit_text_navigation<T: Clone>(
    runtime: &mut DocumentViewRuntime,
    current_anchor: T,
    focus: T,
    state: TextNavigationState,
    commit: impl FnOnce(
        &mut caret_engine::Session,
        T,
        T,
        VisualAffinity,
        VisualAffinity,
    ) -> Result<(), String>,
) -> Result<String, String> {
    let (anchor, anchor_visual) = if state.extend {
        (current_anchor, state.current_anchor_visual)
    } else {
        (focus.clone(), state.focus_visual)
    };
    commit(
        &mut runtime.session,
        anchor,
        focus,
        anchor_visual,
        state.focus_visual,
    )?;
    runtime.anchor_visual_affinity = anchor_visual;
    runtime.preferred_visual_x_pt = state.preferred_x;
    runtime.interaction_state_json()
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

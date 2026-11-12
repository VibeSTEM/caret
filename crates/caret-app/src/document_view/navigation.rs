//! 按 Parley 视觉几何移动 Rust Session 中的光标和选区。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::DocumentViewRuntime;
use caret_engine::{
    Affinity, Command, Direction, DocumentLayoutSnapshot, EditorKey, Effect, HitAffinity,
    MathEditTarget, NavigationCandidate, Pos, VisualAffinity,
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
        if self.session.command_line().is_some() {
            return dispatch_navigation_key(
                self,
                if forward {
                    EditorKey::RightArrow {
                        candidate: None,
                        extend,
                    }
                } else {
                    EditorKey::LeftArrow {
                        candidate: None,
                        extend,
                    }
                },
                "navigation.command_line",
            );
        }
        if self.session.math_text_input_context().is_some() {
            return move_math_visual(self, forward, extend)
                .unwrap_or_else(|| self.interaction_state_json());
        }
        // 结构范围是文档级选区：普通方向键交给引擎折叠到范围一侧并消费按键，
        // 不能按折叠在焦点端的文本光标再算一个视觉候选。
        if !extend && self.session.has_structure_selection() {
            return dispatch_navigation_key(
                self,
                if forward {
                    EditorKey::RightArrow {
                        candidate: None,
                        extend: false,
                    }
                } else {
                    EditorKey::LeftArrow {
                        candidate: None,
                        extend: false,
                    }
                },
                "navigation.structure_collapse",
            );
        }
        let current = self.session.selection().clone();
        let selection_anchor = self
            .session
            .structure_selection_anchor()
            .cloned()
            .unwrap_or_else(|| current.anchor.clone());
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.session.selection_anchor_visual_affinity();
        let target = {
            let snapshot = self.current_snapshot()?;
            if !extend && current.anchor != current.focus {
                let anchor_key = visual_caret_key(snapshot, &current.anchor, current_anchor_visual);
                let focus_key = visual_caret_key(snapshot, &current.focus, current_visual);
                match (anchor_key, focus_key) {
                    (Some(anchor_key), Some(focus_key)) => {
                        let pick_anchor = pick_visual_anchor(forward, anchor_key, focus_key);
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
            return dispatch_navigation_key(
                self,
                if forward {
                    EditorKey::RightArrow {
                        candidate: None,
                        extend,
                    }
                } else {
                    EditorKey::LeftArrow {
                        candidate: None,
                        extend,
                    }
                },
                "navigation.horizontal",
            );
        };
        let extend_selection = extend && !collapsed_selection;
        let generation = navigation_generation(self)?;
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
            |session, _, focus, _, focus_visual| {
                let candidate = Some(NavigationCandidate::Document {
                    focus,
                    visual_affinity: focus_visual,
                    generation: generation.clone(),
                });
                session
                    .run(Command::EditorKey(if forward {
                        EditorKey::RightArrow {
                            candidate,
                            extend: extend_selection,
                        }
                    } else {
                        EditorKey::LeftArrow {
                            candidate,
                            extend: extend_selection,
                        }
                    }))
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
        if self.session.command_line().is_some() {
            return dispatch_navigation_key(
                self,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.command_line",
            );
        }
        if !extend && self.session.math_block_boundary_exit().is_some() {
            return dispatch_navigation_key(
                self,
                vertical_navigation_key(line_delta, None, false),
                "navigation.math_block_boundary",
            );
        }
        // InlineMath 在公式内没有垂直候选时先退出 MathEdit，同一次按键还要继续
        // 文档纵向导航；记录进入数学分支前的 Inline 目标，退出后按文档路径重放。
        let inline_math_edit = self
            .session
            .math_edit()
            .is_some_and(|edit| matches!(edit.target, MathEditTarget::Inline { .. }));
        let math_result = if self.session.math_text_input_context().is_some() {
            move_math_vertical(self, line_delta, extend)
        } else if self.session.math_tree_input_context().is_some() {
            move_math_tree_vertical(self, line_delta, extend)
        } else if inline_math_edit {
            // InlineMath 的 Source 分支没有本地垂直选区：直接按公式边界退出，
            // 随后由本函数在同一次按键内继续文档纵向导航。
            Some(dispatch_navigation_key(
                self,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.inline_math_source_exit",
            ))
        } else {
            None
        };
        if let Some(result) = math_result {
            let result = result?;
            if inline_math_edit && self.session.math_edit().is_none() {
                // 公式已在本次按键中退出到 U+FFFC 前/后侧，继续同一方向的文档
                // 纵向导航；此时 math_edit 为 None，递归不会再次进入数学分支。
                return self.move_vertical_caret_json(line_delta, extend);
            }
            return Ok(result);
        }
        let at_vertical_edge = {
            let focus = self.session.selection().focus.clone();
            let visual_affinity = self.session.selection_visual_affinity();
            let preferred_x = self.preferred_visual_x_pt;
            let snapshot = self.current_snapshot()?;
            let Some(paragraph_index) = snapshot.paragraph_index_by_id(&focus.paragraph_id) else {
                return Err("navigation.unknown_paragraph".to_owned());
            };
            let paragraph = &snapshot.paragraphs[paragraph_index];
            let Some(cursor) = paragraph.layout.cursor_at(focus.offset, visual_affinity) else {
                return Err("navigation.invalid_caret".to_owned());
            };
            paragraph
                .layout
                .move_vertical(cursor, line_delta, preferred_x)
                .is_none()
        };
        if at_vertical_edge {
            let result = dispatch_navigation_key(
                self,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.vertical_edge",
            )?;
            if self.session.math_edit().is_some()
                || self.session.math_block_boundary_exit().is_some()
            {
                self.preferred_visual_x_pt = None;
                return Ok(result);
            }
        }
        let current = self.session.selection().clone();
        let selection_anchor = self
            .session
            .structure_selection_anchor()
            .cloned()
            .unwrap_or_else(|| current.anchor.clone());
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.session.selection_anchor_visual_affinity();
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
            return dispatch_navigation_key(
                self,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.vertical",
            );
        };
        let generation = navigation_generation(self)?;
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
            |session, _, focus, _, focus_visual| {
                let candidate = Some(NavigationCandidate::Document {
                    focus,
                    visual_affinity: focus_visual,
                    generation: generation.clone(),
                });
                session
                    .run(Command::EditorKey(vertical_navigation_key(
                        line_delta, candidate, extend,
                    )))
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
        if self.session.command_line().is_some() {
            return dispatch_navigation_key(
                self,
                if end {
                    EditorKey::End {
                        candidate: None,
                        extend,
                    }
                } else {
                    EditorKey::Home {
                        candidate: None,
                        extend,
                    }
                },
                "navigation.command_line",
            );
        }
        if self.session.math_text_input_context().is_some()
            && let Some(result) = move_math_line_edge(self, end, extend)
        {
            return result;
        }
        let current = self.session.selection().clone();
        let current_visual = self.session.selection_visual_affinity();
        let current_anchor_visual = self.session.selection_anchor_visual_affinity();
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
            return dispatch_navigation_key(
                self,
                if end {
                    EditorKey::End {
                        candidate: None,
                        extend,
                    }
                } else {
                    EditorKey::Home {
                        candidate: None,
                        extend,
                    }
                },
                "navigation.line_edge",
            );
        };
        let generation = navigation_generation(self)?;
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
            |session, _, focus, _, focus_visual| {
                let candidate = Some(NavigationCandidate::Document {
                    focus,
                    visual_affinity: focus_visual,
                    generation: generation.clone(),
                });
                session
                    .run(Command::EditorKey(if end {
                        EditorKey::End { candidate, extend }
                    } else {
                        EditorKey::Home { candidate, extend }
                    }))
                    .map_err(|error| format!("selection.set: {error}"))
            },
        )
    }
}

/// 将无布局候选或 UI 已计算的候选交给统一按键入口处理。
fn dispatch_navigation_key(
    runtime: &mut DocumentViewRuntime,
    key: EditorKey,
    error_context: &str,
) -> Result<String, String> {
    let effect = runtime
        .session
        .run(Command::EditorKey(key))
        .map_err(|error| format!("{error_context}: {error}"))?;
    runtime.publish_editor_effect_json(effect, error_context)
}

/// 确认几何候选仍来自当前会话、文档和字体代际。
fn navigation_generation(
    runtime: &mut DocumentViewRuntime,
) -> Result<caret_engine::NavigationGeneration, String> {
    let snapshot = runtime.current_snapshot()?.clone();
    let session_id = runtime.session.session_id().clone();
    if snapshot.session_id.as_ref() != Some(&session_id)
        || snapshot.document_generation != runtime.session.document_generation()
        || snapshot.font_generation != runtime.layout.font_generation()
    {
        return Err("navigation.stale_snapshot".to_owned());
    }
    Ok(caret_engine::NavigationGeneration {
        session_id,
        document_generation: snapshot.document_generation,
        font_generation: snapshot.font_generation,
    })
}

fn move_math_visual(
    runtime: &mut DocumentViewRuntime,
    forward: bool,
    extend: bool,
) -> Option<Result<String, String>> {
    let context = runtime.session.math_text_input_context()?;
    let anchor_visual_affinity = context.anchor_visual_affinity;
    let (position, visual_affinity, collapsed_selection) =
        if !extend && context.anchor_utf16 != context.focus_utf16 {
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
                    let pick_anchor = pick_visual_anchor(forward, anchor_key, focus_key);
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
                Err(error) if error == "navigation.math_text_edge" => {
                    if !extend
                        && context.focus_utf16 != 0
                        && context.focus_utf16 != context.surrounding_text.encode_utf16().count()
                    {
                        return Some(runtime.interaction_state_json());
                    }
                    return Some(dispatch_navigation_key(
                        runtime,
                        if forward {
                            EditorKey::RightArrow {
                                candidate: None,
                                extend,
                            }
                        } else {
                            EditorKey::LeftArrow {
                                candidate: None,
                                extend,
                            }
                        },
                        "navigation.math_text",
                    ));
                }
                Err(error) => return Some(Err(error)),
            }
        };
    let generation = match navigation_generation(runtime) {
        Ok(generation) => generation,
        Err(error) => return Some(Err(error)),
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
        |session, _, focus, _, focus_visual| {
            let candidate = Some(NavigationCandidate::MathText {
                focus,
                visual_affinity: focus_visual,
                generation: generation.clone(),
            });
            session
                .run(Command::EditorKey(if forward {
                    EditorKey::RightArrow { candidate, extend }
                } else {
                    EditorKey::LeftArrow { candidate, extend }
                }))
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
    let preferred_x = runtime.preferred_visual_x_pt.or_else(|| {
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
    });
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
            if let Some(preferred_x) = preferred_x {
                runtime.preferred_visual_x_pt = Some(preferred_x);
            }
            return Some(dispatch_navigation_key(
                runtime,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.math_text",
            ));
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
            let preferred_x = preferred_x.unwrap_or(hit.caret.x_pt as f32);
            let generation = match navigation_generation(runtime) {
                Ok(generation) => generation,
                Err(error) => return Some(Err(error)),
            };
            let old_context = runtime.session.math_text_input_context();
            let result = dispatch_navigation_key(
                runtime,
                vertical_navigation_key(
                    line_delta,
                    Some(NavigationCandidate::MathTextHit {
                        hit: hit.clone(),
                        generation,
                    }),
                    false,
                ),
                "navigation.math_text_focus",
            );
            if runtime
                .session
                .math_text_input_context()
                .is_some_and(|context| {
                    context.target == hit.target && context.focus == hit.position
                })
                && old_context.is_none_or(|context| context.focus != hit.position)
            {
                runtime.preferred_visual_x_pt = Some(preferred_x);
            }
            return Some(result);
        }
        caret_engine::MathTextVerticalTarget::DifferentMathStructure(hit) if !extend => {
            let preferred_x = preferred_x.unwrap_or(hit.caret.x_pt as f32);
            let generation = match navigation_generation(runtime) {
                Ok(generation) => generation,
                Err(error) => return Some(Err(error)),
            };
            let old_math_edit = runtime.session.math_edit().cloned();
            let result = dispatch_navigation_key(
                runtime,
                vertical_navigation_key(
                    line_delta,
                    Some(NavigationCandidate::MathTreeHit { hit, generation }),
                    false,
                ),
                "navigation.math_text_structure_focus",
            );
            if result.is_ok() && runtime.session.math_edit() != old_math_edit.as_ref() {
                runtime.preferred_visual_x_pt = Some(preferred_x);
            }
            return Some(result);
        }
        caret_engine::MathTextVerticalTarget::DifferentTextIsland(_)
        | caret_engine::MathTextVerticalTarget::DifferentMathStructure(_) => {
            if let Some(preferred_x) = preferred_x {
                runtime.preferred_visual_x_pt = Some(preferred_x);
            }
            return Some(dispatch_navigation_key(
                runtime,
                vertical_navigation_key(line_delta, None, extend),
                "navigation.math_text",
            ));
        }
    };
    let generation = match navigation_generation(runtime) {
        Ok(generation) => generation,
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
            preferred_x: Some(preferred_x),
        },
        |session, _, focus, _, focus_visual| {
            let candidate = Some(NavigationCandidate::MathText {
                focus,
                visual_affinity: focus_visual,
                generation: generation.clone(),
            });
            session
                .run(Command::EditorKey(vertical_navigation_key(
                    line_delta, candidate, extend,
                )))
                .map_err(|error| format!("selection.math_text: {error}"))
        },
    ))
}

fn move_math_tree_vertical(
    runtime: &mut DocumentViewRuntime,
    line_delta: isize,
    extend: bool,
) -> Option<Result<String, String>> {
    let (target, position) = runtime.session.math_tree_input_context()?;
    let preferred_visual_x = runtime.preferred_visual_x_pt;
    let key = |candidate| vertical_navigation_key(line_delta, candidate, extend);
    if extend {
        // Tree 选区只保存一个序列路径，无法表达跨操作数的扩展范围。
        return Some(dispatch_navigation_key(
            runtime,
            key(None),
            "navigation.math_tree",
        ));
    }
    let snapshot = match runtime.current_snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => return Some(Err(error)),
    };
    let current_geometry = snapshot.math_tree_caret_geometry(&target, &position);
    let preferred_x =
        preferred_visual_x.or_else(|| current_geometry.map(|caret| caret.x_pt as f32));
    let moved =
        snapshot.move_math_tree_vertical_target(&target, &position, line_delta, preferred_x);
    let generation = match navigation_generation(runtime) {
        Ok(generation) => generation,
        Err(error) => return Some(Err(error)),
    };
    let candidate = match moved {
        Some(caret_engine::MathTreeVerticalTarget::Structure(hit)) => {
            Some(NavigationCandidate::MathTreeHit { hit, generation })
        }
        Some(caret_engine::MathTreeVerticalTarget::Text(hit)) => {
            Some(NavigationCandidate::MathTextHit { hit, generation })
        }
        None => None,
    };
    let result = dispatch_navigation_key(runtime, key(candidate), "navigation.math_tree");
    if result.is_ok() && preferred_x.is_some() {
        runtime.preferred_visual_x_pt = preferred_x;
    }
    Some(result)
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
            return Some(dispatch_navigation_key(
                runtime,
                if end {
                    EditorKey::End {
                        candidate: None,
                        extend,
                    }
                } else {
                    EditorKey::Home {
                        candidate: None,
                        extend,
                    }
                },
                "navigation.math_text",
            ));
        }
        Err(error) => return Some(Err(error)),
    };
    let generation = match navigation_generation(runtime) {
        Ok(generation) => generation,
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
        |session, _, focus, _, focus_visual| {
            let candidate = Some(NavigationCandidate::MathText {
                focus,
                visual_affinity: focus_visual,
                generation: generation.clone(),
            });
            session
                .run(Command::EditorKey(if end {
                    EditorKey::End { candidate, extend }
                } else {
                    EditorKey::Home { candidate, extend }
                }))
                .map_err(|error| format!("selection.math_text: {error}"))
        },
    ))
}

/// 把方向、布局候选和扩展状态统一映射到编辑器 Up/Down 键。
fn vertical_navigation_key(
    line_delta: isize,
    candidate: Option<NavigationCandidate>,
    extend: bool,
) -> EditorKey {
    if line_delta < 0 {
        EditorKey::UpArrow {
            candidate,
            extend,
            structure: false,
        }
    } else {
        EditorKey::DownArrow {
            candidate,
            extend,
            structure: false,
        }
    }
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
    ) -> Result<Effect, String>,
) -> Result<String, String> {
    let (anchor, anchor_visual) = if state.extend {
        (current_anchor, state.current_anchor_visual)
    } else {
        (focus.clone(), state.focus_visual)
    };
    let effect = commit(
        &mut runtime.session,
        anchor,
        focus,
        anchor_visual,
        state.focus_visual,
    )?;
    if effect.selection_changed {
        runtime.preferred_visual_x_pt = state.preferred_x;
    }
    if effect.document_changed {
        return runtime.layout_snapshot_json();
    }
    runtime.interaction_state_json()
}

fn visual_caret_key(
    snapshot: &DocumentLayoutSnapshot,
    position: &Pos,
    affinity: VisualAffinity,
) -> Option<(usize, f64, f64)> {
    let caret = snapshot.caret_geometry(position, affinity)?;
    Some((caret.page_index, caret.y0_pt, caret.x_pt))
}

fn pick_visual_anchor(forward: bool, anchor: (usize, f64, f64), focus: (usize, f64, f64)) -> bool {
    if forward {
        anchor > focus
    } else {
        anchor < focus
    }
}

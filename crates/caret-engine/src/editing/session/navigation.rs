//! 提交光标与选区导航，保持逻辑环境亲和性和选区校验规则。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::compare_positions;
use super::super::{
    Affinity, Direction, EditError, Effect, HostCaretKind, HostCaretPosition, HostCaretTransition,
    Pos, Selection,
};
use super::Session;
use caret_layout::VisualAffinity;

#[cfg(test)]
#[path = "navigation/tests.rs"]
mod tests;

impl Session {
    pub(super) fn move_caret(&mut self, direction: Direction) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        if self.selection.anchor != self.selection.focus {
            let ordering = compare_positions(
                &self.paragraph_index,
                &self.selection.anchor,
                &self.selection.focus,
            )?;
            let (left, right) = if ordering.is_le() {
                (&self.selection.anchor, &self.selection.focus)
            } else {
                (&self.selection.focus, &self.selection.anchor)
            };
            let pos = match direction {
                Direction::Backward => left.clone(),
                Direction::Forward => right.clone(),
            };
            self.selection = Selection {
                anchor: pos.clone(),
                focus: pos,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if let Some(effect) = self.move_empty_inline_host_boundary(direction)? {
            return Ok(effect);
        }
        if matches!(
            self.host_caret_position.as_ref(),
            Some(
                HostCaretPosition::Before {
                    kind: HostCaretKind::Inline,
                    ..
                } | HostCaretPosition::After {
                    kind: HostCaretKind::Inline,
                    ..
                }
            )
        ) {
            if self.enter_adjacent_math_if_present(direction)? {
                return Ok(Effect {
                    selection_changed: true,
                    ..Effect::default()
                });
            }
            let pos = self.selection.focus.clone();
            let map = self.text_map_for(&pos)?;
            let next_offset = match direction {
                Direction::Backward => map.previous_boundary(pos.offset),
                Direction::Forward => map.next_boundary(pos.offset),
            };
            if next_offset == pos.offset {
                return Ok(Effect::default());
            }
        }
        if let Some(effect) = self.move_from_math_block_boundary_stop(direction)? {
            return Ok(effect);
        }
        let pos = self.selection.focus.clone();
        let map = self.text_map_for(&pos)?;
        map.validate_pos(&pos)?;
        let forward = direction == Direction::Forward;
        let boundary_transition =
            map.horizontal_environment_transition(pos.offset, pos.affinity, forward);
        if forward
            && pos.affinity == Affinity::Upstream
            && matches!(
                boundary_transition,
                Some(HostCaretTransition::EnterStart | HostCaretTransition::LeaveAfter)
            )
        {
            let target = Pos {
                affinity: Affinity::Downstream,
                ..pos
            };
            self.selection = Selection {
                anchor: target.clone(),
                focus: target,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        if !forward
            && pos.affinity == Affinity::Downstream
            && matches!(
                boundary_transition,
                Some(HostCaretTransition::EnterEnd | HostCaretTransition::LeaveBefore)
            )
        {
            let target = Pos {
                affinity: Affinity::Upstream,
                ..pos
            };
            self.selection = Selection {
                anchor: target.clone(),
                focus: target,
            };
            return Ok(Effect {
                selection_changed: true,
                ..Effect::default()
            });
        }
        let offset = match direction {
            Direction::Backward => map.previous_boundary(pos.offset),
            Direction::Forward => map.next_boundary(pos.offset),
        };
        let affinity = if map.has_environment_boundary(offset) {
            match direction {
                Direction::Backward => Affinity::Downstream,
                Direction::Forward => Affinity::Upstream,
            }
        } else {
            pos.affinity
        };
        let target = Pos {
            offset,
            affinity,
            ..pos
        };
        let changed = target != self.selection.focus;
        self.selection = Selection {
            anchor: target.clone(),
            focus: target,
        };
        Ok(Effect {
            selection_changed: changed,
            ..Effect::default()
        })
    }

    /// 处理零长度 InlineEnvironment 的唯一内部插入点及两侧退出。
    pub(super) fn move_empty_inline_host_boundary(
        &mut self,
        direction: Direction,
    ) -> Result<Option<Effect>, EditError> {
        let focus = self.selection.focus.clone();
        let host_position = self.host_caret_position.clone();
        let map = self.text_map_for(&focus)?;
        let forward = direction == Direction::Forward;
        if let Some(position) = host_position {
            match position {
                HostCaretPosition::Inside {
                    host_id,
                    kind: HostCaretKind::Inline,
                    local,
                } if local == focus && map.is_empty_inline_environment(focus.offset, &host_id) => {
                    let transition = HostCaretPosition::Inside {
                        host_id: host_id.clone(),
                        kind: HostCaretKind::Inline,
                        local: focus.clone(),
                    }
                    .horizontal_transition(forward, true, true);
                    let (outside, affinity) = match transition {
                        HostCaretTransition::LeaveBefore => {
                            (Direction::Backward, Affinity::Upstream)
                        }
                        HostCaretTransition::LeaveAfter => {
                            (Direction::Forward, Affinity::Downstream)
                        }
                        _ => return Ok(None),
                    };
                    self.host_caret_position = Some(HostCaretPosition::outside_with_kind(
                        host_id,
                        outside,
                        HostCaretKind::Inline,
                    ));
                    self.selection.focus.affinity = affinity;
                    self.selection.anchor = self.selection.focus.clone();
                    self.selection_visual_affinity = match affinity {
                        Affinity::Upstream => VisualAffinity::Upstream,
                        Affinity::Downstream => VisualAffinity::Downstream,
                    };
                    return Ok(Some(Effect {
                        selection_changed: true,
                        ..Effect::default()
                    }));
                }
                HostCaretPosition::Before {
                    host_id,
                    kind: HostCaretKind::Inline,
                } if map.is_empty_inline_environment(focus.offset, &host_id) => {
                    let transition = HostCaretPosition::<Pos>::Before {
                        host_id: host_id.clone(),
                        kind: HostCaretKind::Inline,
                    }
                    .horizontal_transition(forward, true, true);
                    let toward_host = matches!(transition, HostCaretTransition::EnterStart);
                    let adjacent = if toward_host {
                        None
                    } else {
                        map.adjacent_empty_inline_environment(focus.offset, &host_id, false)
                    };
                    let target_id = if toward_host { Some(host_id) } else { adjacent };
                    if let Some(host_id) = target_id {
                        self.host_caret_position = Some(HostCaretPosition::Inside {
                            host_id,
                            kind: HostCaretKind::Inline,
                            local: focus,
                        });
                        return Ok(Some(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        }));
                    }
                    return Ok(None);
                }
                HostCaretPosition::After {
                    host_id,
                    kind: HostCaretKind::Inline,
                } if map.is_empty_inline_environment(focus.offset, &host_id) => {
                    let transition = HostCaretPosition::<Pos>::After {
                        host_id: host_id.clone(),
                        kind: HostCaretKind::Inline,
                    }
                    .horizontal_transition(forward, true, true);
                    let toward_host = matches!(transition, HostCaretTransition::EnterEnd);
                    let adjacent = if toward_host {
                        None
                    } else {
                        map.adjacent_empty_inline_environment(focus.offset, &host_id, true)
                    };
                    let target_id = if toward_host { Some(host_id) } else { adjacent };
                    if let Some(host_id) = target_id {
                        self.host_caret_position = Some(HostCaretPosition::Inside {
                            host_id,
                            kind: HostCaretKind::Inline,
                            local: focus,
                        });
                        return Ok(Some(Effect {
                            selection_changed: true,
                            ..Effect::default()
                        }));
                    }
                    return Ok(None);
                }
                HostCaretPosition::Inside { .. } => return Ok(None),
                HostCaretPosition::Before { .. } | HostCaretPosition::After { .. } => {
                    return Ok(None);
                }
            }
        }

        let empty_environment = map.empty_inline_environment_stack_at(focus.offset, focus.affinity);
        let Some(host_id) = empty_environment.last().and_then(|span| span.id.clone()) else {
            return Ok(None);
        };
        let outside_position = match focus.affinity {
            Affinity::Upstream => HostCaretPosition::<Pos>::Before {
                host_id: host_id.clone(),
                kind: HostCaretKind::Inline,
            },
            Affinity::Downstream => HostCaretPosition::<Pos>::After {
                host_id: host_id.clone(),
                kind: HostCaretKind::Inline,
            },
        };
        let transition = outside_position.horizontal_transition(forward, true, true);
        let entering = matches!(
            transition,
            HostCaretTransition::EnterStart | HostCaretTransition::EnterEnd
        );
        if !entering {
            return Ok(None);
        }
        self.host_caret_position = Some(HostCaretPosition::Inside {
            host_id,
            kind: HostCaretKind::Inline,
            local: focus,
        });
        Ok(Some(Effect {
            selection_changed: true,
            ..Effect::default()
        }))
    }
}

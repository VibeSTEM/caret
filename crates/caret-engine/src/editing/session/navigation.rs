//! 提交光标与选区导航，保持逻辑环境亲和性和选区校验规则。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::super::paragraph_ops::compare_positions;
use super::super::{Affinity, Direction, EditError, Effect, Pos, Selection};
use super::Session;

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
        let pos = self.selection.focus.clone();
        let map = self.text_map_for(&pos)?;
        map.validate_pos(&pos)?;
        let active_environment = map
            .inline_environment_at(pos.offset, pos.affinity)
            .is_some();
        if direction == Direction::Forward
            && pos.affinity == Affinity::Upstream
            && (if active_environment {
                map.has_environment_end(pos.offset)
            } else {
                map.has_underlined_run_end(pos.offset)
            })
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
        if direction == Direction::Backward
            && pos.affinity == Affinity::Downstream
            && (if active_environment {
                map.has_environment_start(pos.offset)
            } else {
                map.has_underlined_run_start(pos.offset)
            })
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
        let boundary_affinity = match direction {
            Direction::Backward => Affinity::Downstream,
            Direction::Forward => Affinity::Upstream,
        };
        let at_active_scope_boundary = map.inline_boundary_source_at(offset).is_some();
        let affinity = if at_active_scope_boundary {
            boundary_affinity
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
}

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
        if direction == Direction::Forward
            && pos.affinity == Affinity::Upstream
            && map.has_environment_end(pos.offset)
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
            && map.has_environment_start(pos.offset)
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
}

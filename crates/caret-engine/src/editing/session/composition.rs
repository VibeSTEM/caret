//! 管理 IME 组字的更新、幂等提交与取消，提交正文仍复用文本编辑事务。

use super::super::{EditError, Effect, InputComposition};
use super::Session;

impl Session {
    pub(super) fn update_composition(
        &mut self,
        id: u64,
        preedit: String,
    ) -> Result<Effect, EditError> {
        if self.last_committed_composition == Some(id) {
            return Err(EditError::InvalidCompositionId);
        }
        if self
            .composition
            .as_ref()
            .is_some_and(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        let next = Some(InputComposition { id, preedit });
        let changed = self.composition != next;
        self.composition = next;
        Ok(Effect {
            composition_changed: changed,
            ..Effect::default()
        })
    }

    pub(super) fn commit_composition(&mut self, id: u64, text: &str) -> Result<Effect, EditError> {
        if self.last_committed_composition == Some(id) {
            return Ok(Effect::default());
        }
        if self
            .composition
            .as_ref()
            .is_none_or(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        let prior = self.composition.take();
        match self.insert_text(text) {
            Ok(mut effect) => {
                self.last_committed_composition = Some(id);
                effect.composition_changed = true;
                Ok(effect)
            }
            Err(error) => {
                self.composition = prior;
                Err(error)
            }
        }
    }

    pub(super) fn cancel_composition(&mut self, id: u64) -> Result<Effect, EditError> {
        if self
            .composition
            .as_ref()
            .is_none_or(|current| current.id != id)
        {
            return Err(EditError::InvalidCompositionId);
        }
        self.composition = None;
        Ok(Effect {
            composition_changed: true,
            ..Effect::default()
        })
    }
}

//! 执行段落级格式命令并更新段落覆盖值。

use super::Session;
use crate::editing::{EditError, Effect};

impl Session {
    pub(super) fn set_paragraph_first_line_indent_chars(
        &mut self,
        chars: u16,
    ) -> Result<Effect, EditError> {
        if self.composition.is_some() {
            return Err(EditError::CompositionPending);
        }
        let position = self.selection.focus.clone();
        let current = self
            .paragraph_for(&position)?
            .layout
            .as_ref()
            .and_then(|layout| layout.first_line_indent_chars);
        if current == Some(chars) {
            return Ok(Effect::default());
        }
        let paragraph = self.paragraph_for_mut(&position)?;
        paragraph
            .layout
            .get_or_insert_default()
            .first_line_indent_chars = Some(chars);
        Ok(Effect {
            document_changed: true,
            changed_paragraph_id: Some(position.paragraph_id),
            ..Effect::default()
        })
    }
}

//! 提供段落与大纲标题的结构语义。

use caret_doc::Paragraph;

use crate::StructureKind;

pub(crate) struct ParagraphAtom<'a> {
    paragraph: &'a Paragraph,
}

impl<'a> ParagraphAtom<'a> {
    pub(crate) fn new(paragraph: &'a Paragraph) -> Self {
        Self { paragraph }
    }

    pub(crate) fn id(&self) -> &'a caret_doc::NodeId {
        &self.paragraph.id
    }

    pub(crate) fn kind(&self) -> StructureKind {
        outline_level(self.paragraph).map_or(StructureKind::Paragraph, |level| {
            StructureKind::Heading { level }
        })
    }

    pub(crate) fn outline_level(&self) -> Option<u8> {
        outline_level(self.paragraph)
    }
}

fn outline_level(paragraph: &Paragraph) -> Option<u8> {
    let level = paragraph.props.get("outlineLevel")?.as_u64()?;
    u8::try_from(level)
        .ok()
        .filter(|level| (1..=9).contains(level))
}

//! 提供块环境参与结构装配所需的语义字段。

use caret_doc::{BlockEnvironment, BlockEnvironmentKind};

use crate::StructureKind;

pub(crate) struct BlockEnvironmentAtom<'a> {
    environment: &'a BlockEnvironment,
}

impl<'a> BlockEnvironmentAtom<'a> {
    pub(crate) fn new(environment: &'a BlockEnvironment) -> Self {
        Self { environment }
    }

    pub(crate) fn id(&self) -> &'a caret_doc::NodeId {
        &self.environment.id
    }

    pub(crate) fn kind(&self) -> StructureKind {
        match self.environment.kind {
            BlockEnvironmentKind::Section => StructureKind::Section,
            BlockEnvironmentKind::Custom => StructureKind::Custom,
        }
    }

    pub(crate) fn title(&self) -> Option<&'a caret_doc::Paragraph> {
        self.environment.title.as_ref()
    }

    pub(crate) fn children(&self) -> &'a [caret_doc::Block] {
        &self.environment.children
    }
}

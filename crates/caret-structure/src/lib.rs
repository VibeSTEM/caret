//! 提供与文档树分离的结构索引、结构范围和导航查询。

mod atoms;
mod index;

pub use index::{
    IndexedStructure, StructureClass, StructureIndex, StructureKind, StructurePosition,
    StructureRange,
};

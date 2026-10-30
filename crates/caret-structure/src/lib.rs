//! 提供与文档树分离的结构索引、结构范围和导航查询。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

mod atoms;
mod index;

pub use index::{
    IndexedStructure, StructureClass, StructureIndex, StructureKind, StructurePosition,
    StructureRange,
};

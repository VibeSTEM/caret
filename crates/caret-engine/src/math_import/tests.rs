//! 验证解析结果到持久化文档数学节点的转换。

use super::{ImportedMath, MathImportError, import_parsed_math};
use caret_doc::{
    Block, CdocFile, Document, InlineNode, MATH_TREE_SCHEMA_VERSION, MathNode, MathNodeKind,
    OpenedDocument, open,
};
use caret_math::{MathInputKind, parse};
use std::collections::HashSet;

#[test]
fn converts_inline_math_to_caret_owned_schema_with_unique_ids() {
    let source = r"\frac{x}{1}";
    let Ok(parsed) = parse(source, MathInputKind::Inline) else {
        panic!("inline formula should parse");
    };
    let Ok(ImportedMath::Inline(inline)) = import_parsed_math(&parsed) else {
        panic!("inline formula should import");
    };
    assert_eq!(inline.math_tree.schema_version, MATH_TREE_SCHEMA_VERSION);
    assert_eq!(inline.math_tree.source, source);
    assert_eq!(inline.math_tree.root.kind, MathNodeKind::Fraction);
    assert_eq!(inline.math_tree.root.children.len(), 2);
    assert_eq!(
        inline
            .math_tree
            .root
            .attributes
            .get("style")
            .map(String::as_str),
        Some("frac")
    );

    let mut ids = HashSet::new();
    assert!(ids.insert(inline.id.as_str()));
    collect_unique_node_ids(&inline.math_tree.root, &mut ids);
}

#[test]
fn converts_display_equation_and_retains_parser_specific_structure() {
    let source = r"\sum_{i=0}^n i";
    let Ok(parsed) = parse(source, MathInputKind::Equation) else {
        panic!("equation should parse");
    };
    let Ok(ImportedMath::Equation(equation)) = import_parsed_math(&parsed) else {
        panic!("equation should import");
    };
    assert_eq!(equation.math_tree.source, source);
    assert_eq!(equation.math_tree.schema_version, MATH_TREE_SCHEMA_VERSION);
    assert_eq!(equation.math_tree.root.kind, MathNodeKind::Row);
    let sum = &equation.math_tree.root.children[0];
    assert_eq!(sum.kind, MathNodeKind::Sum);
}

#[test]
fn converts_align_rows_and_keeps_complete_source_at_the_container() {
    let source = r"\begin{aligned}a &= b \\ c &= d\end{aligned}";
    let Ok(parsed) = parse(source, MathInputKind::Align) else {
        panic!("align should parse");
    };
    let Ok(ImportedMath::Align(align)) = import_parsed_math(&parsed) else {
        panic!("align should import");
    };
    assert_eq!(align.source, source);
    assert_eq!(align.rows.len(), 2);
    assert!(align.rows.iter().all(|row| row.cells.len() == 2));
    assert!(
        align.rows.iter().flat_map(|row| &row.cells).all(|cell| {
            cell.schema_version == MATH_TREE_SCHEMA_VERSION && cell.source.is_empty()
        })
    );

    let mut ids = HashSet::new();
    assert!(ids.insert(align.id.as_str()));
    for row in &align.rows {
        assert!(ids.insert(row.id.as_str()));
        for cell in &row.cells {
            collect_unique_node_ids(&cell.root, &mut ids);
        }
    }
}

#[test]
fn preserves_source_when_align_schema_cannot_represent_environment_rows() {
    let source = r"\begin{aligned}a &= b \\ \intertext{because} \\ c &= d\end{aligned}";
    let Ok(parsed) = parse(source, MathInputKind::Align) else {
        panic!("align with intertext should parse");
    };
    let Err(MathImportError::UnsupportedAlignRow {
        row_kind,
        source: preserved,
        source_range,
    }) = import_parsed_math(&parsed)
    else {
        panic!("align intertext row should be rejected");
    };
    assert_eq!(row_kind, caret_math::MathSyntaxKind::Intertext);
    assert_eq!(preserved, source);
    assert_eq!(source_range, 0..source.len());
    assert_eq!(parsed.source(), source);
}

#[test]
fn coalesces_contiguous_plain_atoms_before_allocating_document_nodes() {
    let source = "xxxxxxxx";
    let Ok(parsed) = parse(source, MathInputKind::Inline) else {
        panic!("plain symbol run should parse");
    };
    let Ok(ImportedMath::Inline(inline)) = import_parsed_math(&parsed) else {
        panic!("plain symbol run should import");
    };
    assert_eq!(inline.math_tree.root.kind, MathNodeKind::Row);
    assert_eq!(inline.math_tree.root.children.len(), 1);
    assert_eq!(inline.math_tree.root.children[0].kind, MathNodeKind::Symbol);
    assert_eq!(
        inline.math_tree.root.children[0].text.as_deref(),
        Some(source)
    );
}

#[test]
fn coalescing_preserves_matrix_cell_boundaries_and_non_ordinary_atoms() {
    let Ok(parsed) = parse(r"\begin{matrix}a & b\end{matrix}", MathInputKind::Inline) else {
        panic!("matrix should parse");
    };
    let Ok(ImportedMath::Inline(matrix)) = import_parsed_math(&parsed) else {
        panic!("matrix should import");
    };
    assert_eq!(matrix.math_tree.root.kind, MathNodeKind::Matrix);
    assert_eq!(matrix.math_tree.root.children.len(), 1);
    assert_eq!(matrix.math_tree.root.children[0].kind, MathNodeKind::Row);
    assert_eq!(matrix.math_tree.root.children[0].children.len(), 2);

    let Ok(parsed) = parse("++", MathInputKind::Inline) else {
        panic!("binary symbols should parse");
    };
    let Ok(ImportedMath::Inline(binary_symbols)) = import_parsed_math(&parsed) else {
        panic!("binary symbols should import");
    };
    assert_eq!(binary_symbols.math_tree.root.children.len(), 2);
}

#[test]
fn imported_math_survives_cdoc_round_trip_with_declared_syntax_capability() {
    let Ok(parsed_inline) = parse(r"x^2", MathInputKind::Inline) else {
        panic!("inline formula should parse");
    };
    let Ok(ImportedMath::Inline(inline)) = import_parsed_math(&parsed_inline) else {
        panic!("inline formula should import");
    };
    let Ok(parsed_equation) = parse(r"\frac{a}{b}", MathInputKind::Equation) else {
        panic!("equation should parse");
    };
    let Ok(ImportedMath::Equation(equation)) = import_parsed_math(&parsed_equation) else {
        panic!("equation should import");
    };
    let Ok(parsed_align) = parse(
        r"\begin{aligned}a &= b \\ c &= d\end{aligned}",
        MathInputKind::Align,
    ) else {
        panic!("align should parse");
    };
    let Ok(ImportedMath::Align(align)) = import_parsed_math(&parsed_align) else {
        panic!("align should import");
    };

    let mut document = Document::blank();
    let Block::Paragraph(paragraph) = &mut document.body[0] else {
        panic!("blank document should start with a paragraph");
    };
    paragraph.inlines.push(InlineNode::Math(inline));
    document.body.push(Block::Equation(equation));
    document.body.push(Block::Align(align));
    let Ok(file) = CdocFile::new(document) else {
        panic!("imported formulas should satisfy document validation");
    };
    assert!(
        file.required_features()
            .iter()
            .any(|feature| feature == "caret.math.syntax.v1")
    );
    let Ok(bytes) = file.to_bytes() else {
        panic!("imported formulas should serialize");
    };
    let Ok(OpenedDocument::Editable(reopened)) = open(&bytes) else {
        panic!("imported formulas should reopen as editable");
    };
    assert_eq!(reopened.document(), file.document());
}

fn collect_unique_node_ids<'a>(node: &'a MathNode, ids: &mut HashSet<&'a str>) {
    assert!(ids.insert(node.id.as_str()), "duplicate math node ID");
    for child in &node.children {
        collect_unique_node_ids(child, ids);
    }
}

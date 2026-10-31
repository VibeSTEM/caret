//! 将后端无关的公式语法树映射到 latex-rust 的布局 AST。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use super::MathLayoutError;
use crate::{MathSyntaxKind, MathSyntaxNode};
use latex_rust::{
    AccentKind, AtomKind, ColSpec, Color, DelimSize, Delimiter, Dim, EnvRow, FracStyle,
    IntegralKind, MathNode, MatrixStyle, PhantomKind, SpaceKind, TextItem, TextRun, TextStyle,
    parse_color_spec,
};
use std::collections::HashMap;

pub(super) fn to_latex_node(
    node: &MathSyntaxNode,
    provenance: &mut HashMap<usize, String>,
) -> Result<MathNode, MathLayoutError> {
    let attr = |key: &str| node.attributes.get(key).map(String::as_str);
    let required = |key: &str| {
        attr(key).ok_or_else(|| MathLayoutError::UnsupportedTree(format!("missing {key}")))
    };
    let child = |index: usize, map: &mut HashMap<usize, String>| {
        let value = node.children.get(index).ok_or_else(|| {
            MathLayoutError::UnsupportedTree(format!("missing child {index} for {:?}", node.kind))
        })?;
        boxed_child(value, map)
    };
    let result = match node.kind {
        MathSyntaxKind::Row => MathNode::Row(vector_children(&node.children, provenance)?),
        MathSyntaxKind::Symbol => {
            let text = node
                .text
                .as_deref()
                .ok_or_else(|| MathLayoutError::UnsupportedTree("symbol has no text".into()))?;
            if let Some(class) = attr("atomClass") {
                let atom_kind = atom_kind(class)?;
                let mut atoms = text
                    .chars()
                    .map(|character| MathNode::Atom(character, atom_kind))
                    .collect::<Vec<_>>();
                if atoms.len() == 1 {
                    atoms
                        .pop()
                        .ok_or_else(|| MathLayoutError::UnsupportedTree("empty symbol".into()))?
                } else {
                    let mut row = MathNode::Row(atoms);
                    if let MathNode::Row(items) = &row {
                        for item in items {
                            if let Some(id) = node.provenance.as_ref() {
                                provenance.insert(item as *const MathNode as usize, id.clone());
                            }
                        }
                    }
                    std::mem::replace(&mut row, MathNode::Row(Vec::new()))
                }
            } else {
                MathNode::Symbol(text.to_owned())
            }
        }
        MathSyntaxKind::Operator => MathNode::Operator(
            node.text
                .clone()
                .ok_or_else(|| MathLayoutError::UnsupportedTree("operator has no name".into()))?,
            required("limits")? == "true",
        ),
        MathSyntaxKind::Fraction => MathNode::Fraction(
            fraction_style(required("style")?)?,
            child(0, provenance)?,
            child(1, provenance)?,
        ),
        MathSyntaxKind::Radical => MathNode::Radical(
            optional_child(node.children.first(), provenance)?,
            child(1, provenance)?,
        ),
        MathSyntaxKind::Superscript => {
            MathNode::Superscript(child(0, provenance)?, child(1, provenance)?)
        }
        MathSyntaxKind::Subscript => {
            MathNode::Subscript(child(0, provenance)?, child(1, provenance)?)
        }
        MathSyntaxKind::SubSup => MathNode::SubSup(
            child(0, provenance)?,
            child(1, provenance)?,
            child(2, provenance)?,
        ),
        MathSyntaxKind::Delimited => MathNode::Delimited(
            delimiter(required("left")?)?,
            child(0, provenance)?,
            delimiter(required("right")?)?,
        ),
        MathSyntaxKind::SizedDelimiter => MathNode::SizedDelim(
            delimiter(node.text.as_deref().ok_or_else(|| {
                MathLayoutError::UnsupportedTree("sized delimiter has no value".into())
            })?)?,
            delimiter_size(required("size")?)?,
            atom_kind(required("atomClass")?)?,
        ),
        MathSyntaxKind::Matrix => {
            let style = matrix_style(required("style")?)?;
            let columns = required("columns")?
                .chars()
                .map(col_spec)
                .collect::<Result<Vec<_>, _>>()?;
            let mut rows = Vec::with_capacity(node.children.len());
            for row in &node.children {
                match row.kind {
                    MathSyntaxKind::Row => {
                        rows.push(EnvRow::cells(vector_children(&row.children, provenance)?))
                    }
                    MathSyntaxKind::Hline => rows.push(EnvRow::Hline),
                    MathSyntaxKind::Intertext => rows.push(EnvRow::Intertext(boxed_child(
                        row.children.first().ok_or_else(|| {
                            MathLayoutError::UnsupportedTree("intertext has no body".into())
                        })?,
                        provenance,
                    )?)),
                    _ => return Err(MathLayoutError::UnsupportedTree("matrix row kind".into())),
                }
            }
            MathNode::Matrix(style, columns, rows)
        }
        MathSyntaxKind::Substack => {
            MathNode::Substack(vector_children(&node.children, provenance)?)
        }
        MathSyntaxKind::Intertext => MathNode::Intertext(child(0, provenance)?),
        MathSyntaxKind::Hline => MathNode::Hline,
        MathSyntaxKind::Sum => MathNode::Sum(
            optional_child(node.children.first(), provenance)?,
            optional_child(node.children.get(1), provenance)?,
        ),
        MathSyntaxKind::Integral => MathNode::Integral(
            integral_kind(required("integralKind")?)?,
            optional_child(node.children.first(), provenance)?,
            optional_child(node.children.get(1), provenance)?,
        ),
        MathSyntaxKind::Product => MathNode::Product(
            optional_child(node.children.first(), provenance)?,
            optional_child(node.children.get(1), provenance)?,
        ),
        MathSyntaxKind::Limit => {
            MathNode::Limit(optional_child(node.children.first(), provenance)?)
        }
        MathSyntaxKind::OverUnder => MathNode::OverUnder(
            child(0, provenance)?,
            optional_child(node.children.get(1), provenance)?,
            optional_child(node.children.get(2), provenance)?,
        ),
        MathSyntaxKind::Accent => {
            MathNode::Accent(child(0, provenance)?, accent(required("accent")?)?)
        }
        MathSyntaxKind::CancelTo => {
            MathNode::CancelTo(child(0, provenance)?, child(1, provenance)?)
        }
        MathSyntaxKind::Text => MathNode::Text(
            text_run(&node.children, provenance)?,
            text_style(required("style")?)?,
        ),
        MathSyntaxKind::Space => space_node(node)?,
        MathSyntaxKind::Color => MathNode::Color(color(required("color")?)?, child(0, provenance)?),
        MathSyntaxKind::TextColor => {
            MathNode::TextColor(color(required("color")?)?, child(0, provenance)?)
        }
        MathSyntaxKind::ColorBox => {
            MathNode::ColorBox(color(required("color")?)?, child(0, provenance)?)
        }
        MathSyntaxKind::FColorBox => MathNode::FColorBox(
            color(required("borderColor")?)?,
            color(required("fillColor")?)?,
            child(0, provenance)?,
        ),
        MathSyntaxKind::Strut => MathNode::Strut(
            exact_dimension(node, "height")?,
            exact_dimension(node, "depth")?,
        ),
        MathSyntaxKind::Phantom => MathNode::Phantom(
            phantom_kind(required("phantomKind")?)?,
            child(0, provenance)?,
        ),
        MathSyntaxKind::Placeholder => MathNode::Row(Vec::new()),
        MathSyntaxKind::TextContent
        | MathSyntaxKind::TextSpace
        | MathSyntaxKind::TextSymbol
        | MathSyntaxKind::TextStyle => {
            return Err(MathLayoutError::UnsupportedTree(
                "text token outside a text run".into(),
            ));
        }
    };
    Ok(result)
}

fn boxed_child(
    node: &MathSyntaxNode,
    provenance: &mut HashMap<usize, String>,
) -> Result<Box<MathNode>, MathLayoutError> {
    let child = Box::new(to_latex_node(node, provenance)?);
    register_provenance(&child, node, provenance);
    Ok(child)
}

fn vector_children(
    nodes: &[MathSyntaxNode],
    provenance: &mut HashMap<usize, String>,
) -> Result<Vec<MathNode>, MathLayoutError> {
    let mut output = Vec::with_capacity(nodes.len());
    for node in nodes {
        output.push(to_latex_node(node, provenance)?);
        if let Some(child) = output.last() {
            register_provenance(child, node, provenance);
        }
    }
    Ok(output)
}

pub(super) fn register_provenance(
    node: &MathNode,
    syntax: &MathSyntaxNode,
    map: &mut HashMap<usize, String>,
) {
    if let Some(id) = &syntax.provenance {
        map.insert(node as *const MathNode as usize, id.clone());
    }
}

fn optional_child(
    node: Option<&MathSyntaxNode>,
    map: &mut HashMap<usize, String>,
) -> Result<Option<Box<MathNode>>, MathLayoutError> {
    node.map(|node| boxed_child(node, map)).transpose()
}

fn text_run(
    nodes: &[MathSyntaxNode],
    map: &mut HashMap<usize, String>,
) -> Result<TextRun, MathLayoutError> {
    let mut items = Vec::with_capacity(nodes.len());
    for node in nodes {
        match node.kind {
            MathSyntaxKind::TextContent => {
                items.push(TextItem::Str(node.text.clone().unwrap_or_default()))
            }
            MathSyntaxKind::TextSpace => items.push(match node.text.as_deref() {
                Some("space") => TextItem::Space,
                Some("control_space") => TextItem::ControlSpace,
                Some("no_break_space") => TextItem::NoBreakSpace,
                _ => {
                    return Err(MathLayoutError::UnsupportedTree(
                        "unknown text space".into(),
                    ));
                }
            }),
            MathSyntaxKind::TextSymbol => {
                items.push(TextItem::Symbol(match node.text.as_deref() {
                    Some("ldots") => "ldots",
                    Some("quad") => "quad",
                    Some("qquad") => "qquad",
                    _ => {
                        return Err(MathLayoutError::UnsupportedTree(
                            "unknown text symbol".into(),
                        ));
                    }
                }))
            }
            MathSyntaxKind::TextStyle => items.push(TextItem::Nested(
                text_style(
                    node.attributes
                        .get("style")
                        .map(String::as_str)
                        .ok_or_else(|| {
                            MathLayoutError::UnsupportedTree("text style missing".into())
                        })?,
                )?,
                text_run(&node.children, map)?,
            )),
            _ => items.push(TextItem::Math(boxed_child(node, map)?)),
        }
    }
    Ok(TextRun::new(items))
}

fn atom_kind(value: &str) -> Result<AtomKind, MathLayoutError> {
    match value {
        "ord" => Ok(AtomKind::Ord),
        "op" => Ok(AtomKind::Op),
        "bin" => Ok(AtomKind::Bin),
        "rel" => Ok(AtomKind::Rel),
        "open" => Ok(AtomKind::Open),
        "close" => Ok(AtomKind::Close),
        "punct" => Ok(AtomKind::Punct),
        "inner" => Ok(AtomKind::Inner),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown atom class {value}"
        ))),
    }
}
fn fraction_style(value: &str) -> Result<FracStyle, MathLayoutError> {
    match value {
        "frac" => Ok(FracStyle::Frac),
        "dfrac" => Ok(FracStyle::Dfrac),
        "tfrac" => Ok(FracStyle::Tfrac),
        "cfrac" => Ok(FracStyle::Cfrac),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown fraction style {value}"
        ))),
    }
}
fn matrix_style(value: &str) -> Result<MatrixStyle, MathLayoutError> {
    match value {
        "matrix" => Ok(MatrixStyle::Matrix),
        "pmatrix" => Ok(MatrixStyle::Pmatrix),
        "bmatrix" => Ok(MatrixStyle::Bmatrix),
        "vmatrix" => Ok(MatrixStyle::Vmatrix),
        "Vmatrix" => Ok(MatrixStyle::VVmatrix),
        "Bmatrix" => Ok(MatrixStyle::BBmatrix),
        "cases" => Ok(MatrixStyle::Cases),
        "array" => Ok(MatrixStyle::Array),
        "aligned" => Ok(MatrixStyle::Aligned),
        "split" => Ok(MatrixStyle::Split),
        "align" => Ok(MatrixStyle::Align),
        "gather" => Ok(MatrixStyle::Gather),
        "multline" => Ok(MatrixStyle::Multline),
        "equation" => Ok(MatrixStyle::Equation),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown matrix style {value}"
        ))),
    }
}
fn col_spec(value: char) -> Result<ColSpec, MathLayoutError> {
    match value {
        'l' => Ok(ColSpec::Left),
        'c' => Ok(ColSpec::Center),
        'r' => Ok(ColSpec::Right),
        '|' => Ok(ColSpec::VRule),
        _ => Err(MathLayoutError::UnsupportedTree(
            "unknown column specifier".into(),
        )),
    }
}
fn delimiter(value: &str) -> Result<Delimiter, MathLayoutError> {
    if value == "empty" {
        Ok(Delimiter::Empty)
    } else if let Some(value) = value.strip_prefix("char:") {
        let mut chars = value.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) => Ok(Delimiter::Char(c)),
            _ => Err(MathLayoutError::UnsupportedTree(
                "invalid character delimiter".into(),
            )),
        }
    } else if let Some(value) = value.strip_prefix("named:") {
        Ok(Delimiter::Named(value.to_owned()))
    } else {
        Err(MathLayoutError::UnsupportedTree(format!(
            "unknown delimiter {value}"
        )))
    }
}
fn delimiter_size(value: &str) -> Result<DelimSize, MathLayoutError> {
    match value {
        "big" => Ok(DelimSize::Big),
        "Big" => Ok(DelimSize::Big2),
        "bigg" => Ok(DelimSize::Bigg),
        "Bigg" => Ok(DelimSize::Bigg2),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown delimiter size {value}"
        ))),
    }
}
fn integral_kind(value: &str) -> Result<IntegralKind, MathLayoutError> {
    match value {
        "int" => Ok(IntegralKind::Int),
        "iint" => Ok(IntegralKind::Iint),
        "iiint" => Ok(IntegralKind::Iiint),
        "oint" => Ok(IntegralKind::Oint),
        "oiint" => Ok(IntegralKind::Oiint),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown integral {value}"
        ))),
    }
}
fn phantom_kind(value: &str) -> Result<PhantomKind, MathLayoutError> {
    match value {
        "phantom" => Ok(PhantomKind::Full),
        "vphantom" => Ok(PhantomKind::Vertical),
        "hphantom" => Ok(PhantomKind::Horizontal),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown phantom {value}"
        ))),
    }
}
fn text_style(value: &str) -> Result<TextStyle, MathLayoutError> {
    match value {
        "rm" => Ok(TextStyle::Rm),
        "bf" => Ok(TextStyle::Bf),
        "it" => Ok(TextStyle::It),
        "sf" => Ok(TextStyle::Sf),
        "tt" => Ok(TextStyle::Tt),
        "bb" => Ok(TextStyle::Bb),
        "cal" => Ok(TextStyle::Cal),
        "frak" => Ok(TextStyle::Frak),
        "scr" => Ok(TextStyle::Scr),
        "boldsymbol" => Ok(TextStyle::Boldsymbol),
        "pmb" => Ok(TextStyle::Pmb),
        "text" => Ok(TextStyle::Text),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown text style {value}"
        ))),
    }
}
fn accent(value: &str) -> Result<AccentKind, MathLayoutError> {
    match value {
        "hat" => Ok(AccentKind::Hat),
        "check" => Ok(AccentKind::Check),
        "breve" => Ok(AccentKind::Breve),
        "acute" => Ok(AccentKind::Acute),
        "grave" => Ok(AccentKind::Grave),
        "tilde" => Ok(AccentKind::Tilde),
        "bar" => Ok(AccentKind::Bar),
        "vec" => Ok(AccentKind::Vec),
        "dot" => Ok(AccentKind::Dot),
        "ddot" => Ok(AccentKind::Ddot),
        "dddot" => Ok(AccentKind::Dddot),
        "ddddot" => Ok(AccentKind::Ddddot),
        "widehat" => Ok(AccentKind::WideHat),
        "widetilde" => Ok(AccentKind::WideTilde),
        "overline" => Ok(AccentKind::Overline),
        "underline" => Ok(AccentKind::Underline),
        "overbrace" => Ok(AccentKind::Overbrace),
        "underbrace" => Ok(AccentKind::Underbrace),
        "overleftarrow" => Ok(AccentKind::Overleftarrow),
        "overrightarrow" => Ok(AccentKind::Overrightarrow),
        "overleftrightarrow" => Ok(AccentKind::Overleftrightarrow),
        "underleftarrow" => Ok(AccentKind::Underleftarrow),
        "underrightarrow" => Ok(AccentKind::Underrightarrow),
        "underleftrightarrow" => Ok(AccentKind::Underleftrightarrow),
        "cancel" => Ok(AccentKind::Cancel),
        "bcancel" => Ok(AccentKind::BCancel),
        "xcancel" => Ok(AccentKind::XCancel),
        "boxed" => Ok(AccentKind::Boxed),
        "ring" => Ok(AccentKind::Ring),
        "not" => Ok(AccentKind::Not),
        _ => Err(MathLayoutError::UnsupportedTree(format!(
            "unknown accent {value}"
        ))),
    }
}
fn color(value: &str) -> Result<Color, MathLayoutError> {
    parse_color_spec("HTML", value.trim_start_matches('#'), None)
        .map_err(|error| MathLayoutError::UnsupportedTree(error.to_string()))
}
fn exact_dimension(node: &MathSyntaxNode, prefix: &str) -> Result<Dim, MathLayoutError> {
    let numerator = node
        .attributes
        .get(&format!("{prefix}Numerator"))
        .and_then(|v| v.parse::<i64>().ok())
        .ok_or_else(|| MathLayoutError::UnsupportedTree(format!("invalid {prefix} numerator")))?;
    let denominator = node
        .attributes
        .get(&format!("{prefix}Denominator"))
        .and_then(|v| v.parse::<i64>().ok())
        .filter(|v| *v != 0)
        .ok_or_else(|| MathLayoutError::UnsupportedTree(format!("invalid {prefix} denominator")))?;
    Ok(Dim::ratio(numerator, denominator))
}
fn space_node(node: &MathSyntaxNode) -> Result<MathNode, MathLayoutError> {
    let kind = match node.attributes.get("spaceKind").map(String::as_str) {
        Some("thin") => SpaceKind::Thin,
        Some("medium") => SpaceKind::Medium,
        Some("thick") => SpaceKind::Thick,
        Some("negative_thin") => SpaceKind::NegThin,
        Some("quad") => SpaceKind::Quad,
        Some("qquad") => SpaceKind::Qquad,
        Some("control") => SpaceKind::ControlSpace,
        Some("hspace") => SpaceKind::Hspace(exact_dimension(node, "value")?),
        _ => {
            return Err(MathLayoutError::UnsupportedTree(
                "unknown math space".into(),
            ));
        }
    };
    Ok(MathNode::Space(kind))
}

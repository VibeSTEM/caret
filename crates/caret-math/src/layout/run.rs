//! 原子序列排版：TeX 二元运算符降级规则、原子间距与水平拼接。
//
// SPDX-License-Identifier: Apache-2.0
//
// 本文件移植自 Typst：
//   仓库：https://github.com/typst/typst
//   原路径：crates/typst-library/src/math/ir/process.rs（spacing）、
//           crates/typst-layout/src/math/run.rs（行拼接）
//   提交：9dfd3a08500b7896045f907433cf7b4b02434fad（v0.15.1）
//   版权：Copyright 2023 Typst Project Developers，Apache License 2.0
//   修改：类别改用 Caret 的 AtomClass（Op 对应 Typst 的 Large）；
//   用 TeXbook 规则 5、6 替代 Typst 的 Vary 类；显式空白不参与相邻判断；不支持换行。

use super::context::{Context, Style};
use super::frame::{Fragment, Frame, Point};
use crate::symbols::AtomClass;

#[derive(Default)]
struct RowMapping {
    positions: Vec<f64>,
    ranges: Vec<(usize, usize)>,
}

const THIN: f64 = 1.0 / 6.0;
const MEDIUM: f64 = 2.0 / 9.0;
const THICK: f64 = 5.0 / 18.0;

/// TeXbook 规则 5、6：处于不能作二元运算的位置时，Bin 降为 Ord。
fn demote_binaries(fragments: &mut [Fragment]) {
    let atoms: Vec<usize> = (0..fragments.len())
        .filter(|&i| !fragments[i].space)
        .collect();
    let mut previous: Option<AtomClass> = None;
    for (position, &index) in atoms.iter().enumerate() {
        if fragments[index].class == AtomClass::Bin {
            let left_blocks = matches!(
                previous,
                None | Some(
                    AtomClass::Bin
                        | AtomClass::Op
                        | AtomClass::Rel
                        | AtomClass::Open
                        | AtomClass::Punct
                )
            );
            let next = atoms.get(position + 1).map(|&n| fragments[n].class);
            let right_blocks = matches!(
                next,
                None | Some(AtomClass::Rel | AtomClass::Close | AtomClass::Punct)
            );
            if left_blocks || right_blocks {
                fragments[index].class = AtomClass::Ord;
            }
        }
        previous = Some(fragments[index].class);
    }
}

/// 两个相邻原子之间的间距（em）。
fn spacing(left: &Fragment, right: &Fragment, script: bool) -> f64 {
    use AtomClass::*;
    match (left.class, right.class) {
        (_, Punct) => 0.0,
        (Punct, _) if !script => THIN,
        (Open, _) | (_, Close) => 0.0,
        (Rel, Rel) => 0.0,
        (Rel, _) | (_, Rel) if !script => THICK,
        (Bin, _) | (_, Bin) if !script => MEDIUM,
        (Op, Open) => 0.0,
        (Op, _) | (_, Op) => THIN,
        _ => 0.0,
    }
}

/// 把片段序列拼成一行。单个片段原样返回，保留其字形、斜体校正等属性。
pub(crate) fn layout_row(ctx: &Context<'_>, style: Style, fragments: Vec<Fragment>) -> Fragment {
    let mut has_marker = Vec::with_capacity(fragments.len());
    let mut marker_targets = vec![None; fragments.len()];
    let mut marker_frames = Vec::with_capacity(fragments.len());
    let mut visible = Vec::with_capacity(fragments.len());
    let mut pending_markers = Vec::new();
    let mut marker_count = 0;
    for (original_index, fragment) in fragments.into_iter().enumerate() {
        if fragment.position_marker {
            marker_count += 1;
            has_marker.push(true);
            marker_frames.push(Some(fragment.frame));
            pending_markers.push(original_index);
        } else {
            has_marker.push(false);
            marker_frames.push(None);
            let visible_index = visible.len();
            for marker_index in pending_markers.drain(..) {
                marker_targets[marker_index] = Some(visible_index);
            }
            visible.push(fragment);
        }
    }
    if marker_count == 0 {
        return layout_visible_row(ctx, style, visible, false).0;
    }

    let (mut row, mapping) = layout_visible_row(ctx, style, visible, true);
    let mapping = mapping.unwrap_or_default();
    let mut source_items: Vec<_> = row.frame.items.drain(..).map(Some).collect();
    let mut ordered_items = Vec::with_capacity(source_items.len() + marker_count);
    let mut visible_index = 0;
    for (original_index, is_marker) in has_marker.into_iter().enumerate() {
        if is_marker {
            let x = marker_targets[original_index]
                .and_then(|index| mapping.positions.get(index).copied())
                .unwrap_or(row.width());
            let Some(marker) = marker_frames[original_index].take() else {
                continue;
            };
            let y = row.ascent() - marker.ascent();
            ordered_items.extend(
                marker
                    .items
                    .into_iter()
                    .map(|(point, item)| (Point::new(point.x + x, point.y + y), item)),
            );
        } else {
            let (start, end) = mapping.ranges[visible_index];
            for item in &mut source_items[start..end] {
                if let Some(item) = item.take() {
                    ordered_items.push(item);
                }
            }
            visible_index += 1;
        }
    }
    row.frame.items = ordered_items;
    row
}

fn layout_visible_row(
    ctx: &Context<'_>,
    style: Style,
    mut fragments: Vec<Fragment>,
    capture_positions: bool,
) -> (Fragment, Option<RowMapping>) {
    if fragments.len() == 1
        && let Some(single) = fragments.pop()
    {
        let range = (0, single.frame.items.len());
        return (
            single,
            capture_positions.then(|| RowMapping {
                positions: vec![0.0],
                ranges: vec![range],
            }),
        );
    }
    if fragments.is_empty() {
        return (
            Fragment::new(Frame::new(0.0, 0.0, 0.0), AtomClass::Ord),
            capture_positions.then(RowMapping::default),
        );
    }
    demote_binaries(&mut fragments);
    let em = ctx.font_size(style);
    let script = style.is_script();

    let mut gaps = vec![0.0; fragments.len()];
    let mut last: Option<usize> = None;
    for index in 0..fragments.len() {
        if fragments[index].space {
            continue;
        }
        if let Some(previous) = last {
            // 间距插在左原子之后。
            gaps[previous] = spacing(&fragments[previous], &fragments[index], script) * em;
        }
        last = Some(index);
    }

    let ascent = fragments.iter().map(Fragment::ascent).fold(0.0, f64::max);
    let descent = fragments.iter().map(Fragment::descent).fold(0.0, f64::max);
    let width: f64 = fragments.iter().map(Fragment::width).sum::<f64>() + gaps.iter().sum::<f64>();
    let mut frame = Frame::new(width.max(0.0), ascent + descent, ascent);
    let mut x = 0.0;
    let mut italics = 0.0;
    let mut mapping = capture_positions.then(|| RowMapping {
        positions: Vec::with_capacity(fragments.len()),
        ranges: Vec::with_capacity(fragments.len()),
    });
    for (fragment, gap) in fragments.into_iter().zip(gaps) {
        if let Some(mapping) = &mut mapping {
            mapping.positions.push(x);
        }
        let y = ascent - fragment.ascent();
        let advance = fragment.width();
        // 行末片段的斜体校正传给整行，供后续脚标使用。
        italics = if super::is_zero(gap) {
            fragment.italics_correction
        } else {
            0.0
        };
        let start = frame.items.len();
        frame.push_frame(Point::new(x, y), fragment.frame);
        if let Some(mapping) = &mut mapping {
            mapping.ranges.push((start, frame.items.len()));
        }
        x += advance + gap;
    }
    let mut row = Fragment::new(frame, AtomClass::Ord);
    row.italics_correction = italics;
    (row, mapping)
}

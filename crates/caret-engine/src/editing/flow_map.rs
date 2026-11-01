//! 将泛型混合内容流投影为逻辑文本、节点路径与范围。
//
// SPDX-License-Identifier: Apache-2.0
// 署名：Yuki Lu

use std::ops::Range;

use caret_doc::{FlowContent, InlineEnvironmentKind, NodeId};

use super::EditError;
use super::grapheme::GraphemeMap;

/// 行内数学对象在宿主逻辑文本中的占位字符。
pub(crate) const OBJECT_REPLACEMENT: char = '\u{fffc}';

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FlowPathOptions {
    pub(super) physical: bool,
    pub(super) legacy_edit: bool,
}

/// 物理索引只在构图时对应的宿主树版本内有效；修改结构后必须重新构图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlowPath {
    pub(super) content_indices: Vec<usize>,
    pub(super) run_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum FlowSpanKind<AtomTag> {
    Run,
    SoftBreak,
    MathAtom(AtomTag),
}

/// 一段逻辑文本对应的物理位置和过渡期编辑路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlowSpan<AtomTag> {
    pub(super) range: Range<usize>,
    pub(super) path: Option<FlowPath>,
    /// 现有正文写入器仍使用展平 EditNode 索引；迁移写入器前不能替换它。
    pub(super) legacy_edit_path: Option<Vec<usize>>,
    pub(super) environment_depth: usize,
    pub(super) kind: FlowSpanKind<AtomTag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlowEnvironmentRange {
    pub(super) id: NodeId,
    pub(super) kind: InlineEnvironmentKind,
    pub(super) range: Range<usize>,
    pub(super) depth: usize,
    pub(super) path: Option<FlowPath>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FlowMap<SpanData> {
    pub(super) text: String,
    pub(super) spans: Vec<SpanData>,
    /// 后序排列，与原正文 environment span 顺序一致。
    pub(super) environments: Vec<FlowEnvironmentRange>,
    pub(super) grapheme_boundaries: Vec<usize>,
}

struct FlowPosition<'a> {
    content_path: &'a [usize],
    run_index: Option<usize>,
    legacy_path: &'a [usize],
    environment_depth: usize,
}

#[derive(Default)]
struct FlowPaths {
    physical: Vec<usize>,
    legacy_edit: Vec<usize>,
}

/// 按宿主规则投影一个 FlowContent 流；固定 MathFlow 槽位由宿主分别投影。
///
/// span 投影失败时映射整体失败。回调产生的外部副作用由调用方暂存，并且只能在成功后提交。
pub(super) fn build_flow_map_with_spans<T, AtomTag, SpanData>(
    contents: &[FlowContent<T>],
    mut project_atom: impl FnMut(&T) -> AtomTag,
    path_mode: FlowPathOptions,
    mut project_span: impl FnMut(FlowSpan<AtomTag>) -> Result<SpanData, EditError>,
) -> Result<FlowMap<SpanData>, EditError> {
    let mut map = FlowMap::<SpanData> {
        text: String::new(),
        spans: Vec::new(),
        environments: Vec::new(),
        grapheme_boundaries: Vec::new(),
    };
    visit(
        contents,
        &mut FlowPaths::default(),
        &mut map,
        &mut project_atom,
        &mut project_span,
        path_mode,
        0,
    )?;
    map.grapheme_boundaries = GraphemeMap::new(&map.text).into_boundaries();
    Ok(map)
}

fn visit<T, AtomTag, SpanData>(
    contents: &[FlowContent<T>],
    paths: &mut FlowPaths,
    map: &mut FlowMap<SpanData>,
    project_atom: &mut impl FnMut(&T) -> AtomTag,
    project_span: &mut impl FnMut(FlowSpan<AtomTag>) -> Result<SpanData, EditError>,
    path_mode: FlowPathOptions,
    environment_depth: usize,
) -> Result<(), EditError> {
    let mut logical_index = 0;
    for (content_index, content) in contents.iter().enumerate() {
        match content {
            FlowContent::Text(flow) => {
                for (run_index, run) in flow.runs.iter().enumerate() {
                    paths.physical.push(content_index);
                    paths.legacy_edit.push(logical_index);
                    append_span(
                        map,
                        &run.text,
                        FlowPosition {
                            content_path: &paths.physical,
                            run_index: Some(run_index),
                            legacy_path: &paths.legacy_edit,
                            environment_depth,
                        },
                        FlowSpanKind::Run,
                        path_mode,
                        project_span,
                    )?;
                    paths.legacy_edit.pop();
                    paths.physical.pop();
                    logical_index += 1;
                }
            }
            FlowContent::Environment(environment) => {
                paths.physical.push(content_index);
                paths.legacy_edit.push(logical_index);
                let start = map.text.len();
                visit(
                    &environment.children,
                    paths,
                    map,
                    project_atom,
                    project_span,
                    path_mode,
                    environment_depth + 1,
                )?;
                map.environments.push(FlowEnvironmentRange {
                    id: environment.id.clone(),
                    kind: environment.kind,
                    range: start..map.text.len(),
                    depth: environment_depth,
                    path: path_mode.physical.then(|| FlowPath {
                        content_indices: paths.physical.clone(),
                        run_index: None,
                    }),
                });
                paths.legacy_edit.pop();
                paths.physical.pop();
                logical_index += 1;
            }
            FlowContent::SoftBreak(_) => {
                paths.physical.push(content_index);
                paths.legacy_edit.push(logical_index);
                append_span(
                    map,
                    "\n",
                    FlowPosition {
                        content_path: &paths.physical,
                        run_index: None,
                        legacy_path: &paths.legacy_edit,
                        environment_depth,
                    },
                    FlowSpanKind::SoftBreak,
                    path_mode,
                    project_span,
                )?;
                paths.legacy_edit.pop();
                paths.physical.pop();
                logical_index += 1;
            }
            FlowContent::Math(atom) => {
                paths.physical.push(content_index);
                paths.legacy_edit.push(logical_index);
                let mut encoded = [0; 4];
                append_span(
                    map,
                    OBJECT_REPLACEMENT.encode_utf8(&mut encoded),
                    FlowPosition {
                        content_path: &paths.physical,
                        run_index: None,
                        legacy_path: &paths.legacy_edit,
                        environment_depth,
                    },
                    FlowSpanKind::MathAtom(project_atom(atom)),
                    path_mode,
                    project_span,
                )?;
                paths.legacy_edit.pop();
                paths.physical.pop();
                logical_index += 1;
            }
            FlowContent::Opaque(_) => return Err(EditError::UnsupportedOpaqueContent),
        }
    }
    Ok(())
}

fn append_span<AtomTag, SpanData>(
    map: &mut FlowMap<SpanData>,
    value: &str,
    position: FlowPosition<'_>,
    kind: FlowSpanKind<AtomTag>,
    path_mode: FlowPathOptions,
    project_span: &mut impl FnMut(FlowSpan<AtomTag>) -> Result<SpanData, EditError>,
) -> Result<(), EditError> {
    let start = map.text.len();
    map.text.push_str(value);
    map.spans.push(project_span(FlowSpan {
        range: start..map.text.len(),
        path: path_mode.physical.then(|| FlowPath {
            content_indices: position.content_path.to_vec(),
            run_index: position.run_index,
        }),
        legacy_edit_path: path_mode.legacy_edit.then(|| position.legacy_path.to_vec()),
        environment_depth: position.environment_depth,
        kind,
    })?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use caret_doc::{InlineEnvironment, Run, SoftBreak, TextFlow};

    fn text(value: &str) -> FlowContent<()> {
        FlowContent::Text(TextFlow {
            runs: vec![Run {
                text: value.to_owned(),
                language: None,
                props: Default::default(),
                extra: Default::default(),
            }],
            source_range: None,
            source_map: Vec::new(),
            extra: Default::default(),
        })
    }

    #[test]
    fn map_keeps_physical_and_legacy_paths_distinct() {
        let contents = vec![
            FlowContent::Text(TextFlow {
                runs: vec![
                    Run {
                        text: String::new(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    },
                    Run {
                        text: "e".to_owned(),
                        language: None,
                        props: Default::default(),
                        extra: Default::default(),
                    },
                ],
                source_range: None,
                source_map: Vec::new(),
                extra: Default::default(),
            }),
            FlowContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Bold,
                definition_id: None,
                source_range: None,
                props: Default::default(),
                children: vec![
                    text("\u{301}"),
                    FlowContent::SoftBreak(SoftBreak::default()),
                ],
                extra: Default::default(),
            }),
            FlowContent::Math(()),
            FlowContent::Environment(InlineEnvironment {
                id: NodeId::generate(),
                kind: InlineEnvironmentKind::Italic,
                definition_id: None,
                source_range: None,
                props: Default::default(),
                children: Vec::new(),
                extra: Default::default(),
            }),
        ];

        let map = match build_flow_map_with_spans(
            &contents,
            |_| "inline-math",
            FlowPathOptions {
                physical: true,
                legacy_edit: true,
            },
            Ok,
        ) {
            Ok(map) => map,
            Err(error) => panic!("valid flow was rejected: {error}"),
        };

        assert_eq!(map.text, "e\u{301}\n\u{fffc}");
        assert_eq!(map.grapheme_boundaries, [0, 3, 4, 7]);
        assert_eq!(map.spans[0].range, 0..0);
        let physical_path = map.spans[2].path.as_ref();
        assert_eq!(
            physical_path.map(|path| path.content_indices.as_slice()),
            Some([1, 0].as_slice())
        );
        assert_eq!(physical_path.and_then(|path| path.run_index), Some(0));
        assert_eq!(
            map.spans[2].legacy_edit_path.as_deref(),
            Some([2, 0].as_slice())
        );
        assert_eq!(map.spans[3].kind, FlowSpanKind::SoftBreak);
        assert_eq!(map.spans[4].kind, FlowSpanKind::MathAtom("inline-math"));
        assert_eq!(map.environments[0].range, 1..4);
        assert_eq!(
            map.environments[0]
                .path
                .as_ref()
                .map(|path| path.content_indices.as_slice()),
            Some([1].as_slice())
        );
        assert_eq!(map.environments[1].range, 7..7);
        assert_eq!(map.environments[1].depth, 0);
    }

    #[test]
    fn opaque_content_rejects_mapping() {
        let contents = [FlowContent::<()>::Opaque(caret_doc::OpaqueNode {
            raw: serde_json::json!({"type": "future"}),
        })];

        assert!(matches!(
            build_flow_map_with_spans(
                &contents,
                |_| (),
                FlowPathOptions {
                    physical: false,
                    legacy_edit: true,
                },
                Ok,
            ),
            Err(EditError::UnsupportedOpaqueContent)
        ));
    }
}

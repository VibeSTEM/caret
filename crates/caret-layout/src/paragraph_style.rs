//! 将 `.cdoc` 的全局段落样式与段落覆盖解析为布局可直接消费的单位。

use caret_doc::{
    DocumentSettings, ParagraphLayoutOverrides, ParagraphLineSpacing as StoredLineSpacing,
};

use crate::LineSpacing;

/// 可直接供段落布局和分页使用的有效样式。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedParagraphStyle {
    pub line_spacing: LineSpacing,
    pub space_before_pt: f32,
    pub space_after_pt: f32,
    pub widow_orphan_control: bool,
    pub keep_with_next: bool,
    pub keep_lines_together: bool,
    pub page_break_before: bool,
}

/// 文档持久化的段落样式无法映射为有效布局值时返回错误。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParagraphStyleError {
    InvalidLineSpacing,
}

impl std::fmt::Display for ParagraphStyleError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("layout.invalid_paragraph_style")
    }
}

impl std::error::Error for ParagraphStyleError {}

/// 合并文档默认值和可选段落覆盖；存储长度使用百分之一 pt，布局边界转换为 pt。
pub fn resolve_paragraph_style(
    document: &DocumentSettings,
    overrides: Option<&ParagraphLayoutOverrides>,
) -> Result<ResolvedParagraphStyle, ParagraphStyleError> {
    let defaults = &document.paragraph_layout;
    let line_spacing = overrides
        .and_then(|value| value.line_spacing.as_ref())
        .unwrap_or(&defaults.line_spacing);
    let line_spacing = match line_spacing {
        StoredLineSpacing::Natural { .. } => LineSpacing::Natural,
        StoredLineSpacing::Multiple { percent, .. } if *percent > 0 => {
            LineSpacing::Multiple { percent: *percent }
        }
        StoredLineSpacing::AtLeast {
            min_centipoints, ..
        } if *min_centipoints > 0 => LineSpacing::AtLeast {
            min_centipoints: *min_centipoints,
        },
        StoredLineSpacing::Exactly { centipoints, .. } if *centipoints > 0 => {
            LineSpacing::Exactly {
                centipoints: *centipoints,
            }
        }
        _ => return Err(ParagraphStyleError::InvalidLineSpacing),
    };
    let centipoints_to_points = |value: u32| value as f32 * 0.01;

    Ok(ResolvedParagraphStyle {
        line_spacing,
        space_before_pt: centipoints_to_points(
            overrides
                .and_then(|value| value.space_before_centipoints)
                .unwrap_or(defaults.space_before_centipoints),
        ),
        space_after_pt: centipoints_to_points(
            overrides
                .and_then(|value| value.space_after_centipoints)
                .unwrap_or(defaults.space_after_centipoints),
        ),
        widow_orphan_control: overrides
            .and_then(|value| value.widow_orphan_control)
            .unwrap_or(defaults.widow_orphan_control),
        keep_with_next: overrides
            .and_then(|value| value.keep_with_next)
            .unwrap_or(defaults.keep_with_next),
        keep_lines_together: overrides
            .and_then(|value| value.keep_lines_together)
            .unwrap_or(defaults.keep_lines_together),
        page_break_before: overrides
            .and_then(|value| value.page_break_before)
            .unwrap_or(defaults.page_break_before),
    })
}

#[cfg(test)]
mod tests {
    use caret_doc::{DocumentSettings, ParagraphLayoutOverrides, ParagraphLineSpacing};

    use super::{ParagraphStyleError, resolve_paragraph_style};
    use crate::LineSpacing;

    #[test]
    fn default_document_style_resolves_to_bundled_values() {
        let style = resolve_paragraph_style(&DocumentSettings::default(), None)
            .unwrap_or_else(|error| panic!("valid defaults rejected: {error}"));

        assert_eq!(style.line_spacing, LineSpacing::Multiple { percent: 115 });
        assert_eq!(style.space_before_pt, 0.0);
        assert_eq!(style.space_after_pt, 8.0);
        assert!(style.widow_orphan_control);
        assert!(!style.keep_with_next);
        assert!(!style.keep_lines_together);
        assert!(!style.page_break_before);
    }

    #[test]
    fn paragraph_overrides_only_the_values_it_sets() {
        let style = resolve_paragraph_style(
            &DocumentSettings::default(),
            Some(&ParagraphLayoutOverrides {
                line_spacing: Some(ParagraphLineSpacing::Exactly {
                    centipoints: 1400,
                    extra: Default::default(),
                }),
                space_after_centipoints: Some(500),
                keep_with_next: Some(true),
                ..ParagraphLayoutOverrides::default()
            }),
        )
        .unwrap_or_else(|error| panic!("valid overrides rejected: {error}"));

        assert_eq!(
            style.line_spacing,
            LineSpacing::Exactly { centipoints: 1400 }
        );
        assert_eq!(style.space_before_pt, 0.0);
        assert_eq!(style.space_after_pt, 5.0);
        assert!(style.widow_orphan_control);
        assert!(style.keep_with_next);
    }

    #[test]
    fn zero_multiple_is_rejected_at_layout_boundary() {
        let error = resolve_paragraph_style(
            &DocumentSettings::default(),
            Some(&ParagraphLayoutOverrides {
                line_spacing: Some(ParagraphLineSpacing::Multiple {
                    percent: 0,
                    extra: Default::default(),
                }),
                ..ParagraphLayoutOverrides::default()
            }),
        );

        assert_eq!(error, Err(ParagraphStyleError::InvalidLineSpacing));
    }
}

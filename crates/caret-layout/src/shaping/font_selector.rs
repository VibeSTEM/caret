//! 为每个 shaping cluster 组合文档候选字体、emoji 字体和 Fontique 系统回退。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use fontique::{
    Attributes, FallbackKey, FontStyle, FontWeight, FontWidth, GenericFamily, QueryFont,
    QueryStatus,
};
use parley_engine::itemize::Segment;
use parley_engine::shape::CharCluster;
use parley_engine::{
    AnalysisDataSources, FontData, FontInstance, FontInstanceRef, FontSelector, ShapeOptions,
};

use crate::FontCatalog;
use crate::font_selection::{FontPreferenceRules, FontSelectionContext};

use super::ShapingStyle;

pub(super) struct CatalogSelector<'a> {
    query: fontique::Query<'a>,
    styles: &'a [ShapingStyle],
    preferences: Option<&'a FontPreferenceRules>,
    active_style_index: Option<u16>,
    active_emoji_mode: Option<bool>,
    active_script: Option<String>,
    active_language: Option<String>,
    selected: Option<FontInstance>,
    data_sources: AnalysisDataSources,
    pub(super) requested_family_missing: bool,
    pub(super) actual_families: Vec<String>,
    family_names: Arc<HashMap<fontique::FamilyId, String>>,
    available_family_names: Arc<HashSet<String>>,
    pub(super) font_generation: u64,
}

impl<'a> CatalogSelector<'a> {
    pub(super) fn new(
        catalog: &'a mut FontCatalog,
        styles: &'a [ShapingStyle],
        preferences: Option<&'a FontPreferenceRules>,
    ) -> Self {
        let font_generation = catalog.generation();
        let family_names = Arc::clone(&catalog.family_names_by_id);
        let available_family_names = Arc::clone(&catalog.family_names_lowercase);
        let query = catalog.collection.query(&mut catalog.source_cache);
        Self {
            query,
            styles,
            preferences,
            active_style_index: None,
            active_emoji_mode: None,
            active_script: None,
            active_language: None,
            selected: None,
            data_sources: AnalysisDataSources::new(),
            requested_family_missing: false,
            actual_families: Vec::new(),
            family_names,
            available_family_names,
            font_generation,
        }
    }

    fn query_font(&mut self, cluster: &mut CharCluster) -> Option<QueryFont> {
        let mut best: Option<(parley_engine::shape::Coverage, QueryFont)> = None;
        self.query.matches_with(|font| {
            let coverage = cluster.calculate_coverage(
                |character| {
                    font.charmap().is_some_and(|charmap| {
                        charmap.map(character).is_some_and(|glyph| glyph != 0)
                    })
                },
                &self.data_sources,
            );
            if best
                .as_ref()
                .is_none_or(|(best_coverage, _)| coverage > *best_coverage)
            {
                best = Some((coverage, font.clone()));
            }
            if coverage.is_complete() {
                QueryStatus::Stop
            } else {
                QueryStatus::Continue
            }
        });
        best.map(|(_, font)| font)
    }
}

impl FontSelector for CatalogSelector<'_> {
    fn select_font(
        &mut self,
        segment: &Segment,
        options: &ShapeOptions<'_>,
        cluster: &mut CharCluster,
    ) -> Option<FontInstanceRef<'_>> {
        let style_index = cluster.chars().first()?.style_index;
        let style = self.styles.get(style_index as usize)?;
        let emoji_mode = cluster.is_emoji();
        let script_tag = segment.script.as_str();
        let language_tag = style.language.as_deref();
        if self.active_style_index != Some(style_index)
            || self.active_emoji_mode != Some(emoji_mode)
            || self.active_script.as_deref() != Some(script_tag)
            || self.active_language.as_deref() != language_tag
        {
            let mut families = Vec::with_capacity(7);
            if emoji_mode {
                families.push(GenericFamily::Emoji.into());
            }
            if let Some(family) = style.requested_family.as_deref() {
                self.requested_family_missing |=
                    !self.available_family_names.contains(&family.to_lowercase());
            }
            if let Some(preferences) = self.preferences {
                let script = script_tag.parse().ok()?;
                for candidate in preferences.family_candidates(FontSelectionContext {
                    script,
                    language: language_tag,
                    math_context: false,
                    explicit_family: style.requested_family.as_deref(),
                }) {
                    self.requested_family_missing |= !self
                        .available_family_names
                        .contains(&candidate.family.to_lowercase());
                    families.push(fontique::QueryFamily::from(candidate.family));
                }
            } else if let Some(family) = style.requested_family.as_deref() {
                families.push(fontique::QueryFamily::from(family));
            }
            if families.is_empty() || emoji_mode {
                families.push(GenericFamily::SystemUi.into());
            }
            self.query.set_families(families);
            self.query.set_attributes(Attributes::new(
                FontWidth::default(),
                if style.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                },
                if style.bold {
                    FontWeight::BOLD
                } else {
                    FontWeight::NORMAL
                },
            ));
            self.active_style_index = Some(style_index);
            self.active_emoji_mode = Some(emoji_mode);
            self.active_script = Some(script_tag.to_owned());
            self.active_language = language_tag.map(str::to_owned);
        }
        self.query
            .set_fallbacks(FallbackKey::new(segment.script, options.language.as_ref()));
        let font = self.query_font(cluster)?;
        if let Some(name) = self.family_names.get(&font.family.0)
            && !self.actual_families.iter().any(|previous| previous == name)
        {
            self.actual_families.push(name.clone());
        }
        self.selected = Some(FontInstance {
            font: FontData {
                data: font.blob,
                index: font.index,
            },
            synthesis: font.synthesis,
        });
        self.selected.as_ref().map(FontInstance::as_ref)
    }
}

impl FontSelector for &mut CatalogSelector<'_> {
    fn begin_segment(&mut self, segment: &Segment, options: &ShapeOptions<'_>) {
        let _ = (segment, options);
    }

    fn select_font(
        &mut self,
        segment: &Segment,
        options: &ShapeOptions<'_>,
        cluster: &mut CharCluster,
    ) -> Option<FontInstanceRef<'_>> {
        CatalogSelector::select_font(self, segment, options, cluster)
    }
}

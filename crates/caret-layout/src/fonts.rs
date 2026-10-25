//! 枚举、缓存和显式刷新系统及当前用户可用字体。

use fontique::{Collection, CollectionOptions, SourceCache};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
#[cfg(windows)]
use std::{env, fs, path::PathBuf};

/// 字体目录条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontFamily {
    /// 系统报告的字体族名。
    pub name: String,
}

/// 字体目录创建或刷新错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontCatalogError {
    /// 系统字体后端未返回任何可用字体。
    Empty,
}

impl std::fmt::Display for FontCatalogError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("layout.font_catalog_empty"),
        }
    }
}

impl std::error::Error for FontCatalogError {}

/// 缓存系统字体和已加载字体数据；输入/塑形路径不会扫描目录。
pub struct FontCatalog {
    pub(crate) collection: Collection,
    pub(crate) source_cache: SourceCache,
    generation: u64,
    families: Vec<FontFamily>,
    pub(crate) family_names_by_id: Arc<HashMap<fontique::FamilyId, String>>,
    pub(crate) family_names_lowercase: Arc<HashSet<String>>,
}

impl FontCatalog {
    /// 枚举当前系统与用户字体并构建初始目录。
    pub fn system() -> Result<Self, FontCatalogError> {
        Self::build(1)
    }

    /// 原子重建目录与字体数据缓存；旧目录在新目录构建完成前仍保持有效。
    pub fn refresh(&mut self) -> Result<u64, FontCatalogError> {
        let next_generation = self.generation.saturating_add(1);
        let replacement = Self::build(next_generation)?;
        *self = replacement;
        Ok(self.generation)
    }

    /// 当前字体目录代际，用于失效受字体变化影响的布局缓存。
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// 返回按 Unicode 小写键排序且去重的字体族快照。
    pub fn families(&self) -> &[FontFamily] {
        &self.families
    }

    fn build(generation: u64) -> Result<Self, FontCatalogError> {
        let mut collection = Collection::new(CollectionOptions {
            shared: false,
            system_fonts: true,
        });
        collection.load_system_fonts();
        #[cfg(windows)]
        collection.load_fonts_from_paths(installed_font_paths());
        let mut families: Vec<_> = collection
            .family_names()
            .map(|name| FontFamily {
                name: name.to_owned(),
            })
            .collect();
        families.sort_by_cached_key(|family| family.name.to_lowercase());
        families.dedup_by(|left, right| left.name.eq_ignore_ascii_case(&right.name));
        if families.is_empty() {
            return Err(FontCatalogError::Empty);
        }
        let family_ids: Vec<_> = collection.family_ids().collect();
        let family_names_by_id = family_ids
            .into_iter()
            .filter_map(|id| collection.family_name(id).map(|name| (id, name.to_owned())))
            .collect();
        let family_names_lowercase = families
            .iter()
            .map(|family| family.name.to_lowercase())
            .collect();
        Ok(Self {
            collection,
            source_cache: SourceCache::new(Default::default()),
            generation,
            families,
            family_names_by_id: Arc::new(family_names_by_id),
            family_names_lowercase: Arc::new(family_names_lowercase),
        })
    }
}

/// DirectWrite 返回进程缓存的系统集合；显式刷新时再扫描 Windows 标准字体目录，纳入新安装字体。
#[cfg(windows)]
fn installed_font_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(windows) = env::var_os("WINDIR") {
        paths.push(PathBuf::from(windows).join("Fonts"));
    }
    if let Some(local_app_data) = env::var_os("LOCALAPPDATA") {
        paths.push(PathBuf::from(local_app_data).join("Microsoft/Windows/Fonts"));
    }

    let mut fonts = Vec::new();
    for directory in paths {
        let Ok(entries) = fs::read_dir(directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let supported = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    matches!(
                        extension.to_ascii_lowercase().as_str(),
                        "ttf" | "otf" | "ttc" | "otc"
                    )
                });
            if supported {
                fonts.push(path);
            }
        }
    }
    fonts.sort();
    fonts
}

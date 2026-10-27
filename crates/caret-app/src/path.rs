//! 跨平台路径与逻辑资源引用。
//!
//! 根类别由宿主显式注入（以后由 Qt 经标准目录发现后传入），领域层只保存结构化的
//! 根类别 + 相对片段，解析时才生成本机路径；解析结果恒为绝对路径，
//! 与进程 cwd 无关是结构保证（不做任何相对化操作）。
//!
//! 引用规则：拒绝空引用、NUL 字节、绝对路径（含 Unix 上伪装成相对组件
//! 的 `C:` 盘符）、词法 `..` 越界、Windows 保留设备名与尾随点/空格组件；
//! 文件系统级越界（符号链接/junction）由 `resolve_existing` /
//! `resolve_write_target` 以规范化包含检查拒绝。qrc 是只读内置资源，
//! 永远不会解析为本机路径。逻辑根检查降低误操作风险，不构成对恶意
//! 并发文件系统操作的安全隔离。

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

/// 逻辑根类别；scheme 用于 `ResourceRef` 的字符串形态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RootCategory {
    /// 文档相对资源，绑定显式文档资源根（`project:/`）；不依赖 cwd。
    Project,
    /// 用户配置目录（宿主经 QStandardPaths::AppConfigLocation 注入）。
    UserConfig,
    /// 应用数据目录（宿主经 QStandardPaths::AppDataLocation 注入）。
    AppData,
    /// 可重建缓存目录（宿主经 QStandardPaths::CacheLocation 注入）。
    Cache,
    /// 会话/临时工作区（宿主经 QStandardPaths::TempLocation 注入）。
    Session,
    /// 内置 qrc 资源：只读，不映射为本机路径。
    Qrc,
}

impl RootCategory {
    /// 逻辑地址 scheme；解析与生成都以此为准，未知 scheme 拒绝。
    pub fn scheme(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::UserConfig => "user-config",
            Self::AppData => "app-data",
            Self::Cache => "cache",
            Self::Session => "session",
            Self::Qrc => "qrc",
        }
    }

    /// 由 scheme 解析类别；大小写敏感。
    pub fn from_scheme(scheme: &str) -> Option<Self> {
        match scheme {
            "project" => Some(Self::Project),
            "user-config" => Some(Self::UserConfig),
            "app-data" => Some(Self::AppData),
            "cache" => Some(Self::Cache),
            "session" => Some(Self::Session),
            "qrc" => Some(Self::Qrc),
            _ => None,
        }
    }
}

/// 稳定错误码：UI/跨语言只按 `code()` 分支，`Display` 供日志诊断。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// 引用为空或仅分隔符。
    EmptyReference,
    /// 相对片段内含绝对路径成分（根目录、盘符前缀或 UNC）。
    AbsoluteRejected(String),
    /// 词法 `..` 越出根目录。
    ParentEscape(String),
    /// 组件命中 Windows 保留设备名（CON、COM1 等），跨平台统一拒绝。
    ReservedName(String),
    /// 组件以点或空格结尾，Windows 上非法，统一拒绝。
    TrailingDotOrSpace(String),
    /// 引用内含 NUL 字节。
    NulByte,
    /// 逻辑引用仅使用 `/`，拒绝可能被 Windows 当作分隔符的反斜杠。
    BackslashRejected(String),
    /// 未知 scheme。
    UnknownScheme(String),
    /// 引用缺少 `scheme:/` 前缀。
    MissingScheme,
    /// 根类别未注入或目录不存在/不可用。
    RootMissing(RootCategory),
    /// 注入的根不是绝对路径。
    RootNotAbsolute(RootCategory),
    /// qrc 无本机根，注入或本机解析都被拒绝。
    QrcRootForbidden,
    /// qrc 引用被请求解析为本机路径。
    QrcNotNative,
    /// 目标或其现存祖先规范化后越出根（符号链接/junction 越界）。
    NotContained(String),
    /// `resolve_existing` 的目标不存在。
    NotFound(String),
}

impl PathError {
    /// 机器可读错误码；跨 FFI 以 `code: detail` 文本传递。
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyReference => "path.empty_reference",
            Self::AbsoluteRejected(_) => "path.absolute_rejected",
            Self::ParentEscape(_) => "path.parent_escape",
            Self::ReservedName(_) => "path.reserved_name",
            Self::TrailingDotOrSpace(_) => "path.trailing_dot_or_space",
            Self::NulByte => "path.nul_byte",
            Self::BackslashRejected(_) => "path.backslash_rejected",
            Self::UnknownScheme(_) => "path.unknown_scheme",
            Self::MissingScheme => "path.missing_scheme",
            Self::RootMissing(_) => "path.root_missing",
            Self::RootNotAbsolute(_) => "path.root_not_absolute",
            Self::QrcRootForbidden => "path.qrc_root_forbidden",
            Self::QrcNotNative => "path.qrc_not_native",
            Self::NotContained(_) => "path.not_contained",
            Self::NotFound(_) => "path.not_found",
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::EmptyReference
            | Self::NulByte
            | Self::MissingScheme
            | Self::QrcRootForbidden
            | Self::QrcNotNative => String::new(),
            Self::AbsoluteRejected(fragment)
            | Self::BackslashRejected(fragment)
            | Self::ParentEscape(fragment)
            | Self::NotContained(fragment)
            | Self::NotFound(fragment) => fragment.clone(),
            Self::ReservedName(name) | Self::TrailingDotOrSpace(name) => name.clone(),
            Self::UnknownScheme(scheme) => scheme.to_owned(),
            Self::RootMissing(category) | Self::RootNotAbsolute(category) => {
                category.scheme().to_owned()
            }
        }
    }
}

impl Display for PathError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let detail = self.detail();
        if detail.is_empty() {
            write!(formatter, "{}", self.code())
        } else {
            write!(formatter, "{}: {}", self.code(), detail)
        }
    }
}

/// 结构化逻辑资源引用：根类别 + 相对片段。
/// 字符串形态为 `scheme:/relative/...`（`ResourceRef::parse` /
/// `ResourceRef::to_logical` 互逆；相对片段原样保留，不做磁盘规范化）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceRef {
    pub category: RootCategory,
    pub relative: String,
}

impl ResourceRef {
    /// 解析逻辑地址；scheme 后必须跟 `:/`。相对片段合法性由 `PathService` 的解析入口
    /// 统一校验，此处只做结构切分。
    pub fn parse(reference: &str) -> Result<Self, PathError> {
        let Some((scheme, relative)) = reference.split_once(":/") else {
            return Err(PathError::MissingScheme);
        };
        if scheme.is_empty() {
            return Err(PathError::MissingScheme);
        }
        let Some(category) = RootCategory::from_scheme(scheme) else {
            return Err(PathError::UnknownScheme(scheme.to_owned()));
        };
        Ok(Self {
            category,
            relative: relative.to_owned(),
        })
    }

    /// 生成逻辑地址；与 `parse` 往返一致。
    pub fn to_logical(&self) -> String {
        format!("{}:/{}", self.category.scheme(), self.relative)
    }
}

impl From<PathError> for String {
    /// 错误文本即 `Display` 渲染（`code: detail`），供以字符串错误为契约的
    /// 边界（FFI 包装、测试 `?` 传播）直接转换。
    fn from(error: PathError) -> String {
        error.to_string()
    }
}

impl std::error::Error for PathError {}

/// Windows 保留设备名（不含扩展名比较）；跨平台统一拒绝，保证文档资源引用
/// 在平台间搬迁时不会落到目标平台非法文件名上。
const RESERVED_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 校验相对片段并返回词法规范化后的路径片段列表（去除 `.`，折叠受控
/// `..`）；任何越界/非法成分在此拒绝，不触碰文件系统。
fn validate_relative(relative: &str) -> Result<Vec<String>, PathError> {
    if relative.is_empty() {
        return Err(PathError::EmptyReference);
    }
    if relative.contains('\0') {
        return Err(PathError::NulByte);
    }
    // 不让 PathBuf::join 在 Windows 将单个逻辑组件重新解释为 UNC 或父级路径。
    if relative.contains('\\') {
        return Err(PathError::BackslashRejected(relative.to_owned()));
    }
    let mut folded: Vec<String> = Vec::new();
    // 逻辑引用以 `/` 为唯一分隔符，手工切分而非经平台 Path 解析：Windows
    // 盘符 Prefix、反斜杠分隔等平台差异（Linux/macOS 上永不可达或语义漂移）
    // 在三平台间保持同一套判定。
    for part in relative.split('/') {
        if part.is_empty() {
            // 首个空组件 = 引用以 `/` 开头（绝对路径冒充相对）。
            if folded.is_empty() && relative.starts_with('/') {
                return Err(PathError::AbsoluteRejected(format!("/{relative}")));
            }
            // 尾随或连续分隔符按容忍处理（与 Path::components 的尾斜杠语义一致）。
            continue;
        }
        if part == "." {
            continue;
        }
        if part == ".." {
            if folded.pop().is_some() {
                continue;
            }
            return Err(PathError::ParentEscape(relative.to_owned()));
        }
        // Unix 上盘符前缀是普通组件。`C:` 与 `C:secret` 都要拒绝：
        // Windows 的 `PathBuf::push` 会把它们当成新的盘符，结果不再位于注入根内。
        if part.len() >= 2 && part.as_bytes()[1] == b':' && part.as_bytes()[0].is_ascii_alphabetic()
        {
            return Err(PathError::AbsoluteRejected(format!("{part}/{relative}")));
        }
        if part.ends_with('.') || part.ends_with(' ') {
            return Err(PathError::TrailingDotOrSpace(part.to_owned()));
        }
        let stem = part
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if RESERVED_NAMES.contains(&stem.as_str()) {
            return Err(PathError::ReservedName(part.to_owned()));
        }
        folded.push(part.to_owned());
    }
    if folded.is_empty() {
        return Err(PathError::EmptyReference);
    }
    Ok(folded)
}

/// 根类别到本机根目录的解析服务。根由宿主显式注入且必须绝对；未注入的
/// 类别解析即失败，不回退 cwd 或任何隐式位置。
#[derive(Debug, Clone, Default)]
pub struct PathService {
    roots: BTreeMap<RootCategory, PathBuf>,
}

impl PathService {
    pub fn new() -> Self {
        Self::default()
    }

    /// 注入/替换某类别的根；qrc 无本机根，重复注入以最新为准。
    pub fn set_root(&mut self, category: RootCategory, root: &Path) -> Result<(), PathError> {
        if category == RootCategory::Qrc {
            return Err(PathError::QrcRootForbidden);
        }
        if !root.is_absolute() {
            return Err(PathError::RootNotAbsolute(category));
        }
        self.roots.insert(category, root.to_path_buf());
        Ok(())
    }

    /// 已注入的本机根（qrc 恒为 `None`）。
    pub fn root(&self, category: RootCategory) -> Option<&Path> {
        self.roots.get(&category).map(|root| root.as_path())
    }

    fn native_root(&self, category: RootCategory) -> Result<&Path, PathError> {
        if category == RootCategory::Qrc {
            return Err(PathError::QrcNotNative);
        }
        self.roots
            .get(&category)
            .map(|root| root.as_path())
            .ok_or(PathError::RootMissing(category))
    }

    /// 纯逻辑解析：结构校验 + 根拼接，不访问文件系统；结果为绝对路径。
    pub fn resolve(&self, reference: &ResourceRef) -> Result<PathBuf, PathError> {
        let root = self.native_root(reference.category)?;
        let folded = validate_relative(&reference.relative)?;
        let mut path = root.to_path_buf();
        for part in folded {
            path.push(part);
        }
        Ok(path)
    }

    /// 校验逻辑引用结构和根授权，不检查目标是否存在。
    pub fn validate_reference(&self, reference: &ResourceRef) -> Result<(), PathError> {
        validate_relative(&reference.relative)?;
        if reference.category != RootCategory::Qrc {
            self.native_root(reference.category)?;
        }
        Ok(())
    }

    /// 读解析：目标必须存在；目标与根都规范化（解析符号链接/junction）
    /// 后必须保持包含关系，否则视为文件系统级越界。
    pub fn resolve_existing(&self, reference: &ResourceRef) -> Result<PathBuf, PathError> {
        let path = self.resolve(reference)?;
        let canonical_root = self.canonical_root(reference.category)?;
        let canonical = std::fs::canonicalize(&path)
            .map_err(|_| PathError::NotFound(path.display().to_string()))?;
        if !canonical.starts_with(canonical_root) {
            return Err(PathError::NotContained(canonical.display().to_string()));
        }
        Ok(canonical)
    }

    /// 写目标解析：目标可以尚不存在；自目标向上的最深现存祖先（不越过
    /// 根）规范化后必须仍在根内，覆盖"未创建目标"的越界场景。返回未
    /// 规范化的拼接路径（写入方按原语义创建文件），包含性已由祖先保证。
    /// 祖先回溯按注入形态的根做前缀判断，包含判定用双方的规范化形态，
    /// 注入根本身含符号链接时不会误判。悬空链接也算已有路径项，无法规范化
    /// 时拒绝，不能跳过链接后把其目标误当成根内尚未创建的文件。
    pub fn resolve_write_target(&self, reference: &ResourceRef) -> Result<PathBuf, PathError> {
        let canonical_root = self.canonical_root(reference.category)?;
        let path = self.resolve(reference)?;
        let ancestor = path
            .ancestors()
            .find(|candidate| {
                // exists() 跟随符号链接，悬空链接会被误判为不存在。
                !matches!(std::fs::symlink_metadata(candidate),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound)
            })
            .ok_or_else(|| PathError::NotContained(path.display().to_string()))?;
        let canonical_ancestor = std::fs::canonicalize(ancestor)
            .map_err(|_| PathError::NotContained(ancestor.display().to_string()))?;
        if !canonical_ancestor.starts_with(canonical_root) {
            return Err(PathError::NotContained(
                canonical_ancestor.display().to_string(),
            ));
        }
        Ok(path)
    }

    /// 根必须存在且可规范化；标准目录缺失时立即报错，不静默回退。
    fn canonical_root(&self, category: RootCategory) -> Result<PathBuf, PathError> {
        let root = self.native_root(category)?;
        std::fs::canonicalize(root).map_err(|_| PathError::RootMissing(category))
    }
}

#[cfg(test)]
mod tests {
    use super::{PathError, PathService, ResourceRef, RootCategory};
    use std::fs;
    use std::path::{Path, PathBuf};

    /// 隔离夹具根：位于系统临时目录下（先规范化，规避 macOS /tmp 符号
    /// 链接前缀差异），测试结束清理。
    struct FixtureRoot {
        root: PathBuf,
    }

    impl FixtureRoot {
        fn new(label: &str) -> std::io::Result<Self> {
            let base = std::fs::canonicalize(std::env::temp_dir())?;
            let root = base.join(format!("caret-path-{}-{label}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root)?;
            Ok(Self { root })
        }

        fn service(&self) -> Result<PathService, PathError> {
            let mut service = PathService::new();
            service.set_root(RootCategory::Project, &self.root)?;
            Ok(service)
        }
    }

    impl Drop for FixtureRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn reference(relative: &str) -> ResourceRef {
        ResourceRef {
            category: RootCategory::Project,
            relative: relative.to_owned(),
        }
    }

    #[test]
    fn logical_reference_round_trips() -> Result<(), Box<dyn std::error::Error>> {
        let parsed = ResourceRef::parse("project:/assets/齿轮 box/a.txt")?;
        assert_eq!(parsed.category, RootCategory::Project);
        assert_eq!(parsed.relative, "assets/齿轮 box/a.txt");
        assert_eq!(parsed.to_logical(), "project:/assets/齿轮 box/a.txt");

        for scheme in ["user-config", "app-data", "cache", "session", "qrc"] {
            let parsed = ResourceRef::parse(&format!("{scheme}:/x"))?;
            assert_eq!(parsed.category.scheme(), scheme);
        }
        Ok(())
    }

    #[test]
    fn every_scheme_resolves_only_under_its_injected_root() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = FixtureRoot::new("schemes")?;
        let mut service = PathService::new();
        for category in [
            RootCategory::Project,
            RootCategory::UserConfig,
            RootCategory::AppData,
            RootCategory::Cache,
            RootCategory::Session,
        ] {
            let root = fixture.root.join(category.scheme());
            fs::create_dir(&root)?;
            service.set_root(category, &root)?;
            let resolved = service.resolve(&ResourceRef {
                category,
                relative: "item".to_owned(),
            })?;
            assert_eq!(resolved, root.join("item"));
            assert!(resolved.is_absolute());
        }
        assert!(matches!(
            service.resolve(&ResourceRef {
                category: RootCategory::Qrc,
                relative: "item".to_owned(),
            }),
            Err(PathError::QrcNotNative)
        ));
        assert!(matches!(
            ResourceRef::parse("workspace:/item"),
            Err(PathError::UnknownScheme(scheme)) if scheme == "workspace"
        ));
        Ok(())
    }

    #[test]
    fn logical_reference_rejects_structural_errors() -> Result<(), Box<dyn std::error::Error>> {
        for (input, expected) in [
            ("", PathError::MissingScheme),
            ("assets/x", PathError::MissingScheme),
            (":/x", PathError::MissingScheme),
            (
                "http://example.com/x",
                PathError::UnknownScheme("http".to_owned()),
            ),
            (
                "workspace:/x",
                PathError::UnknownScheme("workspace".to_owned()),
            ),
        ] {
            assert!(
                matches!(ResourceRef::parse(input), Err(ref error) if *error == expected),
                "input={input:?} 应被拒绝"
            );
        }
        Ok(())
    }

    #[test]
    fn resolve_joins_without_filesystem_and_stays_absolute()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = FixtureRoot::new("join")?;
        let service = fixture.service()?;
        let resolved = service.resolve(&reference("assets/./b/../齿轮/a.txt"))?;
        assert!(resolved.is_absolute());
        assert_eq!(
            resolved,
            fixture.root.join("assets").join("齿轮").join("a.txt")
        );
        Ok(())
    }

    #[test]
    fn relative_rules_reject_invalid_fragments() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = FixtureRoot::new("rules")?;
        let service = fixture.service()?;
        for (fragment, code) in [
            ("", "path.empty_reference"),
            (".", "path.empty_reference"),
            ("/abs", "path.absolute_rejected"),
            ("C:/win", "path.absolute_rejected"),
            ("C:secret", "path.absolute_rejected"),
            ("notes/C:secret", "path.absolute_rejected"),
            ("//server/share/asset.pa", "path.absolute_rejected"),
            (r"\\server\share\asset.pa", "path.backslash_rejected"),
            (r"C:\win", "path.backslash_rejected"),
            (r"assets\..\..\outside", "path.backslash_rejected"),
            ("..", "path.parent_escape"),
            ("a/../../b", "path.parent_escape"),
            ("CON", "path.reserved_name"),
            ("dir/com3.txt", "path.reserved_name"),
            ("trailing.", "path.trailing_dot_or_space"),
            ("trailing ", "path.trailing_dot_or_space"),
            ("nul\0byte", "path.nul_byte"),
        ] {
            assert!(
                matches!(
                    service.resolve(&reference(fragment)),
                    Err(ref error) if error.code() == code
                ),
                "fragment={fragment:?} 应被拒绝"
            );
        }
        Ok(())
    }

    #[test]
    fn roots_must_be_absolute_and_injected() -> Result<(), Box<dyn std::error::Error>> {
        let mut service = PathService::new();
        assert!(matches!(
            service.set_root(RootCategory::Project, Path::new("relative/root")),
            Err(PathError::RootNotAbsolute(RootCategory::Project))
        ));
        assert!(matches!(
            service.set_root(RootCategory::Qrc, Path::new("/tmp")),
            Err(PathError::QrcRootForbidden)
        ));
        // root() 访问器：未注入返回 None，注入后可读。
        assert_eq!(service.root(RootCategory::Project), None);
        assert!(matches!(
            service.resolve(&reference("a")),
            Err(PathError::RootMissing(RootCategory::Project))
        ));
        let fixture = FixtureRoot::new("roots")?;
        service.set_root(RootCategory::Project, &fixture.root)?;
        assert_eq!(
            service.root(RootCategory::Project),
            Some(fixture.root.as_path())
        );
        assert_eq!(service.resolve(&reference("a"))?, fixture.root.join("a"));
        Ok(())
    }

    #[test]
    fn qrc_never_resolves_to_native_path() -> Result<(), Box<dyn std::error::Error>> {
        let fixture = FixtureRoot::new("qrc")?;
        let service = fixture.service()?;
        let qrc_ref = ResourceRef {
            category: RootCategory::Qrc,
            relative: "icons/open.svg".to_owned(),
        };
        assert!(matches!(
            service.resolve(&qrc_ref),
            Err(PathError::QrcNotNative)
        ));
        Ok(())
    }

    #[test]
    fn resolve_preserves_component_case_without_folding() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = FixtureRoot::new("case")?;
        let service = fixture.service()?;
        let upper = service.resolve(&reference("Assets/GearBox.PA"))?;
        let lower = service.resolve(&reference("assets/gearbox.pa"))?;
        // 服务层不做任何大小写折叠：两种写法保持各自的字节形态。磁盘的
        // 大小写敏感性由平台文件系统决定（macOS/Windows 默认不敏感），
        // 引用语义层的区分交给调用方。
        assert_ne!(upper, lower);
        assert!(upper.ends_with("Assets/GearBox.PA"));
        assert!(lower.ends_with("assets/gearbox.pa"));
        Ok(())
    }

    #[test]
    fn resolve_existing_requires_present_contained_target() -> Result<(), Box<dyn std::error::Error>>
    {
        let fixture = FixtureRoot::new("existing")?;
        let assets = fixture.root.join("assets");
        fs::create_dir_all(&assets)?;
        fs::write(assets.join("模型 v1.txt"), b"note")?;

        let service = fixture.service()?;
        let existing = service.resolve_existing(&reference("assets/模型 v1.txt"))?;
        assert!(existing.is_absolute());
        assert!(existing.ends_with("模型 v1.txt"));

        assert!(matches!(
            service.resolve_existing(&reference("assets/missing.txt")),
            Err(ref error) if error.code() == "path.not_found"
        ));
        Ok(())
    }

    #[test]
    fn write_target_allows_missing_file_but_checks_ancestor()
    -> Result<(), Box<dyn std::error::Error>> {
        let fixture = FixtureRoot::new("write")?;
        fs::create_dir_all(fixture.root.join("out"))?;
        let service = fixture.service()?;
        let target = service.resolve_write_target(&reference("out/新 工程口袋/未创建.pa"))?;
        assert_eq!(
            target,
            fixture
                .root
                .join("out")
                .join("新 工程口袋")
                .join("未创建.pa")
        );
        Ok(())
    }

    #[test]
    fn missing_root_directory_is_an_error() -> Result<(), Box<dyn std::error::Error>> {
        let base = std::fs::canonicalize(std::env::temp_dir())?;
        let absent = base.join(format!("caret-path-absent-{}", std::process::id()));
        let _ = fs::remove_dir_all(&absent);
        let mut service = PathService::new();
        service.set_root(RootCategory::Project, &absent)?;
        assert!(matches!(
            service.resolve_existing(&reference("a")),
            Err(PathError::RootMissing(RootCategory::Project))
        ));
        let _ = fs::remove_dir_all(&absent);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_escape_outside_root_is_rejected() -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let fixture = FixtureRoot::new("symlink")?;
        // 根目录的兄弟目录：with_file_name 直接构造，无 Option 分支。
        let outside = fixture
            .root
            .with_file_name(format!("caret-path-outside-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outside);
        fs::create_dir_all(&outside)?;
        fs::write(outside.join("secret.pa"), b"x")?;

        let link = fixture.root.join("leak");
        symlink(&outside, &link)?;

        let service = fixture.service()?;
        assert!(matches!(
            service.resolve_existing(&reference("leak/secret.pa")),
            Err(ref error) if error.code() == "path.not_contained"
        ));
        // 写目标同样按最深现存祖先拒绝。
        assert!(matches!(
            service.resolve_write_target(&reference("leak/secret.pa")),
            Err(ref error) if error.code() == "path.not_contained"
        ));
        let _ = fs::remove_dir_all(&outside);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlinks_are_not_treated_as_new_write_targets()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let fixture = FixtureRoot::new("dangling-symlink")?;
        let outside = FixtureRoot::new("dangling-outside")?;
        let service = fixture.service()?;
        for (name, target) in [
            ("outside-file", outside.root.join("missing.txt")),
            ("inside-file", fixture.root.join("missing.txt")),
        ] {
            symlink(&target, fixture.root.join(name))?;
            assert!(service.resolve_existing(&reference(name)).is_err());
            assert!(matches!(
                service.resolve_write_target(&reference(name)),
                Err(PathError::NotContained(_))
            ));
            assert!(!target.exists());
        }
        symlink(
            outside.root.join("missing-dir"),
            fixture.root.join("outside-dir"),
        )?;
        assert!(matches!(
            service.resolve_write_target(&reference("outside-dir/new.txt")),
            Err(PathError::NotContained(_))
        ));
        assert!(!outside.root.join("missing-dir").exists());
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_root_allows_existing_and_missing_targets_inside_the_root()
    -> Result<(), Box<dyn std::error::Error>> {
        use std::os::unix::fs::symlink;

        let fixture = FixtureRoot::new("symlink-root")?;
        let actual_root = fixture.root.join("actual");
        fs::create_dir(&actual_root)?;
        fs::write(actual_root.join("part.txt"), b"text")?;
        let linked_root = fixture.root.join("linked-root");
        symlink(&actual_root, &linked_root)?;

        let mut service = PathService::new();
        service.set_root(RootCategory::Project, &linked_root)?;
        assert_eq!(
            service.resolve_existing(&reference("part.txt"))?,
            fs::canonicalize(actual_root.join("part.txt"))?
        );
        assert_eq!(
            service.resolve_write_target(&reference("new/result.pa"))?,
            linked_root.join("new/result.pa")
        );
        Ok(())
    }

    #[test]
    fn error_code_and_detail_cover_every_variant() -> Result<(), Box<dyn std::error::Error>> {
        // 遍历所有错误变体，覆盖 code()/detail() 的每个 match 臂。
        let cases: Vec<(PathError, &str, &str)> = vec![
            (PathError::EmptyReference, "path.empty_reference", ""),
            (
                PathError::AbsoluteRejected("C:/x".into()),
                "path.absolute_rejected",
                "C:/x",
            ),
            (
                PathError::ParentEscape("..".into()),
                "path.parent_escape",
                "..",
            ),
            (
                PathError::ReservedName("NUL".into()),
                "path.reserved_name",
                "NUL",
            ),
            (
                PathError::TrailingDotOrSpace("x ".into()),
                "path.trailing_dot_or_space",
                "x ",
            ),
            (PathError::NulByte, "path.nul_byte", ""),
            (
                PathError::BackslashRejected(r"assets\part.pa".into()),
                "path.backslash_rejected",
                r"assets\part.pa",
            ),
            (
                PathError::UnknownScheme("ws".into()),
                "path.unknown_scheme",
                "ws",
            ),
            (PathError::MissingScheme, "path.missing_scheme", ""),
            (
                PathError::RootMissing(RootCategory::Cache),
                "path.root_missing",
                "cache",
            ),
            (
                PathError::RootNotAbsolute(RootCategory::Cache),
                "path.root_not_absolute",
                "cache",
            ),
            (PathError::QrcRootForbidden, "path.qrc_root_forbidden", ""),
            (PathError::QrcNotNative, "path.qrc_not_native", ""),
            (
                PathError::NotContained("/x".into()),
                "path.not_contained",
                "/x",
            ),
            (PathError::NotFound("/x".into()), "path.not_found", "/x"),
        ];
        for (error, code, detail) in cases {
            assert_eq!(error.code(), code, "{error:?}");
            let rendered = error.to_string();
            if detail.is_empty() {
                assert_eq!(rendered, code, "{error:?}");
            } else {
                assert_eq!(rendered, format!("{code}: {detail}"), "{error:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn empty_and_repeated_separators_are_tolerated_consistently()
    -> Result<(), Box<dyn std::error::Error>> {
        // 手工 `/` 切分语义：尾随与连续分隔符容忍（与 Path::components 的
        // 尾斜杠语义一致）；首分隔符仍按绝对路径拒绝。
        let fixture = FixtureRoot::new("separators")?;
        let service = fixture.service()?;
        let trailing = service.resolve(&reference("assets/"))?;
        assert_eq!(trailing, fixture.root.join("assets"));
        let inner = service.resolve(&reference("assets//齿轮"))?;
        assert_eq!(inner, fixture.root.join("assets").join("齿轮"));
        Ok(())
    }

    #[test]
    fn path_error_converts_to_string_directly() -> Result<(), Box<dyn std::error::Error>> {
        let rendered: String = PathError::NotFound("/x".into()).into();
        assert_eq!(rendered, "path.not_found: /x");
        Ok(())
    }
}

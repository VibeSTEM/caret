//! 验证应用资源路径的解析和安全边界。

use super::{PathError, PathService, ResourceRef, RootCategory};
use core::mem::DropGuard;
use std::fs;
use std::path::{Path, PathBuf};

/// 夹具根目录的清理守卫。
type FixtureGuard = DropGuard<PathBuf, fn(PathBuf)>;

/// 创建隔离的夹具根目录：位于系统临时目录下（先规范化，规避 macOS /tmp 符号
/// 链接前缀差异），守卫在测试结束时递归清理。
fn fixture_root(label: &str) -> std::io::Result<(PathBuf, FixtureGuard)> {
    let base = std::fs::canonicalize(std::env::temp_dir())?;
    let root = base.join(format!("caret-path-{}-{label}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root)?;
    let guard: FixtureGuard = DropGuard::new(root.clone(), |root| {
        let _ = fs::remove_dir_all(root);
    });
    Ok((root, guard))
}

fn service(root: &Path) -> Result<PathService, PathError> {
    let mut service = PathService::new();
    service.set_root(RootCategory::Project, root)?;
    Ok(service)
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

    for scheme in [
        "user-config",
        "app-data",
        "app-local-data",
        "cache",
        "session",
        "qrc",
    ] {
        let parsed = ResourceRef::parse(&format!("{scheme}:/x"))?;
        assert_eq!(parsed.category.scheme(), scheme);
    }
    Ok(())
}

#[test]
fn every_scheme_resolves_only_under_its_injected_root() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("schemes")?;
    let mut service = PathService::new();
    for category in [
        RootCategory::Project,
        RootCategory::UserConfig,
        RootCategory::AppData,
        RootCategory::AppLocalData,
        RootCategory::Cache,
        RootCategory::Session,
    ] {
        let category_root = root.join(category.scheme());
        fs::create_dir(&category_root)?;
        service.set_root(category, &category_root)?;
        let resolved = service.resolve(&ResourceRef {
            category,
            relative: "item".to_owned(),
        })?;
        assert_eq!(resolved, category_root.join("item"));
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
fn resolve_joins_without_filesystem_and_stays_absolute() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("join")?;
    let service = service(&root)?;
    let resolved = service.resolve(&reference("assets/./b/../齿轮/a.txt"))?;
    assert!(resolved.is_absolute());
    assert_eq!(resolved, root.join("assets").join("齿轮").join("a.txt"));
    Ok(())
}

#[test]
fn relative_rules_reject_invalid_fragments() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("rules")?;
    let service = service(&root)?;
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
    let (root, _cleanup) = fixture_root("roots")?;
    service.set_root(RootCategory::Project, &root)?;
    assert_eq!(service.root(RootCategory::Project), Some(root.as_path()));
    assert_eq!(service.resolve(&reference("a"))?, root.join("a"));
    Ok(())
}

#[test]
fn qrc_never_resolves_to_native_path() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("qrc")?;
    let service = service(&root)?;
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
fn resolve_preserves_component_case_without_folding() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("case")?;
    let service = service(&root)?;
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
fn resolve_existing_requires_present_contained_target() -> Result<(), Box<dyn std::error::Error>> {
    let (root, _cleanup) = fixture_root("existing")?;
    let assets = root.join("assets");
    fs::create_dir_all(&assets)?;
    fs::write(assets.join("模型 v1.txt"), b"note")?;

    let service = service(&root)?;
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
fn write_target_allows_missing_file_but_checks_ancestor() -> Result<(), Box<dyn std::error::Error>>
{
    let (root, _cleanup) = fixture_root("write")?;
    fs::create_dir_all(root.join("out"))?;
    let service = service(&root)?;
    let target = service.resolve_write_target(&reference("out/新 工程口袋/未创建.pa"))?;
    assert_eq!(
        target,
        root.join("out").join("新 工程口袋").join("未创建.pa")
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

    let (root, _cleanup) = fixture_root("symlink")?;
    // 根目录的兄弟目录：with_file_name 直接构造，无 Option 分支。
    let outside = root.with_file_name(format!("caret-path-outside-{}", std::process::id()));
    let _ = fs::remove_dir_all(&outside);
    fs::create_dir_all(&outside)?;
    fs::write(outside.join("secret.pa"), b"x")?;

    let link = root.join("leak");
    symlink(&outside, &link)?;

    let service = service(&root)?;
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
fn dangling_symlinks_are_not_treated_as_new_write_targets() -> Result<(), Box<dyn std::error::Error>>
{
    use std::os::unix::fs::symlink;

    let (root, _cleanup) = fixture_root("dangling-symlink")?;
    let (outside_root, _outside_cleanup) = fixture_root("dangling-outside")?;
    let service = service(&root)?;
    for (name, target) in [
        ("outside-file", outside_root.join("missing.txt")),
        ("inside-file", root.join("missing.txt")),
    ] {
        symlink(&target, root.join(name))?;
        assert!(service.resolve_existing(&reference(name)).is_err());
        assert!(matches!(
            service.resolve_write_target(&reference(name)),
            Err(PathError::NotContained(_))
        ));
        assert!(!target.exists());
    }
    symlink(outside_root.join("missing-dir"), root.join("outside-dir"))?;
    assert!(matches!(
        service.resolve_write_target(&reference("outside-dir/new.txt")),
        Err(PathError::NotContained(_))
    ));
    assert!(!outside_root.join("missing-dir").exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlink_root_allows_existing_and_missing_targets_inside_the_root()
-> Result<(), Box<dyn std::error::Error>> {
    use std::os::unix::fs::symlink;

    let (root, _cleanup) = fixture_root("symlink-root")?;
    let actual_root = root.join("actual");
    fs::create_dir(&actual_root)?;
    fs::write(actual_root.join("part.txt"), b"text")?;
    let linked_root = root.join("linked-root");
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
    let (root, _cleanup) = fixture_root("separators")?;
    let service = service(&root)?;
    let trailing = service.resolve(&reference("assets/"))?;
    assert_eq!(trailing, root.join("assets"));
    let inner = service.resolve(&reference("assets//齿轮"))?;
    assert_eq!(inner, root.join("assets").join("齿轮"));
    Ok(())
}

#[test]
fn path_error_converts_to_string_directly() -> Result<(), Box<dyn std::error::Error>> {
    let rendered: String = PathError::NotFound("/x".into()).into();
    assert_eq!(rendered, "path.not_found: /x");
    Ok(())
}

//! FFI 版本、主题、布局与栅格返回契约的行为测试。

use crate::document::{
    current_caret_json, delete_backward_json, document_hit_test_json, insert_soft_break_json,
    insert_text_json, layout_snapshot_json, new_blank_document_runtime, place_caret_at_page_json,
    rasterize_page_glyphs, split_paragraph_json,
};

use crate::generated::bridge::GlyphRasterRequest;
use crate::settings::{
    builtin_theme_catalog, new_settings_runtime, set_caret_blink_enabled, settings_constraints,
    settings_load_warning, settings_snapshot, settings_theme_catalog,
};
use crate::version;

#[test]
fn ffi_version_matches_package_metadata() {
    assert_eq!(version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn ffi_theme_catalog_contains_the_builtin_theme_snapshots() {
    let catalog = builtin_theme_catalog();
    assert!(!catalog.starts_with("error:"));
    assert!(catalog.contains("\"light\""));
    assert!(catalog.contains("\"dark\""));
    assert!(catalog.contains("\"colorBackground\""));
}

#[test]
fn ffi_settings_runtime_round_trips_preferences_and_reports_constraints() -> std::io::Result<()> {
    let root = std::env::temp_dir().join(format!("caret-ffi-settings-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let path = root.join("settings.json");
    let mut runtime = new_settings_runtime(path.to_string_lossy().into_owned());

    let defaults = settings_snapshot(&mut runtime);
    assert!(defaults.error.is_empty());
    assert!(defaults.warning.is_empty());
    assert!(settings_load_warning(&mut runtime).is_empty());
    let constraints = settings_constraints();
    assert!(defaults.window_width >= constraints.minimum_window_width);
    assert!(defaults.window_height >= constraints.minimum_window_height);

    let themes = settings_theme_catalog(&mut runtime);
    assert!(!themes.starts_with("error:"));
    assert!(themes.contains("\"light\""));

    let updated = set_caret_blink_enabled(&mut runtime, false);
    assert!(updated.error.is_empty());
    assert!(!updated.caret_blink_enabled);
    assert!(path.is_file());

    let mut reopened = new_settings_runtime(path.to_string_lossy().into_owned());
    assert!(!settings_snapshot(&mut reopened).caret_blink_enabled);
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn ffi_settings_runtime_surfaces_unreadable_configuration_and_rejected_write() -> std::io::Result<()>
{
    let root = std::env::temp_dir().join(format!(
        "caret-ffi-settings-directory-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root)?;
    let mut runtime = new_settings_runtime(root.to_string_lossy().into_owned());

    let snapshot = settings_snapshot(&mut runtime);
    assert!(snapshot.error.is_empty());
    assert!(snapshot.warning.is_empty());
    assert!(!settings_load_warning(&mut runtime).is_empty());

    let rejected = set_caret_blink_enabled(&mut runtime, false);
    assert!(!rejected.error.is_empty());
    assert!(rejected.caret_blink_enabled);
    assert!(root.is_dir());
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}

#[test]
fn ffi_layout_snapshot_contains_rust_page_and_paragraph_layout() {
    let mut runtime = new_blank_document_runtime();
    let snapshot = layout_snapshot_json(&mut runtime);
    assert!(!snapshot.starts_with("error:"));
    assert!(snapshot.contains("\"widthPt\":595."));
    assert!(snapshot.contains("\"heightPt\":841."));
    assert!(snapshot.contains("\"paragraphs\":["));
    assert!(snapshot.contains("\"fontGeneration\":\"1\""));
    assert_eq!(layout_snapshot_json(&mut runtime), snapshot);
    let hit = document_hit_test_json(&mut runtime, 0, 73.0, 73.0);
    assert!(hit.contains("\"offsetUtf8\":\"0\""));
    assert_eq!(document_hit_test_json(&mut runtime, -1, 73.0, 73.0), "null");
    assert!(place_caret_at_page_json(&mut runtime, 0, 73.0, 73.0).contains("\"offsetUtf8\":\"0\""));
    assert_eq!(
        place_caret_at_page_json(&mut runtime, -1, 73.0, 73.0),
        "null"
    );
    let inserted = insert_text_json(&mut runtime, "xλ".to_owned());
    assert!(!inserted.starts_with("error:"));
    assert!(inserted.contains("\"documentGeneration\":\"1\""));
    assert!(inserted.contains("\"text\":\"xλ\""));
    let caret = current_caret_json(&mut runtime);
    assert!(caret.contains("\"pageIndex\":0"));
    assert!(caret.contains("\"xPt\":"));

    let split = split_paragraph_json(&mut runtime);
    assert!(!split.starts_with("error:"));
    assert!(split.contains("\"documentGeneration\":\"2\""));
    assert!(split.contains("\"text\":\"xλ\""));
    let soft_break = insert_soft_break_json(&mut runtime);
    assert!(!soft_break.starts_with("error:"));
    assert!(soft_break.contains("\"documentGeneration\":\"3\""));
    let deleted = delete_backward_json(&mut runtime);
    assert!(!deleted.starts_with("error:"));
    assert!(deleted.contains("\"documentGeneration\":\"4\""));
}

#[test]
fn ffi_glyph_frame_uses_binary_pixels_and_reports_invalid_geometry() {
    let mut runtime = new_blank_document_runtime();
    assert!(!insert_text_json(&mut runtime, "Caret".to_owned()).starts_with("error:"));
    let mut request = GlyphRasterRequest {
        page_index: 0,
        px_per_pt: 1.0,
        page_origin_device_x_px: 0.0,
        page_origin_device_y_px: 0.0,
        foreground_red: 32,
        foreground_green: 32,
        foreground_blue: 32,
        foreground_alpha: 255,
    };
    let frame = rasterize_page_glyphs(&mut runtime, &request);
    assert!(frame.error.is_empty());
    assert_eq!(frame.page_index, 0);
    assert_eq!(frame.document_generation, 1);
    assert_eq!(frame.font_generation, 1);
    assert!(!frame.images.is_empty());
    assert!(!frame.placements.is_empty());
    assert!(
        frame.images.iter().all(|image| {
            image.pixels.len() == image.width as usize * image.height as usize * 4
        })
    );
    assert!(frame.placements.iter().all(|placement| {
        placement.image_index < frame.images.len() as u32
            && placement.x_pt.is_finite()
            && placement.baseline_y_pt.is_finite()
    }));

    request.px_per_pt = f64::NAN;
    let invalid = rasterize_page_glyphs(&mut runtime, &request);
    assert_eq!(invalid.error, "layout.glyphs.invalid_raster_geometry");
    assert!(invalid.images.is_empty());
    assert!(invalid.placements.is_empty());
}

//! FFI 版本、主题、布局与栅格返回契约的行为测试。

use crate::document::{
    current_caret_json, delete_backward_json, document_hit_test_json, insert_soft_break_json,
    insert_text_json, layout_snapshot_json, new_blank_document_runtime, place_caret_at_page_json,
    rasterize_page_glyphs, split_paragraph_json,
};

use crate::generated::bridge::GlyphRasterRequest;
use crate::settings::builtin_theme_catalog;
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
    assert_eq!(frame.document_generation, 0);
    assert_eq!(frame.font_generation, 1);
    assert!(frame.images.is_empty());
    assert!(frame.placements.is_empty());

    request.px_per_pt = f64::NAN;
    let invalid = rasterize_page_glyphs(&mut runtime, &request);
    assert_eq!(invalid.error, "layout.glyphs.invalid_raster_geometry");
    assert!(invalid.images.is_empty());
    assert!(invalid.placements.is_empty());
}

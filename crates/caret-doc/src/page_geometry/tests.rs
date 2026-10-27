//! 验证页面几何、边距和尺寸约束。

use super::*;
use crate::NodeId;

#[test]
fn a4_dimensions_and_orientation_are_physical_point_values() {
    let portrait = PageGeometry::default();
    let (width, height) = portrait.page_size_pt();
    assert!((width - 595.275_590_551_181_2).abs() < 1e-9);
    assert!((height - 841.889_763_779_527_6).abs() < 1e-9);

    let landscape = PageGeometry {
        orientation: Orientation::Landscape,
        ..portrait
    };
    assert_eq!(landscape.page_size_pt(), (height, width));
}

#[test]
fn document_validation_rejects_malformed_known_geometry_fields() {
    let mut document = crate::Document::blank();
    document.page_sections[0].props.insert(
        "orientation".to_owned(),
        Value::String("sideways".to_owned()),
    );
    assert!(document.validate().is_err());

    document.page_sections[0].props.insert(
        "orientation".to_owned(),
        Value::String("portrait".to_owned()),
    );
    document.page_sections[0]
        .props
        .insert("paperSize".to_owned(), Value::String("letter".to_owned()));
    assert!(document.validate().is_err());

    document.page_sections[0].props.remove("paperSize");
    document.page_sections[0].props.insert(
        "marginsPt".to_owned(),
        serde_json::json!({"top": -1.0, "right": 72.0, "bottom": 72.0, "left": 72.0}),
    );
    assert!(document.validate().is_err());
}

#[test]
fn invalid_margins_are_rejected_without_mutating_page_section() {
    let mut section = PageSection::default_with_id(NodeId::generate());
    section.props.insert("custom".to_owned(), Value::Bool(true));
    let before = section.clone();
    let invalid = PageGeometry {
        margins: PageMargins {
            left: 400.0,
            right: 300.0,
            ..PageMargins::default()
        },
        ..PageGeometry::default()
    };
    assert_eq!(
        section.set_geometry(invalid),
        Err(PageGeometryError::MarginsExhaustPage)
    );
    assert_eq!(section, before);

    let invalid = PageGeometry {
        margins: PageMargins {
            top: f64::INFINITY,
            ..PageMargins::default()
        },
        ..PageGeometry::default()
    };
    assert_eq!(
        section.set_geometry(invalid),
        Err(PageGeometryError::InvalidMargins)
    );
    assert_eq!(section, before);
}

#[test]
fn geometry_round_trips_and_preserves_unknown_properties() {
    let mut section = PageSection::default_with_id(NodeId::generate());
    section
        .props
        .insert("custom".to_owned(), Value::String("keep".to_owned()));
    section.props.insert(
        "marginsPt".to_owned(),
        serde_json::json!({
            "top": 72.0,
            "right": 72.0,
            "bottom": 72.0,
            "left": 72.0,
            "futureMarginOption": "preserve"
        }),
    );
    let geometry = PageGeometry {
        paper_size: PaperSize::A3,
        orientation: Orientation::Landscape,
        margins: PageMargins {
            top: 12.345_678,
            right: 24.0,
            bottom: 36.0,
            left: 48.0,
        },
    };
    assert!(section.set_geometry(geometry).is_ok());
    assert_eq!(section.geometry(), Ok(geometry));
    assert_eq!(
        section.props.get("custom"),
        Some(&Value::String("keep".to_owned()))
    );
    assert_eq!(
        section.props["marginsPt"]["futureMarginOption"],
        Value::String("preserve".to_owned())
    );
}

#[test]
fn cdoc_round_trip_preserves_page_geometry() {
    let geometry = PageGeometry {
        paper_size: PaperSize::A3,
        orientation: Orientation::Landscape,
        margins: PageMargins {
            top: 18.25,
            right: 24.5,
            bottom: 31.75,
            left: 48.125,
        },
    };
    let mut document = crate::Document::blank();
    assert!(document.page_sections[0].set_geometry(geometry).is_ok());
    let file = match crate::CdocFile::new(document) {
        Ok(file) => file,
        Err(error) => panic!("failed to create cdoc: {error}"),
    };
    let bytes = match file.to_bytes() {
        Ok(bytes) => bytes,
        Err(error) => panic!("failed to serialize cdoc: {error}"),
    };
    let opened = match crate::open(&bytes) {
        Ok(opened) => opened,
        Err(error) => panic!("failed to reopen cdoc: {error}"),
    };
    let crate::OpenedDocument::Editable(reopened) = opened else {
        panic!("known page geometry should remain editable");
    };
    assert_eq!(
        reopened.document().page_sections[0].geometry(),
        Ok(geometry)
    );
}

#[test]
fn point_millimeter_conversion_round_trips_without_model_rounding() {
    for value in [0.0, 1.0, 25.4, 210.0, 420.0, 1000.0] {
        let round_trip = points_to_millimeters(millimeters_to_points(value));
        assert!((round_trip - value).abs() < 1e-10);
    }
}

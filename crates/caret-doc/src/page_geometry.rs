//! 定义 A4/A3 纸张几何、pt/mm 换算与经过校验的 PageSection 属性读写。

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use serde_json::{Map, Number, Value};

use crate::{ModelError, PageSection};

const DEFAULT_MARGIN_PT: f64 = 72.0;
const POINTS_PER_INCH: f64 = 72.0;
const MILLIMETERS_PER_INCH: f64 = 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperSize {
    A4,
    A3,
}

impl PaperSize {
    fn dimensions_mm(self) -> (f64, f64) {
        match self {
            Self::A4 => (210.0, 297.0),
            Self::A3 => (297.0, 420.0),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::A4 => "a4",
            Self::A3 => "a3",
        }
    }

    fn parse(value: &str) -> Result<Self, PageGeometryError> {
        match value {
            "a4" => Ok(Self::A4),
            "a3" => Ok(Self::A3),
            _ => Err(PageGeometryError::InvalidPaperSize(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }

    fn parse(value: &str) -> Result<Self, PageGeometryError> {
        match value {
            "portrait" => Ok(Self::Portrait),
            "landscape" => Ok(Self::Landscape),
            _ => Err(PageGeometryError::InvalidOrientation(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageMargins {
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub left: f64,
}

impl Default for PageMargins {
    fn default() -> Self {
        Self {
            top: DEFAULT_MARGIN_PT,
            right: DEFAULT_MARGIN_PT,
            bottom: DEFAULT_MARGIN_PT,
            left: DEFAULT_MARGIN_PT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeometry {
    pub paper_size: PaperSize,
    pub orientation: Orientation,
    pub margins: PageMargins,
}

impl Default for PageGeometry {
    fn default() -> Self {
        Self {
            paper_size: PaperSize::A4,
            orientation: Orientation::Portrait,
            margins: PageMargins::default(),
        }
    }
}

impl PageGeometry {
    pub fn page_size_pt(self) -> (f64, f64) {
        let (width_mm, height_mm) = self.paper_size.dimensions_mm();
        let width = millimeters_to_points(width_mm);
        let height = millimeters_to_points(height_mm);
        match self.orientation {
            Orientation::Portrait => (width, height),
            Orientation::Landscape => (height, width),
        }
    }

    pub fn content_size_pt(self) -> Result<(f64, f64), PageGeometryError> {
        self.validate()?;
        let (page_width, page_height) = self.page_size_pt();
        Ok((
            page_width - self.margins.left - self.margins.right,
            page_height - self.margins.top - self.margins.bottom,
        ))
    }

    pub fn validate(self) -> Result<(), PageGeometryError> {
        let (page_width, page_height) = self.page_size_pt();
        let margins = self.margins;
        let values = [margins.top, margins.right, margins.bottom, margins.left];
        if values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(PageGeometryError::InvalidMargins);
        }
        if margins.left + margins.right >= page_width || margins.top + margins.bottom >= page_height
        {
            return Err(PageGeometryError::MarginsExhaustPage);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageGeometryError {
    InvalidPaperSize(String),
    InvalidOrientation(String),
    InvalidPropertyType(String),
    InvalidMargins,
    MarginsExhaustPage,
}

impl Display for PageGeometryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPaperSize(value) => write!(formatter, "page.invalid_paper_size: {value}"),
            Self::InvalidOrientation(value) => {
                write!(formatter, "page.invalid_orientation: {value}")
            }
            Self::InvalidPropertyType(value) => {
                write!(formatter, "page.invalid_property_type: {value}")
            }
            Self::InvalidMargins => formatter.write_str("page.invalid_margins"),
            Self::MarginsExhaustPage => formatter.write_str("page.margins_exhaust_page"),
        }
    }
}

impl std::error::Error for PageGeometryError {}

pub fn millimeters_to_points(millimeters: f64) -> f64 {
    millimeters * POINTS_PER_INCH / MILLIMETERS_PER_INCH
}

pub fn points_to_millimeters(points: f64) -> f64 {
    points * MILLIMETERS_PER_INCH / POINTS_PER_INCH
}

impl PageSection {
    pub fn geometry(&self) -> Result<PageGeometry, PageGeometryError> {
        let paper_size = read_string_prop(&self.props, "paperSize")?
            .map(PaperSize::parse)
            .transpose()?
            .unwrap_or(PaperSize::A4);
        let orientation = read_string_prop(&self.props, "orientation")?
            .map(Orientation::parse)
            .transpose()?
            .unwrap_or(Orientation::Portrait);
        let margins = self
            .props
            .get("marginsPt")
            .map(parse_margins)
            .transpose()?
            .unwrap_or_default();
        let geometry = PageGeometry {
            paper_size,
            orientation,
            margins,
        };
        geometry.validate()?;
        Ok(geometry)
    }

    pub fn set_geometry(&mut self, geometry: PageGeometry) -> Result<(), PageGeometryError> {
        geometry.validate()?;
        let margins_value = margins_to_value(geometry.margins, self.props.get("marginsPt"))?;
        self.props.insert(
            "paperSize".to_owned(),
            Value::String(geometry.paper_size.as_str().to_owned()),
        );
        self.props.insert(
            "orientation".to_owned(),
            Value::String(geometry.orientation.as_str().to_owned()),
        );
        self.props
            .insert("marginsPt".to_owned(), Value::Object(margins_value));
        Ok(())
    }
}

fn read_string_prop<'a>(
    props: &'a BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<&'a str>, PageGeometryError> {
    props
        .get(key)
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| PageGeometryError::InvalidPropertyType(key.to_owned()))
        })
        .transpose()
}

fn parse_margins(value: &Value) -> Result<PageMargins, PageGeometryError> {
    let values = value.as_object().ok_or(PageGeometryError::InvalidMargins)?;
    let read = |key: &str| {
        values
            .get(key)
            .and_then(Value::as_f64)
            .ok_or(PageGeometryError::InvalidMargins)
    };
    Ok(PageMargins {
        top: read("top")?,
        right: read("right")?,
        bottom: read("bottom")?,
        left: read("left")?,
    })
}

fn margins_to_value(
    margins: PageMargins,
    previous: Option<&Value>,
) -> Result<Map<String, Value>, PageGeometryError> {
    let number = |value: f64| {
        Number::from_f64(value)
            .map(Value::Number)
            .ok_or(PageGeometryError::InvalidMargins)
    };
    let mut values = previous
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    values.insert("top".to_owned(), number(margins.top)?);
    values.insert("right".to_owned(), number(margins.right)?);
    values.insert("bottom".to_owned(), number(margins.bottom)?);
    values.insert("left".to_owned(), number(margins.left)?);
    Ok(values)
}

impl From<PageGeometryError> for ModelError {
    fn from(error: PageGeometryError) -> Self {
        Self::InvalidDocument(error.to_string())
    }
}

#[cfg(test)]
mod tests {
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
}

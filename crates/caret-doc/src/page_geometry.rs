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
mod tests;

#![doc = include_str!("../README.md")]

mod color;
pub mod filter;
pub use filter::{EvalError, Feature, FeatureValue, Filter, GeometryType};
mod number;
mod serde_support;
#[cfg(test)]
mod tests;

pub use color::Rgb;
pub use number::Number;
use serde::{Deserialize, Serialize};

/// A Maploom drawing layer, independent of PDF optional content groups (OCGs).
///
/// Serialization uses Maploom layer JSON (`type`, `source-layer`, `paint`,
/// `layout`). Only the supported fill/line subset can be deserialized.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Layer {
    Fill(FillLayer),
    Line(LineLayer),
}

impl Layer {
    pub fn id(&self) -> &str {
        match self {
            Self::Fill(layer) => &layer.id,
            Self::Line(layer) => &layer.id,
        }
    }

    pub fn source(&self) -> Option<&str> {
        match self {
            Self::Fill(layer) => layer.source.as_deref(),
            Self::Line(layer) => layer.source.as_deref(),
        }
    }

    pub fn source_layer(&self) -> &str {
        match self {
            Self::Fill(layer) => &layer.source_layer,
            Self::Line(layer) => &layer.source_layer,
        }
    }

    pub fn filter(&self) -> Option<&Filter> {
        match self {
            Self::Fill(layer) => layer.filter.as_ref(),
            Self::Line(layer) => layer.filter.as_ref(),
        }
    }

    /// Inclusive minimum and exclusive maximum zoom; `None` means unbounded.
    pub fn zoom_range(&self) -> (Option<f64>, Option<f64>) {
        match self {
            Self::Fill(layer) => (layer.minzoom, layer.maxzoom),
            Self::Line(layer) => (layer.minzoom, layer.maxzoom),
        }
    }

    pub fn visibility(&self) -> Visibility {
        match self {
            Self::Fill(layer) => layer.layout.visibility,
            Self::Line(layer) => layer.layout.visibility,
        }
    }
}

/// A polygon fill with typed paint and layout settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FillLayer {
    pub id: String,
    /// Omit for source-agnostic rendering; a name restricts drawing to that source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(rename = "source-layer")]
    pub source_layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<f64>,
    #[serde(default)]
    pub paint: FillPaint,
    #[serde(default)]
    pub layout: FillLayout,
}

impl FillLayer {
    pub fn new(id: impl Into<String>, source_layer: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: None,
            source_layer: source_layer.into(),
            filter: None,
            minzoom: None,
            maxzoom: None,
            paint: FillPaint::default(),
            layout: FillLayout::default(),
        }
    }
}

/// A line or polygon outline with typed paint and layout settings.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineLayer {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(rename = "source-layer")]
    pub source_layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minzoom: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maxzoom: Option<f64>,
    #[serde(default)]
    pub paint: LinePaint,
    #[serde(default)]
    pub layout: LineLayout,
}

impl LineLayer {
    pub fn new(id: impl Into<String>, source_layer: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            source: None,
            source_layer: source_layer.into(),
            filter: None,
            minzoom: None,
            maxzoom: None,
            paint: LinePaint::default(),
            layout: LineLayout::default(),
        }
    }
}

/// Constant RGB color and constant/zoom-dependent opacity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FillPaint {
    #[serde(rename = "fill-color")]
    pub color: Rgb,
    #[serde(rename = "fill-opacity")]
    pub opacity: Number,
}

impl Default for FillPaint {
    fn default() -> Self {
        Self {
            color: Rgb::BLACK,
            opacity: 1.0.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LinePaint {
    #[serde(rename = "line-color")]
    pub color: Rgb,
    #[serde(rename = "line-opacity")]
    pub opacity: Number,
    #[serde(rename = "line-width")]
    pub width: Number,
    /// Dash lengths in multiples of the line width.
    #[serde(rename = "line-dasharray", skip_serializing_if = "Option::is_none")]
    pub dasharray: Option<Vec<f32>>,
}

impl Default for LinePaint {
    fn default() -> Self {
        Self {
            color: Rgb::BLACK,
            opacity: 1.0.into(),
            width: 1.0.into(),
            dasharray: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FillLayout {
    pub visibility: Visibility,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LineLayout {
    pub visibility: Visibility,
    #[serde(rename = "line-cap")]
    pub cap: LineCap,
    #[serde(rename = "line-join")]
    pub join: LineJoin,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    #[default]
    Visible,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineJoin {
    #[default]
    Miter,
    Bevel,
    Round,
}

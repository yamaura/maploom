use crate::{EvalError, Expression, Feature, Filter, Number, Rgb, Value, Visibility};
use serde::{Deserialize, Serialize};

/// A string-producing feature expression. Null means an absent label.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TextField(Expression);

impl TextField {
    pub fn new(expression: Expression) -> Self {
        Self(expression)
    }
    pub fn property(key: impl Into<String>) -> Self {
        Self(Expression::property(key).to_text())
    }
    pub fn evaluate<F: Feature + ?Sized>(&self, feature: &F) -> Result<String, EvalError> {
        match self.0.evaluate(feature)? {
            Value::String(text) => Ok(text),
            Value::Null => Ok(String::new()),
            _ => Err(EvalError::new(
                "text-field must evaluate to a string or null",
            )),
        }
    }
}

impl From<&str> for TextField {
    fn from(text: &str) -> Self {
        Self(Expression::literal(text))
    }
}
impl From<String> for TextField {
    fn from(text: String) -> Self {
        Self(Expression::literal(text))
    }
}
impl Default for TextField {
    fn default() -> Self {
        "".into()
    }
}

/// Horizontal, unrotated point labels with explicit font families.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextLayer {
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
    pub layout: TextLayout,
    #[serde(default)]
    pub paint: TextPaint,
}

impl TextLayer {
    pub fn new(
        id: impl Into<String>,
        source_layer: impl Into<String>,
        field: impl Into<TextField>,
    ) -> Self {
        Self {
            id: id.into(),
            source: None,
            source_layer: source_layer.into(),
            filter: None,
            minzoom: None,
            maxzoom: None,
            layout: TextLayout {
                field: field.into(),
                ..TextLayout::default()
            },
            paint: TextPaint::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextLayout {
    pub visibility: Visibility,
    #[serde(rename = "text-field")]
    pub field: TextField,
    /// Names in the caller's font catalog, in fallback order. No system lookup.
    #[serde(rename = "text-font")]
    pub fonts: Vec<String>,
    #[serde(rename = "text-size")]
    pub size: Number,
    #[serde(rename = "text-anchor")]
    pub anchor: TextAnchor,
    /// Offset in ems: positive x is right, positive y is down.
    #[serde(rename = "text-offset")]
    pub offset: [f32; 2],
    #[serde(rename = "text-line-height")]
    pub line_height: f32,
    #[serde(rename = "text-padding")]
    pub padding: f32,
    #[serde(rename = "text-allow-overlap")]
    pub allow_overlap: bool,
    #[serde(rename = "text-ignore-placement")]
    pub ignore_placement: bool,
}

impl Default for TextLayout {
    fn default() -> Self {
        Self {
            visibility: Visibility::Visible,
            field: TextField::default(),
            fonts: Vec::new(),
            size: 16.0.into(),
            anchor: TextAnchor::Center,
            offset: [0.0, 0.0],
            line_height: 1.2,
            padding: 2.0,
            allow_overlap: false,
            ignore_placement: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TextPaint {
    #[serde(rename = "text-color")]
    pub color: Rgb,
    #[serde(rename = "text-opacity")]
    pub opacity: Number,
    #[serde(rename = "text-halo-color")]
    pub halo_color: Rgb,
    #[serde(rename = "text-halo-width")]
    pub halo_width: Number,
}

impl Default for TextPaint {
    fn default() -> Self {
        Self {
            color: Rgb::BLACK,
            opacity: 1.0.into(),
            halo_color: Rgb::new(255, 255, 255),
            halo_width: 0.0.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextAnchor {
    #[default]
    Center,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl TextAnchor {
    /// Position of the feature inside the label's box, measured from its top left.
    pub fn fractions(self) -> [f32; 2] {
        match self {
            Self::Center => [0.5, 0.5],
            Self::Left => [0.0, 0.5],
            Self::Right => [1.0, 0.5],
            Self::Top => [0.5, 0.0],
            Self::Bottom => [0.5, 1.0],
            Self::TopLeft => [0.0, 0.0],
            Self::TopRight => [1.0, 0.0],
            Self::BottomLeft => [0.0, 1.0],
            Self::BottomRight => [1.0, 1.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FeatureValue, GeometryType, Layer};
    use serde_json::json;

    #[test]
    fn text_fields_share_expression_semantics_with_filters() {
        struct Place;
        impl Feature for Place {
            fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
                match key {
                    "name" => Some(FeatureValue::String("東京")),
                    "offset" => Some(FeatureValue::Number(9.0)),
                    _ => None,
                }
            }
            fn id(&self) -> Option<FeatureValue<'_>> {
                None
            }
            fn geometry_type(&self) -> GeometryType {
                GeometryType::Point
            }
        }
        let field: TextField = serde_json::from_value(json!([
            "concat",
            ["get", "name"],
            " UTC+",
            ["to-string", ["get", "offset"]]
        ]))
        .unwrap();
        assert_eq!(field.evaluate(&Place).unwrap(), "東京 UTC+9");
        assert_eq!(TextField::property("missing").evaluate(&Place).unwrap(), "");
        let case: TextField = serde_json::from_value(json!([
            "case",
            ["has", "missing"],
            ["get", "missing"],
            ["coalesce", ["get", "name"], "fallback"]
        ]))
        .unwrap();
        assert_eq!(case.evaluate(&Place).unwrap(), "東京");
        let invalid: TextField = serde_json::from_value(json!(["get", "offset"])).unwrap();
        assert!(invalid.evaluate(&Place).is_err());
        assert_eq!(
            serde_json::from_value::<TextField>(serde_json::to_value(&field).unwrap()).unwrap(),
            field
        );
        assert!(
            serde_json::from_value::<TextField>(json!(["case", true, "yes", false, "no"])).is_err()
        );
        let mut text = TextLayer::new("names", "places", case);
        text.layout.fonts = vec!["Noto Sans".into()];
        let layer = Layer::Text(text);
        assert_eq!(serde_json::to_value(&layer).unwrap()["type"], "text");
        assert_eq!(
            serde_json::from_value::<Layer>(serde_json::to_value(&layer).unwrap()).unwrap(),
            layer
        );
    }
}

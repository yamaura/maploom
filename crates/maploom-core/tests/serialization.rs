use maploom_core::{
    Feature, FeatureValue, Filter, GeometryType, Layer, LineCap, LineLayer, Number, Rgb, Visibility,
};
use serde_json::json;

#[test]
fn native_layer_serializes_directly_to_maploom_json() {
    let mut line = LineLayer::new("roads", "transportation");
    line.source = Some("base".into());
    line.paint.color = Rgb::new(51, 102, 153);
    line.paint.width = 2.0.into();
    line.layout.cap = LineCap::Round;
    line.minzoom = Some(5.0);
    line.filter =
        Some(serde_json::from_value::<Filter>(json!(["==", ["get", "kind"], "road"])).unwrap());
    let layer = Layer::Line(line);
    let json = serde_json::to_value(&layer).unwrap();
    assert_eq!(json["type"], "line");
    assert_eq!(json["id"], "roads");
    assert_eq!(json["source"], "base");
    assert_eq!(json["source-layer"], "transportation");
    assert_eq!(json["paint"]["line-color"], "#336699");
    assert_eq!(json["paint"]["line-width"], 2.0);
    assert_eq!(json["layout"]["line-cap"], "round");
    assert_eq!(json["minzoom"], 5.0);
    assert_eq!(serde_json::from_value::<Layer>(json).unwrap(), layer);
}

#[test]
fn reads_standard_layer_defaults_and_short_hex_colors() {
    let Layer::Fill(fill) = serde_json::from_value(json!({
        "id":"land", "type":"fill", "source-layer":"land", "paint":{"fill-color":"#abc"}
    }))
    .unwrap() else {
        panic!("expected fill");
    };
    assert_eq!(fill.paint.color, Rgb::new(170, 187, 204));
    assert_eq!(fill.paint.opacity.evaluate(5.0), Ok(1.0));
    assert_eq!(fill.layout.visibility, Visibility::Visible);
    assert_eq!(fill.source, None);
}

#[test]
fn rejects_unknown_mismatched_and_flat_properties() {
    for invalid in [
        json!({"id":"x", "type":"fill", "source-layer":"x", "paint":{"line-width":2}}),
        json!({"id":"x", "type":"line", "source-layer":"x", "paint":{"line-pattern":"road"}}),
        json!({"id":"x", "type":"line", "source-layer":"x", "layout":{"line-cap":"invalid"}}),
        json!({"id":"x", "type":"line", "source-layer":"x", "line-width":2}),
        json!({"id":"x", "type":"line", "source-layer":"x", "unknown":1}),
        json!({"id":"x", "type":"symbol", "source-layer":"x"}),
        json!({"type":"line", "source-layer":"x"}),
        json!({"id":"x", "type":"line"}),
    ] {
        assert!(
            serde_json::from_value::<Layer>(invalid.clone()).is_err(),
            "accepted {invalid}"
        );
    }
}

#[test]
fn rejects_malformed_filters_when_reading_layers() {
    for filter in [json!(["unknown", 1]), json!(["get"]), json!(["==", 1])] {
        assert!(
            serde_json::from_value::<Layer>(json!({
                "id": "roads", "type": "line", "source-layer": "roads", "filter": filter
            }))
            .is_err()
        );
    }
}

struct EmptyFeature;

impl Feature for EmptyFeature {
    fn property(&self, _: &str) -> Option<FeatureValue<'_>> {
        None
    }
    fn id(&self) -> Option<FeatureValue<'_>> {
        None
    }
    fn geometry_type(&self) -> GeometryType {
        GeometryType::Unknown
    }
}

#[test]
fn core_deserializes_serde_values_without_json() {
    use serde::Deserialize;
    use serde::de::value::{BoolDeserializer, Error, F64Deserializer};

    let filter = Filter::deserialize(BoolDeserializer::<Error>::new(true)).unwrap();
    assert!(filter.evaluate(&EmptyFeature).unwrap());
    let number = Number::deserialize(F64Deserializer::<Error>::new(2.5)).unwrap();
    assert_eq!(number.evaluate(5.0), Ok(2.5));
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Filter::deserialize(F64Deserializer::<Error>::new(value)).is_err());
        assert!(Number::deserialize(F64Deserializer::<Error>::new(value)).is_err());
    }
}

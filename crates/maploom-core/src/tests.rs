use super::*;
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
fn evaluates_and_round_trips_zoom_expressions() {
    let linear = Number::Linear {
        stops: vec![(4.0, 1.0), (10.0, 4.0)],
    };
    assert_eq!(linear.evaluate(3.0), Ok(1.0));
    assert_eq!(linear.evaluate(7.0), Ok(2.5));
    assert_eq!(linear.evaluate(11.0), Ok(4.0));
    assert_eq!(
        serde_json::to_value(&linear).unwrap(),
        json!(["interpolate", ["linear"], ["zoom"], 4.0, 1.0, 10.0, 4.0])
    );
    let step = Number::Step {
        default: 1.0,
        stops: vec![(4.0, 2.0), (10.0, 3.0)],
    };
    assert_eq!(step.evaluate(3.0), Ok(1.0));
    assert_eq!(step.evaluate(4.0), Ok(2.0));
    assert_eq!(step.evaluate(10.0), Ok(3.0));
    for value in [linear, step] {
        assert_eq!(
            serde_json::from_value::<Number>(serde_json::to_value(&value).unwrap()).unwrap(),
            value
        );
    }
}

#[test]
fn invalid_native_numbers_and_colors_are_rejected() {
    for value in [
        Number::Constant(f64::NAN),
        Number::Linear { stops: vec![] },
        Number::Linear {
            stops: vec![(4.0, 1.0), (4.0, 2.0)],
        },
        Number::Step {
            default: 1.0,
            stops: vec![(8.0, 1.0), (4.0, 2.0)],
        },
    ] {
        assert!(value.evaluate(5.0).is_err());
        assert!(serde_json::to_value(&value).is_err());
    }
    for color in ["#a日ab", "transparent", "red", "#abcd", "#gggggg"] {
        assert!(color.parse::<Rgb>().is_err());
    }
}

#[test]
fn unsupported_or_malformed_numeric_expressions_fail_at_deserialization() {
    for value in [
        json!(["interpolate", ["exponential", 2], ["zoom"], 0, 1, 10, 4]),
        json!(["step", ["zoom"], 1, 2]),
        json!(["step", ["zoom"], 1, 8, 1, 4, 2]),
        json!(["get", "width"]),
    ] {
        assert!(serde_json::from_value::<Number>(value).is_err());
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

#[test]
fn filters_round_trip_without_changing_expression_semantics() {
    for value in [
        json!(["==", ["get", "kind"], "road"]),
        json!(["!", ["in", ["geometry-type"], ["literal", ["Polygon"]]]]),
        json!(["!", ["any", ["has", "hidden"]]]),
        json!(["==", ["literal", "road"], "road"]),
        json!(["in", ["get", "kind"], ["literal", ["road", "rail"]]]),
        json!(["match", ["get", "kind"], ["road", "rail"], true, false]),
        json!(["!", ["has", "hidden"]]),
        json!(["==", ["coalesce", ["get", "kind"], "road"], "road"]),
        json!([">", ["to-number", ["get", "rank"], 0], 2]),
        json!(["==", ["%", ["get", "code"], 10000], 101]),
    ] {
        let filter = serde_json::from_value::<Filter>(value.clone()).unwrap();
        let serialized = serde_json::to_value(&filter).unwrap();
        let restored: Filter = serde_json::from_value(serialized).unwrap();
        assert_eq!(restored, filter, "changed {value}");
    }
}

#[test]
fn filters_evaluate_custom_features_without_backend_dependencies() {
    struct Road;
    impl Feature for Road {
        fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
            (key == "kind").then_some(FeatureValue::String("road"))
        }
        fn id(&self) -> Option<FeatureValue<'_>> {
            None
        }
        fn geometry_type(&self) -> GeometryType {
            GeometryType::LineString
        }
    }
    let filter: Filter = serde_json::from_value(json!([
        "all",
        ["==", ["get", "kind"], "road"],
        ["==", ["geometry-type"], "LineString"]
    ]))
    .unwrap();
    assert!(filter.evaluate(&Road).unwrap());
}

#[test]
fn core_expressions_use_literal_strings_and_require_modern_operators() {
    let filter = serde_json::from_value::<Filter>(json!(["==", "road", "road"])).unwrap();
    let json = serde_json::to_value(&filter).unwrap();
    assert_eq!(json, json!(["==", ["literal", "road"], "road"]));
    for unsupported in [
        json!(["!has", "kind"]),
        json!(["none", true]),
        json!(["!in", "kind", "road"]),
    ] {
        assert!(serde_json::from_value::<Filter>(unsupported).is_err());
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
fn match_streams_operator_named_labels_and_expression_fallbacks() {
    for (source, expected) in [
        (r#"["match", "get", ["get", "kind"], true, false]"#, true),
        (
            r#"["match", "other", ["get", "kind"], false, ["==", 1, 1]]"#,
            true,
        ),
        (
            r#"["match", "other", "road", true, ["get", "missing"]]"#,
            false,
        ),
        (
            r#"["match", "other", "road", false, ["!", ["has", "missing"]]]"#,
            true,
        ),
        (
            r#"["match", "other", "road", false, ["literal", true]]"#,
            true,
        ),
        (
            r#"["match", "other", "road", false, ["to-boolean", ["literal", [1]]]]"#,
            true,
        ),
    ] {
        let filter: Filter = serde_json::from_str(source).unwrap();
        assert_eq!(filter.matches(&EmptyFeature), expected, "{source}");
        let encoded = serde_json::to_string(&filter).unwrap();
        assert_eq!(serde_json::from_str::<Filter>(&encoded).unwrap(), filter);
    }
}

#[test]
fn literal_collections_preserve_nested_data() {
    let source = r#"["==", ["literal", {"items": ["get", null, true, 2]}],
        ["literal", {"items": ["get", null, true, 2]}]]"#;
    let filter: Filter = serde_json::from_str(source).unwrap();
    assert!(filter.evaluate(&EmptyFeature).unwrap());
    let encoded = serde_json::to_string(&filter).unwrap();
    assert_eq!(serde_json::from_str::<Filter>(&encoded).unwrap(), filter);
}

#[test]
fn malformed_expression_streams_are_rejected() {
    for source in [
        r#"[]"#,
        r#"["get"]"#,
        r#"["get", "kind", 1]"#,
        r#"["id", 1]"#,
        r#"["literal", 1, 2]"#,
        r#"["==", 1]"#,
        r#"["!", true, false]"#,
        r#"["match", "road", true]"#,
        r#"["match", "road", "road", true]"#,
        r#"["match", "road", "road", true, ["get", "kind", 1]]"#,
    ] {
        assert!(
            serde_json::from_str::<Filter>(source).is_err(),
            "accepted {source}"
        );
    }
    for source in [
        r#"["step", ["zoom", 1], 1, 2, 3]"#,
        r#"["step", ["zoom"], 1, 2]"#,
        r#"["interpolate", ["linear", 1], ["zoom"], 0, 1, 2, 3]"#,
        r#"["interpolate", ["linear"], ["get", "zoom"], 0, 1, 2, 3]"#,
        r#"["interpolate", ["linear"], ["zoom"], 0, 1, 2]"#,
    ] {
        assert!(
            serde_json::from_str::<Number>(source).is_err(),
            "accepted {source}"
        );
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

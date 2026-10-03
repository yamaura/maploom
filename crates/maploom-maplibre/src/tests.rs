use super::*;
use maploom_core::{Feature, FeatureValue, GeometryType, LineLayer};

struct Road {
    kind: &'static str,
    id: Option<f64>,
}

impl Feature for Road {
    fn property(&self, key: &str) -> Option<FeatureValue<'_>> {
        (key == "kind").then_some(FeatureValue::String(self.kind))
    }
    fn id(&self) -> Option<FeatureValue<'_>> {
        self.id.map(FeatureValue::Number)
    }
    fn geometry_type(&self) -> GeometryType {
        GeometryType::LineString
    }
}

#[test]
fn legacy_membership_uses_exact_values_even_with_one_candidate() {
    let filter = from_filter(&json!(["in", "kind", "road"])).unwrap();
    assert!(filter.matches(&Road {
        kind: "road",
        id: None
    }));
    assert!(!filter.matches(&Road {
        kind: "ro",
        id: None
    }));
    assert_eq!(
        to_filter(&filter).unwrap(),
        json!(["in", ["get", "kind"], ["literal", ["road"]]])
    );
    assert!(
        !from_filter(&json!(["in", "kind"])).unwrap().matches(&Road {
            kind: "road",
            id: None
        })
    );
}

#[test]
fn explicit_expressions_keep_string_literals() {
    let filter = from_expression(&json!(["in", "ro", "road"])).unwrap();
    assert!(filter.matches(&Road {
        kind: "rail",
        id: None
    }));
    let equal = from_expression(&json!(["==", "road", "road"])).unwrap();
    assert!(equal.matches(&Road {
        kind: "rail",
        id: None
    }));
    assert_eq!(from_filter(&to_filter(&equal).unwrap()).unwrap(), equal);
}

#[test]
fn translates_nested_legacy_filters_and_special_keys() {
    let filter = from_filter(&json!([
        "all",
        ["==", "kind", "road"],
        ["==", "$type", "LineString"],
        ["has", "$id"],
        ["!in", "$id", 2, 3],
        ["none", ["!has", "kind"], ["==", "$id", 4]]
    ]))
    .unwrap();
    assert!(filter.matches(&Road {
        kind: "road",
        id: Some(1.0)
    }));
    assert!(!filter.matches(&Road {
        kind: "road",
        id: None
    }));
    assert!(!filter.matches(&Road {
        kind: "road",
        id: Some(2.0)
    }));
    assert_eq!(from_filter(&to_filter(&filter).unwrap()).unwrap(), filter);
    assert!(
        from_filter(&json!(["has", "$type"]))
            .unwrap()
            .matches(&Road {
                kind: "road",
                id: None
            })
    );
}

#[test]
fn literals_and_match_labels_are_not_rewritten_as_operators() {
    let filter = from_expression(&json!([
        "match",
        ["get", "kind"],
        ["road", "rail"],
        ["in", "road", ["literal", ["road", "rail"]]],
        false
    ]))
    .unwrap();
    assert!(filter.matches(&Road {
        kind: "road",
        id: None
    }));
    assert_eq!(from_filter(&to_filter(&filter).unwrap()).unwrap(), filter);
}

#[test]
fn mixed_filter_syntax_is_rejected() {
    assert!(
        from_filter(&json!([
            "all",
            ["==", ["get", "kind"], "road"],
            ["!has", "hidden"]
        ]))
        .is_err()
    );
    assert!(
        from_filter(&json!([
            "all",
            ["==", ["get", "kind"], "road"],
            ["in", "kind", "road"]
        ]))
        .is_err()
    );
}

#[test]
fn modern_tree_does_not_turn_literal_comparisons_into_property_tests() {
    let filter = from_filter(&json!([
        "all",
        ["==", ["get", "kind"], "road"],
        ["==", ["literal", "road"], "road"]
    ]))
    .unwrap();
    assert!(filter.matches(&Road {
        kind: "road",
        id: None
    }));
}

#[test]
fn rejects_unsupported_and_malformed_expressions() {
    for value in [
        json!(["within", {}]),
        json!(["get"]),
        json!(["!has", "kind"]),
        json!(["/", 6, 2, 3]),
        json!(["number"]),
        json!(["==", ["get", "kind"], "road", ["collator", {}]]),
        json!(["match", ["get", "kind"], [], true, false]),
        json!(["literal", 1, 2]),
    ] {
        assert!(from_expression(&value).is_err(), "accepted {value}");
    }
    for value in [json!(["==", "kind"]), json!(["in", "kind", {}]), json!([])] {
        assert!(from_filter(&value).is_err(), "accepted {value}");
    }
}

#[test]
fn imports_layers_in_order_and_normalizes_their_filters() {
    let layers = from_layers(&json!([
        {"id": "land", "type": "fill", "source": "base", "source-layer": "land"},
        {"id": "roads", "type": "line", "source": "base", "source-layer": "roads",
         "filter": ["in", "kind", "road"],
         "paint": {"line-width": ["step", ["zoom"], 1, 5, 2]}}
    ]))
    .unwrap();
    assert_eq!(layers[0].id(), "land");
    assert_eq!(layers[1].id(), "roads");
    assert!(layers[1].filter().unwrap().matches(&Road {
        kind: "road",
        id: None
    }));
    assert_eq!(from_layers(&to_layers(&layers).unwrap()).unwrap(), layers);
}

#[test]
fn invalid_layers_are_errors_with_their_index() {
    let error = from_layers(&json!([
        {"id": "land", "type": "fill", "source": "base", "source-layer": "land"},
        {"id": "labels", "type": "symbol", "source": "base", "source-layer": "labels"}
    ]))
    .unwrap_err();
    assert!(error.to_string().contains("layers[1]"));
    let mut layer = LineLayer::new("roads", "roads");
    assert!(to_layer(&Layer::Line(layer.clone())).is_err());
    layer.source = Some("base".into());
    layer.minzoom = Some(25.0);
    assert!(to_layer(&Layer::Line(layer)).is_err());
    assert!(from_layer(&json!({"id": "land", "type": "fill", "source": "base", "source-layer": "land", "paint": {"fill-pattern": "grass"}})).is_err());
}

#[test]
fn null_filter_is_unrestricted() {
    assert!(from_filter(&Value::Null).unwrap().matches(&Road {
        kind: "road",
        id: None
    }));
    let layer = from_layer(&json!({"id": "land", "type": "fill", "source": "base", "source-layer": "land", "filter": null})).unwrap();
    assert!(layer.filter().is_none());
}

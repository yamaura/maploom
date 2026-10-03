use super::*;
use fast_mvt::{
    DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtPoint, MvtReaderRef, MvtTile, MvtValue,
};
use maploom_core::Filter;
use serde_json::json;

#[test]
fn evaluates_expression_filter_against_mvt_feature() {
    let mut feature = point_feature();
    feature.add_tag("kind", MvtValue::from("station"));
    feature.add_tag("rank", MvtValue::from(3_u64));

    let filter = serde_json::from_value::<Filter>(json!([
        "all",
        ["==", ["get", "kind"], "station"],
        [">=", ["get", "rank"], 2],
        ["==", ["geometry-type"], "Point"]
    ]))
    .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn supports_property_and_geometry_filters() {
    let mut feature = point_feature();
    feature.add_tag("class", MvtValue::from("street_major"));
    feature.add_tag("admin_level", MvtValue::from(4_i64));

    let filter = serde_json::from_value::<Filter>(json!([
        "all",
        ["==", ["get", "class"], "street_major"],
        [">=", ["get", "admin_level"], 3],
        ["!", ["in", ["geometry-type"], ["literal", ["Polygon"]]]]
    ]))
    .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn supports_expression_in_with_literal_arrays() {
    let mut feature = point_feature();
    feature.add_tag("nature", MvtValue::from("road"));

    let filter = serde_json::from_value::<Filter>(json!([
        "in",
        ["get", "nature"],
        ["literal", ["road", "highway"]]
    ]))
    .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn supports_negation_and_has() {
    let mut feature = point_feature();
    feature.add_tag("hidden", MvtValue::from(false));

    let filter = serde_json::from_value::<Filter>(json!([
        "!",
        ["any", ["has", "missing"], ["==", ["get", "hidden"], true]]
    ]))
    .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn strict_equality_does_not_match_different_types() {
    let mut feature = point_feature();
    feature.add_tag("rank", MvtValue::from(3_u64));

    let filter = serde_json::from_value::<Filter>(json!(["==", ["get", "rank"], "3"])).unwrap();

    assert!(!filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn ordered_comparison_reports_runtime_type_errors() {
    let mut feature = point_feature();
    feature.add_tag("rank", MvtValue::from("3"));

    let filter = serde_json::from_value::<Filter>(json!([">", ["get", "rank"], 2])).unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).is_err());
    assert!(!filter.matches(&FastMvtFeature(&feature)));
}

#[test]
fn supports_math_used_inside_filters() {
    let mut feature = point_feature();
    feature.add_tag("code", MvtValue::from(10101_u64));

    let filter =
        serde_json::from_value::<Filter>(json!(["==", ["%", ["get", "code"], 10000], 101]))
            .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(&feature)).unwrap());
}

#[test]
fn evaluates_borrowed_mvt_feature_refs() {
    let mut feature = point_feature();
    feature.id = Some(42);
    feature.add_tag("kind", MvtValue::from("station"));
    let bytes = MvtTile {
        layers: vec![MvtLayer {
            name: "poi".to_owned(),
            extent: DEFAULT_EXTENT,
            features: vec![feature],
        }],
    }
    .encode()
    .unwrap();
    let reader = MvtReaderRef::new(&bytes).unwrap();
    let layer = reader.layers().next().unwrap();
    let feature = layer.features().next().unwrap();

    let filter = serde_json::from_value::<Filter>(json!([
        "all",
        ["==", ["id"], 42],
        ["==", ["get", "kind"], "station"]
    ]))
    .unwrap();

    assert!(filter.evaluate(&FastMvtFeature(feature)).unwrap());
}

#[test]
fn rejects_unimplemented_within() {
    let error =
        serde_json::from_value::<Filter>(json!(["within", {"type": "Polygon", "coordinates": []}]))
            .unwrap_err();

    assert!(error.to_string().contains("within"));
}

fn point_feature() -> MvtFeature {
    MvtFeature::new(MvtGeometry::Point(MvtPoint::new(1, 2)))
}

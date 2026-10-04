mod common;

use common::render_chunk;
use fast_mvt::{DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtTile, MvtValue};
use geo_types::{line_string, polygon};
use maploom_pdf::*;
use pdf_writer::{Chunk, Ref};
use serde_json::json;

#[test]
fn named_source_does_not_draw_same_layer_from_other_source() {
    let tile = encode_tile(vec![MvtLayer {
        name: "roads".into(),
        extent: DEFAULT_EXTENT,
        features: vec![MvtFeature::new(MvtGeometry::LineString(line_string![
            (x: 0, y: 0), (x: 4096, y: 4096),
        ]))],
    }]);
    let layers: Vec<Layer> = serde_json::from_value(
        json!([{"id": "roads-line", "type": "line", "source-layer": "roads", "source": "wanted"}]),
    )
    .unwrap();
    let pdf = render_chunk(
        MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0),
        &layers,
        &[
            MvtTileInput::new(0, 0, 0, tile.as_slice()).with_source("wanted"),
            MvtTileInput::new(0, 0, 0, tile.as_slice()).with_source("other"),
        ],
    )
    .unwrap();
    assert_eq!(String::from_utf8_lossy(&pdf).matches("\nS").count(), 1);
}

#[test]
fn malformed_styles_fail_without_panics_or_silent_interpolation() {
    for paint in [
        json!({"line-color":"#a日ab"}),
        json!({"line-color":"transparent"}),
        json!({"line-width":["interpolate",["exponential",2],["zoom"],0,1,10,4]}),
        json!({"line-width":["step",["zoom"],1,2]}),
    ] {
        let value = json!({"id":"roads", "type":"line", "source-layer":"roads", "paint":paint});
        assert!(serde_json::from_value::<Layer>(value).is_err());
    }
    for paint in [
        LinePaint {
            width: (-1.0).into(),
            ..Default::default()
        },
        LinePaint {
            width: f64::NAN.into(),
            ..Default::default()
        },
        LinePaint {
            opacity: 2.0.into(),
            ..Default::default()
        },
        LinePaint {
            dasharray: Some(vec![0.0, 0.0]),
            ..Default::default()
        },
        LinePaint {
            dasharray: Some(vec![1.0, -1.0]),
            ..Default::default()
        },
    ] {
        let mut line = LineLayer::new("roads", "roads");
        line.paint = paint;
        assert!(matches!(
            render_chunk(
                MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0),
                &[Layer::Line(line)],
                &[]
            ),
            Err(PdfRenderError::InvalidLayer { index: 0, .. })
        ));
    }
}

#[test]
fn renders_mvt_fill_and_line_as_pdf_vectors() {
    let tile = encode_tile(vec![
        MvtLayer {
            name: "land".to_owned(),
            extent: DEFAULT_EXTENT,
            features: vec![MvtFeature::new(MvtGeometry::Polygon(polygon![
                (x: 1024, y: 1024),
                (x: 3072, y: 1024),
                (x: 3072, y: 3072),
                (x: 1024, y: 3072),
                (x: 1024, y: 1024),
            ]))],
        },
        MvtLayer {
            name: "roads".to_owned(),
            extent: DEFAULT_EXTENT,
            features: vec![MvtFeature::new(MvtGeometry::LineString(line_string![
                (x: 0, y: 0),
                (x: 4096, y: 4096),
            ]))],
        },
    ]);
    let layers = serde_json::from_value::<Vec<Layer>>(json!([{"id": "land-fill", "type": "fill", "source-layer": "land", "source": "boundary", "paint": {"fill-color": "#eef2ed", "fill-opacity": 0.75}}, {"id": "roads-line", "type": "line", "source-layer": "roads", "source": "boundary", "paint": {"line-color": "#3976a3", "line-width": 2}}]))
    .unwrap();

    let pdf = render_chunk(
        MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0),
        &layers,
        &[MvtTileInput::new(0, 0, 0, tile).with_source("boundary")],
    )
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("50 150 m"));
    assert!(text.contains("150 50 l"));
    assert!(text.contains("f*"));
    assert!(text.contains("\nS"));
    assert!(text.contains("/CA 0.75"));
}

#[test]
fn applies_maploom_filters() {
    let mut included = MvtFeature::new(MvtGeometry::LineString(line_string![
        (x: 0, y: 0),
        (x: 4096, y: 0),
    ]));
    included.add_tag("kind", MvtValue::from("primary"));
    let mut excluded = MvtFeature::new(MvtGeometry::LineString(line_string![
        (x: 0, y: 4096),
        (x: 4096, y: 4096),
    ]));
    excluded.add_tag("kind", MvtValue::from("minor"));
    let tile = encode_tile(vec![MvtLayer {
        name: "road".to_owned(),
        extent: DEFAULT_EXTENT,
        features: vec![included, excluded],
    }]);
    let layers = serde_json::from_value::<Vec<Layer>>(json!([{"id": "road-line", "type": "line", "source-layer": "road", "filter": ["==", ["get", "kind"], "primary"], "paint": {"line-color": "#000000", "line-width": 1}}]))
    .unwrap();

    let pdf = render_chunk(
        MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0),
        &layers,
        &[MvtTileInput::new(0, 0, 0, tile)],
    )
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("200 200 l"));
    assert!(!text.contains("0 0 m"));
}

#[test]
fn evaluates_zoom_interpolated_line_width() {
    let tile = encode_tile(vec![MvtLayer {
        name: "road".to_owned(),
        extent: DEFAULT_EXTENT,
        features: vec![MvtFeature::new(MvtGeometry::LineString(line_string![
            (x: 0, y: 0),
            (x: 4096, y: 4096),
        ]))],
    }]);
    let layers = serde_json::from_value::<Vec<Layer>>(json!([{"id": "road-line", "type": "line", "source-layer": "road", "paint": {"line-width": ["interpolate", ["linear"], ["zoom"], 4, 1, 10, 4]}}]))
    .unwrap();

    let pdf = render_chunk(
        MapViewport::new(200.0, 200.0, LngLatBounds::world(), 7.0),
        &layers,
        &[MvtTileInput::new(0, 0, 0, tile)],
    )
    .unwrap();
    let text = String::from_utf8_lossy(&pdf);

    assert!(text.contains("2.5 w"));
}

#[test]
fn clips_buffered_geometry_to_each_tile_before_painting() {
    let tile = encode_tile(vec![MvtLayer {
        name: "land".into(),
        extent: DEFAULT_EXTENT,
        features: vec![MvtFeature::new(MvtGeometry::Polygon(polygon![
            (x: -256, y: -256), (x: 4352, y: -256),
            (x: 4352, y: 4352), (x: -256, y: 4352), (x: -256, y: -256),
        ]))],
    }]);
    let layers: Vec<Layer> = serde_json::from_value(json!([{
        "id":"land", "type":"fill", "source-layer":"land",
        "paint":{"fill-opacity":0.5}
    }]))
    .unwrap();
    let bytes = render_chunk(
        MapViewport::new(200.0, 200.0, LngLatBounds::world(), 1.0),
        &layers,
        &[
            MvtTileInput::new(1, 0, 0, tile.clone()),
            MvtTileInput::new(1, 1, 0, tile),
        ],
    )
    .unwrap();
    let pdf = String::from_utf8_lossy(&bytes);
    let clips: Vec<Vec<f32>> = pdf
        .lines()
        .filter_map(|line| line.strip_suffix(" re"))
        .map(|line| {
            line.split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect()
        })
        .collect();
    assert_eq!(clips.len(), 2);
    assert!(clips[0][0].abs() < 0.001);
    assert_eq!(&clips[0][1..], &[100.0, 100.0, 100.0]);
    assert_eq!(pdf.matches("re\nW\nn\n").count(), 2);
    assert!(pdf.contains("q\n100 100 100 100 re\nW\nn\n"), "{pdf}");
    assert_eq!(pdf.matches("f*\nQ").count(), 2);
    assert!(pdf.contains("/ca 0.5"));
}

#[test]
fn validates_hidden_and_out_of_zoom_paints_before_accepting_a_style() {
    for layer in [
        json!({"id":"land", "type":"fill", "source-layer":"land", "minzoom":10,
            "paint":{"fill-opacity":["step",["zoom"],0.5,12,1.5]}}),
        json!({"id":"road", "type":"line", "source-layer":"road", "layout":{"visibility":"none"},
            "paint":{"line-width":-2}}),
        json!({"id":"road", "type":"line", "source-layer":"road",
            "paint":{"line-width":["interpolate",["linear"],["zoom"],0,1,10,-1]}}),
    ] {
        let layer = serde_json::from_value::<Layer>(layer).unwrap();
        assert!(matches!(
            validate_layers(std::slice::from_ref(&layer)),
            Err(PdfRenderError::InvalidLayer { index: 0, .. })
        ));
        let mut chunk = Chunk::new();
        assert!(
            write_map(
                &mut chunk,
                Ref::new(1),
                MapViewport::new(100.0, 100.0, LngLatBounds::world(), 0.0),
                &[layer],
                &TileSet::default()
            )
            .is_err()
        );
        assert!(chunk.as_bytes().is_empty());
    }
}

fn encode_tile(layers: Vec<MvtLayer>) -> Vec<u8> {
    MvtTile { layers }.encode().unwrap()
}

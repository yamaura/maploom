use super::*;
use crate::style::line_style;
use fast_mvt::{DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtTile, MvtValue};
use geo_types::{line_string, polygon};
use serde_json::json;

#[test]
fn typed_layers_preserve_zoom_boundaries_and_visibility() {
    let mut line = LineLayer::new("roads", "roads");
    line.minzoom = Some(4.0);
    line.maxzoom = Some(10.0);
    let layers = [Layer::Line(line.clone())];
    assert_eq!(compile_layers(&layers, 3.0).unwrap().len(), 0);
    assert_eq!(compile_layers(&layers, 4.0).unwrap().len(), 1);
    assert_eq!(compile_layers(&layers, 9.0).unwrap().len(), 1);
    assert_eq!(compile_layers(&layers, 10.0).unwrap().len(), 0);
    line.layout.visibility = Visibility::None;
    assert!(
        compile_layers(&[Layer::Line(line.clone())], 5.0)
            .unwrap()
            .is_empty()
    );
    line.minzoom = Some(f64::NAN);
    assert!(matches!(
        compile_layers(&[Layer::Line(line)], 5.0),
        Err(PdfRenderError::InvalidLayer { index: 0, .. })
    ));
}

#[test]
fn writes_one_self_contained_object_without_taking_over_the_document() {
    let mut chunk = Chunk::new();
    chunk.indirect(Ref::new(1)).primitive(42);
    let original = chunk.as_bytes().to_vec();
    let layer: Layer = serde_json::from_value(json!({"id": "land-fill", "type": "fill", "source-layer": "land", "paint": {"fill-opacity": 0.25}}))
    .unwrap();
    let map = write_map(
        &mut chunk,
        Ref::new(77),
        MapViewport::new(200.0, 100.0, LngLatBounds::world(), 0.0),
        &[layer],
        &TileSet::default(),
    )
    .unwrap();
    assert_eq!(map.id(), Ref::new(77));
    assert_eq!(
        chunk.refs().collect::<Vec<_>>(),
        vec![Ref::new(1), Ref::new(77)]
    );
    assert!(chunk.as_bytes().starts_with(&original));
    let text = String::from_utf8_lossy(chunk.as_bytes());
    assert!(text.contains("/Subtype /Form"));
    assert!(text.contains("/BBox [0 0 200 100]"));
    assert!(text.contains("/Resources <<"));
    assert!(text.contains("/ExtGState <<"));
    assert!(text.contains("/ca 0.25"));
    assert!(!text.contains("/Catalog"));
    assert!(!text.contains("/Pages"));
    assert!(!text.contains("/OCProperties"));
}

#[test]
fn failed_render_does_not_mutate_the_callers_chunk() {
    let mut chunk = Chunk::new();
    chunk.indirect(Ref::new(1)).primitive(42);
    let original = chunk.as_bytes().to_vec();
    let layer: Layer =
        serde_json::from_value(json!({"id": "roads-line", "type": "line", "source-layer": "roads", "paint": {"line-width": -1}}))
            .unwrap();
    assert!(
        write_map(
            &mut chunk,
            Ref::new(2),
            MapViewport::new(200.0, 100.0, LngLatBounds::world(), 0.0),
            &[layer],
            &TileSet::default()
        )
        .is_err()
    );
    assert_eq!(chunk.as_bytes(), original);
}

#[test]
fn placement_preserves_aspect_ratio_and_graphics_state() {
    let mut chunk = Chunk::new();
    let map = write_map(
        &mut chunk,
        Ref::new(1),
        MapViewport::new(200.0, 100.0, LngLatBounds::world(), 0.0),
        &[],
        &TileSet::default(),
    )
    .unwrap();
    let mut content = Content::new();
    content.set_line_width(7.0);
    map.draw(
        &mut content,
        Name(b"Map"),
        Rect::new(50.0, 100.0, 350.0, 200.0),
    )
    .unwrap();
    let bytes = content.finish();
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "7 w\nq\n1 0 0 1 100 100 cm\n/Map Do\nQ"
    );
}

#[test]
fn invalid_placement_writes_no_content() {
    let mut chunk = Chunk::new();
    let map = write_map(
        &mut chunk,
        Ref::new(1),
        MapViewport::new(200.0, 100.0, LngLatBounds::world(), 0.0),
        &[],
        &TileSet::default(),
    )
    .unwrap();
    for area in [
        Rect::new(0.0, 0.0, 0.0, 100.0),
        Rect::new(10.0, 0.0, 0.0, 100.0),
        Rect::new(f32::NAN, 0.0, 100.0, 100.0),
    ] {
        let mut content = Content::new();
        assert!(matches!(
            map.draw(&mut content, Name(b"Map"), area),
            Err(PdfRenderError::InvalidArea { .. })
        ));
        assert!(content.finish().is_empty());
    }
}

#[test]
fn application_can_wrap_reused_maps_in_optional_content() {
    let mut pdf = pdf_writer::Pdf::new();
    let view = MapViewport::new(200.0, 100.0, LngLatBounds::world(), 0.0);
    let tiles = TileSet::default();
    let first = write_map(&mut pdf, Ref::new(10), view, &[], &tiles).unwrap();
    let second = write_map(&mut pdf, Ref::new(11), view, &[], &tiles).unwrap();
    pdf.indirect(Ref::new(12))
        .dict()
        .pair(Name(b"Type"), Name(b"OCG"))
        .pair(Name(b"Name"), pdf_writer::TextStr("Roads"));
    {
        let mut catalog = pdf.catalog(Ref::new(1));
        catalog.pages(Ref::new(2));
        let mut oc = catalog.insert(Name(b"OCProperties")).dict();
        oc.insert(Name(b"OCGs")).array().item(Ref::new(12));
        oc.insert(Name(b"D"))
            .dict()
            .insert(Name(b"Order"))
            .array()
            .item(Ref::new(12));
    }
    pdf.pages(Ref::new(2))
        .kids([Ref::new(3), Ref::new(4)])
        .count(2);
    for (page_id, content_id) in [(Ref::new(3), Ref::new(5)), (Ref::new(4), Ref::new(6))] {
        {
            let mut page = pdf.page(page_id);
            page.parent(Ref::new(2))
                .media_box(Rect::new(0.0, 0.0, 500.0, 500.0))
                .contents(content_id);
            let mut resources = page.resources();
            resources
                .x_objects()
                .pair(Name(b"A"), first.id())
                .pair(Name(b"B"), second.id());
            resources
                .insert(Name(b"Properties"))
                .dict()
                .pair(Name(b"Roads"), Ref::new(12));
        }
        let mut content = Content::new();
        // Sharing an OCG must preserve nonadjacent placements and drawing order.
        for (map, name) in [(first, b"A"), (second, b"B"), (first, b"A")] {
            content
                .begin_marked_content_with_properties(Name(b"OC"))
                .properties_named(Name(b"Roads"));
            map.draw(
                &mut content,
                Name(name),
                Rect::new(25.0, 50.0, 225.0, 150.0),
            )
            .unwrap();
            content.end_marked_content();
        }
        let bytes = content.finish();
        let text = String::from_utf8_lossy(&bytes);
        assert_eq!(text.matches("/OC /Roads BDC").count(), 3);
        assert_eq!(text.matches("EMC").count(), 3);
        let a = text.find("/A Do").unwrap();
        let b = text.find("/B Do").unwrap();
        assert!(a < b && b < text.rfind("/A Do").unwrap());
        pdf.stream(content_id, &bytes);
    }
    let bytes = pdf.finish();
    let text = String::from_utf8_lossy(&bytes);
    assert_eq!(text.matches("/Subtype /Form").count(), 2);
    assert_eq!(text.matches("/Type /OCG").count(), 1);
    assert!(text.contains("/Count 2"));
}

#[test]
fn rejects_invalid_viewport_and_tile_coordinates() {
    let page = MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0);
    assert!(matches!(
        render_chunk(
            MapViewport {
                zoom: f64::NAN,
                ..page
            },
            &[],
            &[]
        ),
        Err(PdfRenderError::InvalidZoom)
    ));
    assert!(matches!(
        render_chunk(
            MapViewport {
                width_pt: 0.0,
                ..page
            },
            &[],
            &[]
        ),
        Err(PdfRenderError::InvalidViewportSize { .. })
    ));
    let bounds = LngLatBounds {
        north: f64::INFINITY,
        ..page.bounds
    };
    assert!(matches!(
        render_chunk(MapViewport { bounds, ..page }, &[], &[]),
        Err(PdfRenderError::InvalidBounds)
    ));
    assert!(matches!(
        render_chunk(page, &[], &[MvtTileInput::new(0, 1, 0, &[][..])]),
        Err(PdfRenderError::InvalidTileCoordinates { index: 0 })
    ));
    assert!(matches!(
        render_chunk(page, &[], &[MvtTileInput::new(0, 0, 0, &[255][..])]),
        Err(PdfRenderError::DecodeTile { index: 0, .. })
    ));
}

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
fn dashes_scale_by_width_and_unsupported_layers_are_errors() {
    let mut line = LineLayer::new("roads", "roads");
    line.paint.width = 3.0.into();
    line.paint.dasharray = Some(vec![2.0, 1.0]);
    assert_eq!(
        line_style(&line, 0.0).unwrap().dasharray,
        Some(vec![6.0, 3.0])
    );
    assert!(
        serde_json::from_value::<Layer>(
            json!({"id":"places", "type":"symbol", "source-layer":"places"})
        )
        .is_err()
    );
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
fn viewport_preserves_aspect_ratio() {
    let viewport =
        Viewport::new(MapViewport::new(300.0, 200.0, LngLatBounds::world(), 0.0)).unwrap();

    assert_point_near(viewport.point(WorldPoint { x: 0.0, y: 0.0 }), (50.0, 200.0));
    assert_point_near(viewport.point(WorldPoint { x: 1.0, y: 1.0 }), (250.0, 0.0));
}

fn encode_tile(layers: Vec<MvtLayer>) -> Vec<u8> {
    MvtTile { layers }.encode().unwrap()
}

fn assert_point_near(actual: (f32, f32), expected: (f32, f32)) {
    const EPSILON: f32 = 0.001;
    assert!(
        (actual.0 - expected.0).abs() <= EPSILON,
        "x mismatch: actual {}, expected {}",
        actual.0,
        expected.0
    );
    assert!(
        (actual.1 - expected.1).abs() <= EPSILON,
        "y mismatch: actual {}, expected {}",
        actual.1,
        expected.1
    );
}

fn render_chunk(
    view: MapViewport,
    layers: &[Layer],
    tiles: &[MvtTileInput<'_>],
) -> Result<Vec<u8>, PdfRenderError> {
    let mut chunk = Chunk::new();
    write_map(
        &mut chunk,
        Ref::new(100),
        view,
        layers,
        &TileSet::decode(tiles)?,
    )?;
    Ok(chunk.as_bytes().to_vec())
}

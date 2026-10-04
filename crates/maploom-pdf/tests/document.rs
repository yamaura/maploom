mod common;

use common::render_chunk;
use maploom_pdf::*;
use pdf_writer::{Chunk, Content, Name, Rect, Ref};
use serde_json::json;

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

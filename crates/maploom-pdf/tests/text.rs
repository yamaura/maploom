#![cfg(feature = "fonts")]

use fast_mvt::{DEFAULT_EXTENT, MvtFeature, MvtGeometry, MvtLayer, MvtTile, MvtValue};
use maploom_pdf::*;
use pdf_writer::{Chunk, Ref};

#[path = "support/fonts.rs"]
mod fonts;

fn encode_tile(layers: Vec<MvtLayer>) -> Vec<u8> {
    MvtTile { layers }.encode().unwrap()
}

fn label_tiles(points: &[(i32, i32, &str)]) -> TileSet {
    let features = points
        .iter()
        .map(|&(x, y, name)| {
            let mut feature = MvtFeature::new(MvtGeometry::Point(geo_types::Point::new(x, y)));
            feature.add_tag("name", MvtValue::from(name));
            feature
        })
        .collect();
    let bytes = encode_tile(vec![MvtLayer {
        name: "places".into(),
        extent: DEFAULT_EXTENT,
        features,
    }]);
    TileSet::decode(&[MvtTileInput::new(0, 0, 0, bytes)]).unwrap()
}

fn label_layer() -> TextLayer {
    let mut layer = TextLayer::new("labels", "places", TextField::property("name"));
    layer.layout.fonts = fonts::families();
    layer
}

#[test]
fn embeds_unicode_fonts_once_across_forms_and_preserves_actual_text() {
    let fonts = fonts::catalog();
    let tiles = label_tiles(&[(2048, 2048, "時年 fi בדה")]);
    let mut layer = label_layer();
    layer.paint.halo_width = 1.0.into();
    layer.paint.opacity = 0.5.into();
    let mut chunk = Chunk::new();
    let mut next = Ref::new(10);
    let view = MapViewport::new(300.0, 300.0, LngLatBounds::world(), 0.0);
    {
        let mut writer = PdfMapWriter::new(&mut chunk, &mut next, &fonts);
        let first = writer
            .write_map(view, &[Layer::Text(layer.clone())], &tiles)
            .unwrap();
        let second = writer
            .write_map(
                view,
                &[Layer::Text(layer)],
                &label_tiles(&[(2048, 2048, "Z時年 fi בדה")]),
            )
            .unwrap();
        assert_ne!(first.id(), second.id());
    }
    let form_count = 2;
    let font_count = 4;
    let objects_per_font = 6;
    assert_eq!(
        next,
        Ref::new(10 + form_count + font_count * objects_per_font)
    );
    let text = String::from_utf8_lossy(chunk.as_bytes());
    assert_eq!(text.matches("/Subtype /Type0").count(), 4);
    assert_eq!(text.matches("/Subtype /Form").count(), 2);
    assert_eq!(text.matches("/ActualText").count(), 2);
    assert!(text.contains("/CIDToGIDMap"));
    assert!(text.contains("/FontFile2"));
    assert!(text.contains("/ToUnicode"));
    assert!(
        text.contains("<00660069>"),
        "ligatures retain their Unicode sequence"
    );
    assert!(
        text.contains("<005A>"),
        "later forms contribute new glyphs to shared fonts"
    );
    assert!(text.contains("/ca 0.5"));
    assert!(
        !text.contains("1 Tr") && text.contains("0 Tr"),
        "halos must not add a second text run"
    );
    assert!(text.contains("BT\n") && text.contains(" Tj"));
    assert!(!text.contains("/Subtype /Image"));
    assert!(!text.contains(" W\n"), "text must not be clipped to a tile");
}

#[test]
fn text_errors_leave_chunk_cursor_and_font_cache_usable() {
    let fonts = fonts::catalog();
    let mut chunk = Chunk::new();
    chunk.indirect(Ref::new(1)).primitive(42);
    let original = chunk.as_bytes().to_vec();
    let mut next = Ref::new(10);
    let view = MapViewport::new(200.0, 200.0, LngLatBounds::world(), 0.0);
    let mut layer = label_layer();
    {
        let mut writer = PdfMapWriter::new(&mut chunk, &mut next, &fonts);
        assert!(matches!(
            writer.write_map(
                view,
                &[Layer::Text(layer.clone())],
                &label_tiles(&[(2048, 2048, "🦄")])
            ),
            Err(PdfRenderError::Font(FontError::MissingGlyph { .. }))
        ));
        layer.layout.fonts = vec!["Absent".into()];
        assert!(matches!(
            writer.write_map(
                view,
                &[Layer::Text(layer.clone())],
                &label_tiles(&[(2048, 2048, "A")])
            ),
            Err(PdfRenderError::Font(FontError::MissingFamily(_)))
        ));
        layer = label_layer();
        layer.layout.field = TextField::new(Expression::literal(42.0));
        assert!(
            matches!(writer.write_map(view, &[Layer::Text(layer)], &label_tiles(&[(2048,2048,"A")])), Err(PdfRenderError::Text { layer, .. }) if layer == "labels")
        );
    }
    assert_eq!(chunk.as_bytes(), original);
    assert_eq!(next, Ref::new(10));
    let map = PdfMapWriter::new(&mut chunk, &mut next, &fonts)
        .write_map(
            view,
            &[Layer::Text(label_layer())],
            &label_tiles(&[(2048, 2048, "A")]),
        )
        .unwrap();
    assert_eq!(map.id(), Ref::new(10));
    assert_eq!(next, Ref::new(17));
}

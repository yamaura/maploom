#![cfg(not(feature = "fonts"))]

use maploom_pdf::{
    Layer, LngLatBounds, MapViewport, PdfMapWriter, PdfRenderError, TextLayer, TileSet,
};
use pdf_writer::{Chunk, Ref};

#[test]
fn unsupported_text_leaves_the_document_and_writer_usable() {
    let mut chunk = Chunk::new();
    chunk.indirect(Ref::new(1)).primitive(42);
    let original = chunk.as_bytes().to_vec();
    let mut next = Ref::new(10);
    let view = MapViewport::new(300.0, 300.0, LngLatBounds::world(), 0.0);
    let mut layer = TextLayer::new("labels", "places", "Hello");
    layer.layout.fonts = vec!["Helvetica".into()];
    {
        let mut writer = PdfMapWriter::new(&mut chunk, &mut next);
        assert!(matches!(
            writer.write_map(view, &[Layer::Text(layer)], &TileSet::default()),
            Err(PdfRenderError::InvalidLayer { index: 0, message })
                if message.contains("fonts feature")
        ));
    }
    assert_eq!(chunk.as_bytes(), original);
    assert_eq!(next, Ref::new(10));
    let map = PdfMapWriter::new(&mut chunk, &mut next)
        .write_map(view, &[], &TileSet::default())
        .unwrap();
    assert_eq!(map.id(), Ref::new(10));
    assert_eq!(next, Ref::new(11));
}

use maploom_pdf::{Layer, MapViewport, MvtTileInput, PdfRenderError, TileSet, write_map};
use pdf_writer::{Chunk, Ref};

pub fn render_chunk(
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

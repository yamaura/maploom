//! Application-managed optional PDF layers, sharing one decoded tile set.
mod common;

use maploom_pdf::{LngLatBounds, MapViewport, write_map};
use pdf_writer::{Content, Name, Pdf, Rect, Ref, TextStr, types::PageMode};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| "layers.pdf".into());
    let (style_layers, tiles) = common::sample_data()?;
    let view = MapViewport::new(400.0, 300.0, LngLatBounds::world(), 0.0);
    let area = Rect::new(40.0, 200.0, 440.0, 500.0);
    let mut pdf = Pdf::new();
    let mut next_id = Ref::new(1);
    let catalog_id = next_id.bump();
    let pages_id = next_id.bump();
    let page_id = next_id.bump();
    let content_id = next_id.bump();

    let mut layers = Vec::new();
    for (layer, label) in style_layers.iter().zip(["Land", "Roads"]) {
        let ocg_id = next_id.bump();
        {
            let mut ocg = pdf.indirect(ocg_id).dict();
            ocg.pair(Name(b"Type"), Name(b"OCG"));
            ocg.pair(Name(b"Name"), TextStr(label));
        }
        let map = write_map(
            &mut pdf,
            next_id.bump(),
            view,
            std::slice::from_ref(layer),
            &tiles,
        )?;
        layers.push((label, ocg_id, map));
    }
    let ocg_ids: Vec<_> = layers.iter().map(|(_, id, _)| *id).collect();
    {
        let mut catalog = pdf.catalog(catalog_id);
        catalog.pages(pages_id).page_mode(PageMode::UseOC);
        let mut properties = catalog.insert(Name(b"OCProperties")).dict();
        properties
            .insert(Name(b"OCGs"))
            .array()
            .items(ocg_ids.iter().copied());
        let mut config = properties.insert(Name(b"D")).dict();
        config.pair(Name(b"BaseState"), Name(b"ON"));
        config
            .insert(Name(b"Order"))
            .array()
            .items(ocg_ids.iter().copied());
        config
            .insert(Name(b"ON"))
            .array()
            .items(ocg_ids.iter().copied());
    }
    pdf.pages(pages_id).kids([page_id]).count(1);
    {
        let mut page = pdf.page(page_id);
        page.parent(pages_id)
            .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
            .contents(content_id);
        let mut resources = page.resources();
        {
            let mut xobjects = resources.x_objects();
            for (label, _, map) in &layers {
                xobjects.pair(Name(label.as_bytes()), map.id());
            }
        }
        let mut properties = resources.insert(Name(b"Properties")).dict();
        for (label, ocg_id, _) in &layers {
            properties.pair(Name(label.as_bytes()), *ocg_id);
        }
    }
    let mut content = Content::new();
    for (label, _, map) in &layers {
        let name = Name(label.as_bytes());
        content
            .begin_marked_content_with_properties(Name(b"OC"))
            .properties_named(name);
        map.draw(&mut content, name, area)?;
        content.end_marked_content();
    }
    let content = content.finish();
    pdf.stream(content_id, &content);
    pdf.merge_limits(content.limits());
    std::fs::write(destination, pdf.finish())?;
    Ok(())
}

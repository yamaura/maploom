//! Generate a map with synthetic geometry: `cargo run -p maploom-pdf --example render`.
mod common;
use maploom_pdf::{LngLatBounds, MapViewport, write_map};
use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let destination = std::env::args_os()
        .nth(1)
        .unwrap_or_else(|| "map.pdf".into());
    let (style_layers, tiles) = common::sample_data()?;
    let mut pdf = Pdf::new();
    let mut next_id = Ref::new(1);
    let catalog_id = next_id.bump();
    let pages_id = next_id.bump();
    let page_id = next_id.bump();
    let content_id = next_id.bump();
    let font_id = next_id.bump();
    let map = write_map(
        &mut pdf,
        next_id.bump(),
        MapViewport::new(400.0, 300.0, LngLatBounds::world(), 0.0),
        &style_layers,
        &tiles,
    )?;

    pdf.catalog(catalog_id).pages(pages_id);
    pdf.pages(pages_id).kids([page_id]).count(1);
    pdf.type1_font(font_id).base_font(Name(b"Helvetica"));
    {
        let mut page = pdf.page(page_id);
        page.parent(pages_id)
            .media_box(Rect::new(0.0, 0.0, 595.0, 842.0))
            .contents(content_id);
        let mut resources = page.resources();
        resources.fonts().pair(Name(b"Body"), font_id);
        resources.x_objects().pair(Name(b"Map"), map.id());
    }

    let mut content = Content::new();
    content.set_fill_rgb(0.12, 0.18, 0.22);
    content
        .begin_text()
        .set_font(Name(b"Body"), 22.0)
        .next_line(40.0, 785.0)
        .show(Str(b"Maps inside a document"))
        .end_text();
    map.draw(
        &mut content,
        Name(b"Map"),
        Rect::new(40.0, 340.0, 555.0, 730.0),
    )?;
    map.draw(
        &mut content,
        Name(b"Map"),
        Rect::new(350.0, 110.0, 555.0, 270.0),
    )?;
    content
        .begin_text()
        .set_font(Name(b"Body"), 12.0)
        .next_line(40.0, 300.0)
        .show(Str(b"Main map above; reused detail at the lower right."))
        .end_text();
    let content = content.finish();
    pdf.stream(content_id, &content);
    pdf.merge_limits(content.limits());
    std::fs::write(destination, pdf.finish())?;
    Ok(())
}

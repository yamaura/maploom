use crate::{FontCatalog, FontError, TextError, TextStyle};
use maploom_core::Rgb;
use pdf_writer::{Content, Name, Rect, Str, TextStr, types::TextRenderingMode};
use rustybuzz::{Direction, UnicodeBuffer, ttf_parser::GlyphId};
use std::{collections::BTreeSet, ops::Range};
use unicode_bidi::BidiInfo;
use unicode_script::{Script, UnicodeScript};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) struct Glyph {
    pub(crate) font: usize,
    pub(crate) id: u16,
    pub(crate) unicode: String,
    pub(crate) cid: u16,
    x: f32,
    y: f32,
}

pub struct Label {
    text: String,
    pub(crate) glyphs: Vec<Glyph>,
    pub bounds: Rect,
}

struct Line {
    glyphs: Vec<Glyph>,
    width: f32,
    ascent: f32,
    descent: f32,
}

struct Segment {
    bytes: Range<usize>,
    font: usize,
    script: Script,
}

fn shape_line(
    fonts: &FontCatalog<'_>,
    families: &[String],
    ids: &[usize],
    text: &str,
    size: f32,
) -> Result<Line, FontError> {
    let bidi = BidiInfo::new(text, None);
    let mut line = Line {
        glyphs: Vec::new(),
        width: 0.0,
        ascent: 0.0,
        descent: 0.0,
    };
    // Empty lines still need height for multiline placement.
    let face = &fonts.faces[ids[0]].face;
    let scale = size / face.units_per_em() as f32;
    line.ascent = f32::from(face.ascender()) * scale;
    line.descent = -f32::from(face.descender()) * scale;
    for paragraph in &bidi.paragraphs {
        let (levels, runs) = bidi.visual_runs(paragraph, paragraph.range.clone());
        for run in runs {
            let rtl = levels[run.start].is_rtl();
            let mut segments: Vec<Segment> = Vec::new();
            for (offset, grapheme) in text[run.clone()].grapheme_indices(true) {
                let font = fonts.select(ids, grapheme, families)?;
                let script = grapheme
                    .chars()
                    .map(|c| c.script())
                    .find(|s| !matches!(s, Script::Common | Script::Inherited))
                    .unwrap_or(Script::Common);
                let start = run.start + offset;
                if let Some(previous) = segments.last_mut()
                    && previous.font == font
                    && (previous.script == script
                        || script == Script::Common
                        || previous.script == Script::Common)
                {
                    previous.bytes.end = start + grapheme.len();
                    if previous.script == Script::Common {
                        previous.script = script;
                    }
                    continue;
                }
                segments.push(Segment {
                    bytes: start..start + grapheme.len(),
                    font,
                    script,
                });
            }
            if rtl {
                segments.reverse();
            }
            for segment in segments {
                let face = &fonts.faces[segment.font].face;
                let scale = size / face.units_per_em() as f32;
                line.ascent = line.ascent.max(f32::from(face.ascender()) * scale);
                line.descent = line.descent.max(-f32::from(face.descender()) * scale);
                let mut buffer = UnicodeBuffer::new();
                buffer.push_str(&text[segment.bytes.clone()]);
                buffer.set_direction(if rtl {
                    Direction::RightToLeft
                } else {
                    Direction::LeftToRight
                });
                buffer.guess_segment_properties();
                let shaped = rustybuzz::shape(face, &[], buffer);
                let mut clusters: BTreeSet<_> = shaped
                    .glyph_infos()
                    .iter()
                    .map(|info| info.cluster as usize)
                    .collect();
                clusters.insert(segment.bytes.len());
                let mut assigned = BTreeSet::new();
                for (info, position) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                    if info.glyph_id == 0 {
                        let character = text[segment.bytes.start + info.cluster as usize..]
                            .chars()
                            .next()
                            .unwrap_or('\u{fffd}');
                        if !crate::fonts::ignorable(character) {
                            return Err(FontError::MissingGlyph {
                                character,
                                families: families.to_vec(),
                            });
                        }
                    } else {
                        line.glyphs.push(Glyph {
                            font: segment.font,
                            id: info.glyph_id as u16,
                            unicode: if assigned.insert(info.cluster) {
                                let start = info.cluster as usize;
                                let end = *clusters.range(start + 1..).next().expect("cluster end");
                                text[segment.bytes.start + start..segment.bytes.start + end]
                                    .to_owned()
                            } else {
                                String::new()
                            },
                            cid: 0, // Reserved until form preparation.
                            x: line.width + position.x_offset as f32 * scale,
                            y: position.y_offset as f32 * scale,
                        });
                    }
                    line.width += position.x_advance as f32 * scale;
                }
            }
        }
    }
    Ok(line)
}

pub fn layout(
    fonts: &FontCatalog<'_>,
    style: &TextStyle,
    text: &str,
    point: (f32, f32),
) -> Result<Label, TextError> {
    let ids = fonts
        .resolve(&style.layout.fonts)
        .map_err(TextError::Font)?;
    let normalized = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', "    ");
    let lines = normalized
        .split('\n')
        .map(|line| shape_line(fonts, &style.layout.fonts, &ids, line, style.size))
        .collect::<Result<Vec<_>, _>>()
        .map_err(TextError::Font)?;
    let width = lines.iter().map(|line| line.width).fold(0.0, f32::max);
    let ascent = lines.iter().map(|line| line.ascent).fold(0.0, f32::max);
    let descent = lines.iter().map(|line| line.descent).fold(0.0, f32::max);
    let line_height = style.layout.line_height * style.size;
    let height = ascent + descent + (lines.len() - 1) as f32 * line_height;
    let [anchor_x, anchor_y] = style.layout.anchor.fractions();
    let x = point.0 + style.layout.offset[0] * style.size - width * anchor_x;
    let top = point.1 - style.layout.offset[1] * style.size + height * anchor_y;
    let mut bounds = Rect::new(x, top - height, x + width, top);
    let mut glyphs = Vec::new();
    for (row, line) in lines.into_iter().enumerate() {
        let line_x = x + (width - line.width) * anchor_x;
        let baseline = top - ascent - row as f32 * line_height;
        for mut glyph in line.glyphs {
            glyph.x += line_x;
            glyph.y += baseline;
            let face = &fonts.faces[glyph.font].face;
            let scale = style.size / face.units_per_em() as f32;
            if let Some(bbox) = face.glyph_bounding_box(GlyphId(glyph.id)) {
                bounds.x1 = bounds.x1.min(glyph.x + f32::from(bbox.x_min) * scale);
                bounds.y1 = bounds.y1.min(glyph.y + f32::from(bbox.y_min) * scale);
                bounds.x2 = bounds.x2.max(glyph.x + f32::from(bbox.x_max) * scale);
                bounds.y2 = bounds.y2.max(glyph.y + f32::from(bbox.y_max) * scale);
            }
            glyphs.push(glyph);
        }
    }
    let padding = style.layout.padding + style.halo_width;
    bounds = Rect::new(
        bounds.x1 - padding,
        bounds.y1 - padding,
        bounds.x2 + padding,
        bounds.y2 + padding,
    );
    if ![bounds.x1, bounds.x2, bounds.y1, bounds.y2]
        .iter()
        .all(|n| n.is_finite())
        || glyphs.iter().any(|g| !g.x.is_finite() || !g.y.is_finite())
    {
        return Err(TextError::CoordinateOverflow);
    }
    Ok(Label {
        text: text.to_owned(),
        glyphs,
        bounds,
    })
}

pub fn draw(content: &mut Content, label: &Label, style: &TextStyle, fonts: &FontCatalog<'_>) {
    // Copied text must retain logical Unicode order.
    content
        .begin_marked_content_with_properties(Name(b"Span"))
        .properties()
        .actual_text(TextStr(&label.text));
    content.save_state();
    set_fill_rgb(content, style.color);
    set_stroke_rgb(content, style.halo_color);
    if style.halo_width > 0.0 {
        content.set_line_width(style.halo_width * 2.0);
        content.set_line_join(pdf_writer::types::LineJoinStyle::RoundJoin);
    }
    if style.halo_width > 0.0 {
        draw_halo(content, label, style, fonts);
    }
    draw_glyphs(content, label, style, TextRenderingMode::Fill);
    content.restore_state();
    content.end_marked_content();
}

fn draw_glyphs(content: &mut Content, label: &Label, style: &TextStyle, mode: TextRenderingMode) {
    content.begin_text();
    content.set_text_rendering_mode(mode);
    let mut current = None;
    for glyph in &label.glyphs {
        if current != Some(glyph.font) {
            let name = format!("F{}", glyph.font);
            content.set_font(Name(name.as_bytes()), style.size);
            current = Some(glyph.font);
        }
        content
            .set_text_matrix([1.0, 0.0, 0.0, 1.0, glyph.x, glyph.y])
            .show(Str(&glyph.cid.to_be_bytes()));
    }
    content.end_text();
}

// Halos must not duplicate text during extraction.
fn draw_halo(content: &mut Content, label: &Label, style: &TextStyle, fonts: &FontCatalog<'_>) {
    for glyph in &label.glyphs {
        let face = &fonts.faces[glyph.font].face;
        let scale = style.size / face.units_per_em() as f32;
        let mut outline = Outline {
            content,
            scale,
            origin: (glyph.x, glyph.y),
            current: (0.0, 0.0),
        };
        if face
            .outline_glyph(GlyphId(glyph.id), &mut outline)
            .is_some()
        {
            outline.content.stroke();
        }
    }
}

struct Outline<'a> {
    content: &'a mut Content,
    scale: f32,
    origin: (f32, f32),
    current: (f32, f32),
}
impl Outline<'_> {
    fn point(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.origin.0 + x * self.scale,
            self.origin.1 + y * self.scale,
        )
    }
}
impl rustybuzz::ttf_parser::OutlineBuilder for Outline<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.point(x, y);
        self.content.move_to(px, py);
        self.current = (x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (px, py) = self.point(x, y);
        self.content.line_to(px, py);
        self.current = (x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let c1 = self.point(
            self.current.0 + (x1 - self.current.0) * 2.0 / 3.0,
            self.current.1 + (y1 - self.current.1) * 2.0 / 3.0,
        );
        let c2 = self.point(x + (x1 - x) * 2.0 / 3.0, y + (y1 - y) * 2.0 / 3.0);
        let end = self.point(x, y);
        self.content.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
        self.current = (x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let c1 = self.point(x1, y1);
        let c2 = self.point(x2, y2);
        let end = self.point(x, y);
        self.content.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
        self.current = (x, y);
    }
    fn close(&mut self) {
        self.content.close_path();
    }
}

fn set_fill_rgb(content: &mut Content, color: Rgb) {
    content.set_fill_rgb(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
    );
}
fn set_stroke_rgb(content: &mut Content, color: Rgb) {
    content.set_stroke_rgb(
        f32::from(color.r) / 255.0,
        f32::from(color.g) / 255.0,
        f32::from(color.b) / 255.0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use maploom_core::{TextField, TextLayout};
    fn style() -> TextStyle {
        TextStyle {
            layout: TextLayout {
                fonts: crate::test_fonts::families(),
                ..TextLayout::default()
            },
            size: 16.0,
            color: Rgb::BLACK,
            opacity: 1.0,
            halo_color: Rgb::new(255, 255, 255),
            halo_width: 0.0,
        }
    }
    #[test]
    fn shapes_ligatures_and_keeps_grapheme_fallback_and_bidi_order() {
        let fonts = crate::fonts::test_catalog();
        let style = style();
        let ligature = layout(&fonts, &style, "fi", (100.0, 100.0)).unwrap();
        assert_eq!(ligature.glyphs.len(), 1, "expected ligature shaping");
        assert_eq!(
            ligature
                .glyphs
                .iter()
                .map(|g| g.unicode.as_str())
                .collect::<String>(),
            "fi"
        );
        // The combining-mark case needs coverage beyond the missing-glyph fixtures.
        let mut combining_fonts = FontCatalog::new();
        combining_fonts
            .register("Latin", font_test_data::LAST_RESORT_HE)
            .unwrap();
        let combining = layout(&combining_fonts, &style, "e\u{301}", (100.0, 100.0)).unwrap();
        assert_eq!(
            combining
                .glyphs
                .iter()
                .map(|g| g.unicode.as_str())
                .collect::<String>(),
            "e\u{301}"
        );
        let mixed = layout(&fonts, &style, "A時年", (100.0, 100.0)).unwrap();
        assert_eq!(
            mixed.glyphs.iter().map(|g| g.font).collect::<Vec<_>>(),
            [0, 1, 1]
        );
        let rtl = layout(&fonts, &style, "בדה", (100.0, 100.0)).unwrap();
        let face = &fonts.faces[2].face;
        assert_eq!(
            rtl.glyphs.iter().map(|g| g.id).collect::<Vec<_>>(),
            ['ה', 'ד', 'ב'].map(|c| face.glyph_index(c).unwrap().0)
        );
        let multiline = layout(&fonts, &style, "A\n時年", (100.0, 100.0)).unwrap();
        assert!(multiline.glyphs[0].y > multiline.glyphs[1].y);
        assert!(layout(&fonts, &style, "\u{1f984}", (100.0, 100.0)).is_err());
        let mut numeric = style;
        numeric.layout.field = TextField::property("name");
        numeric.size = f32::MAX;
        assert!(layout(&fonts, &numeric, "AAAA", (100.0, 100.0)).is_err());
    }
}

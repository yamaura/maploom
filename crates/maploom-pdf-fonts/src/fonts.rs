use pdf_writer::{
    Chunk, Name, Rect, Ref, Str,
    types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap},
};
use rustybuzz::{
    Face,
    ttf_parser::{GlyphId, Tag, name_id},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FontError {
    #[error("invalid font {name:?}: {reason}")]
    InvalidFont { name: String, reason: &'static str },
    #[error("font family {0:?} is already registered")]
    DuplicateName(String),
    #[error("font {0:?} exceeds the 65535 distinct PDF glyph/text encodings")]
    EncodingLimit(String),
    #[error("none of the requested font families are registered: {0:?}")]
    MissingFamily(Vec<String>),
    #[error("no glyph for {character:?} (U+{codepoint:04X}) in {families:?}", codepoint = u32::from(*.character))]
    MissingGlyph {
        character: char,
        families: Vec<String>,
    },
}

/// Immutable font data, independent of a PDF document or filesystem.
/// Register standalone, static TrueType fonts before creating a PDF writer.
#[derive(Default)]
pub struct FontCatalog<'a> {
    pub(crate) faces: Vec<FontFace<'a>>,
}

pub(crate) struct FontFace<'a> {
    pub(crate) name: String,
    pub(crate) bytes: &'a [u8],
    pub(crate) face: Face<'a>,
}

impl<'a> FontCatalog<'a> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, name: impl Into<String>, bytes: &'a [u8]) -> Result<(), FontError> {
        let name = name.into();
        let invalid = |reason| FontError::InvalidFont {
            name: name.clone(),
            reason,
        };
        if name.trim().is_empty() {
            return Err(invalid("family name must be nonempty"));
        }
        if self.faces.iter().any(|font| font.name == name) {
            return Err(FontError::DuplicateName(name));
        }
        if bytes.get(..4) == Some(b"ttcf") {
            return Err(invalid(
                "font collections must be extracted into standalone fonts",
            ));
        }
        let face = Face::from_slice(bytes, 0).ok_or_else(|| invalid("cannot parse font"))?;
        if face.raw_face().table(Tag::from_bytes(b"glyf")).is_none() {
            return Err(invalid("only TrueType glyf outlines are supported"));
        }
        if face.is_variable() {
            return Err(invalid(
                "instantiate variable fonts before registering them",
            ));
        }
        if face.units_per_em() <= 0 {
            return Err(invalid("units per em must be positive"));
        }
        self.faces.push(FontFace { name, bytes, face });
        Ok(())
    }

    pub(crate) fn resolve(&self, names: &[String]) -> Result<Vec<usize>, FontError> {
        let ids: Vec<_> = names
            .iter()
            .filter_map(|name| self.faces.iter().position(|font| &font.name == name))
            .collect();
        if ids.is_empty() {
            Err(FontError::MissingFamily(names.to_vec()))
        } else {
            Ok(ids)
        }
    }

    pub(crate) fn select(
        &self,
        ids: &[usize],
        grapheme: &str,
        names: &[String],
    ) -> Result<usize, FontError> {
        if let Some(id) = ids.iter().copied().find(|id| {
            grapheme
                .chars()
                .all(|c| ignorable(c) || self.faces[*id].face.glyph_index(c).is_some())
        }) {
            return Ok(id);
        }
        let character = grapheme
            .chars()
            .find(|c| {
                !ignorable(*c)
                    && !ids
                        .iter()
                        .any(|id| self.faces[*id].face.glyph_index(*c).is_some())
            })
            .unwrap_or_else(|| grapheme.chars().next().unwrap_or('\u{fffd}'));
        Err(FontError::MissingGlyph {
            character,
            families: names.to_vec(),
        })
    }
}

pub(crate) fn ignorable(c: char) -> bool {
    matches!(
        c,
        '\u{200c}' | '\u{200d}' | '\u{200e}' | '\u{200f}' | '\u{061c}' | '\u{feff}'
    ) || ('\u{202a}'..='\u{202e}').contains(&c)
        || ('\u{2066}'..='\u{2069}').contains(&c)
        || ('\u{fe00}'..='\u{fe0f}').contains(&c)
        || ('\u{e0100}'..='\u{e01ef}').contains(&c)
}

#[cfg(test)]
pub(crate) fn test_catalog() -> FontCatalog<'static> {
    crate::test_fonts::catalog()
}

/// Stable CIDs distinguish identical outlines used for different Unicode clusters.
pub(crate) struct EmbeddedFont {
    pub(crate) reference: Ref,
    descendant: Ref,
    descriptor: Ref,
    file: Ref,
    unicode: Ref,
    cid_map: Ref,
    pub(crate) codes: BTreeMap<(u16, String), u16>,
    entries: Vec<(u16, String)>,
}
impl EmbeddedFont {
    pub(crate) fn new(next: &mut Ref) -> Self {
        Self {
            reference: next.bump(),
            descendant: next.bump(),
            descriptor: next.bump(),
            file: next.bump(),
            unicode: next.bump(),
            cid_map: next.bump(),
            codes: BTreeMap::new(),
            entries: Vec::new(),
        }
    }
    pub(crate) fn encode(&mut self, glyph: u16, text: &str) -> u16 {
        let key = (glyph, text.to_owned());
        if let Some(cid) = self.codes.get(&key) {
            return *cid;
        }
        // Encoding capacity must already be validated.
        let cid = (self.entries.len() + 1) as u16;
        self.entries.push(key.clone());
        self.codes.insert(key, cid);
        cid
    }
}

pub(crate) fn embed(chunk: &mut Chunk, font: &FontFace<'_>, index: usize, embedded: &EmbeddedFont) {
    let EmbeddedFont {
        reference: type0,
        descendant,
        descriptor,
        file,
        unicode,
        cid_map,
        entries,
        ..
    } = embedded;
    let face = &font.face;
    let postscript = face
        .names()
        .into_iter()
        .find(|name| name.name_id == name_id::POST_SCRIPT_NAME)
        .and_then(|name| name.to_string())
        .unwrap_or_else(|| format!("MaploomFont{index}"));
    let name: Vec<u8> = postscript
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-+_".contains(&b) {
                b
            } else {
                b'_'
            }
        })
        .collect();
    let info = SystemInfo {
        registry: Str(b"Adobe"),
        ordering: Str(b"Identity"),
        supplement: 0,
    };
    let scale = 1000.0 / face.units_per_em() as f32;
    let bbox = face.global_bounding_box();
    let mut flags = FontFlags::NON_SYMBOLIC;
    if face.is_monospaced() {
        flags |= FontFlags::FIXED_PITCH;
    }
    if face.is_italic() {
        flags |= FontFlags::ITALIC;
    }
    chunk
        .type0_font(*type0)
        .base_font(Name(&name))
        .encoding_predefined(Name(b"Identity-H"))
        .descendant_font(*descendant)
        .to_unicode(*unicode);
    {
        let mut cid = chunk.cid_font(*descendant);
        cid.subtype(CidFontType::Type2)
            .base_font(Name(&name))
            .system_info(info)
            .font_descriptor(*descriptor)
            .default_width(0.0)
            .cid_to_gid_map_stream(*cid_map);
        cid.widths().consecutive(
            1,
            entries.iter().map(|(id, _)| {
                f32::from(face.glyph_hor_advance(GlyphId(*id)).unwrap_or(0)) * scale
            }),
        );
    }
    chunk
        .font_descriptor(*descriptor)
        .name(Name(&name))
        .flags(flags)
        .bbox(Rect::new(
            f32::from(bbox.x_min) * scale,
            f32::from(bbox.y_min) * scale,
            f32::from(bbox.x_max) * scale,
            f32::from(bbox.y_max) * scale,
        ))
        .italic_angle(face.italic_angle())
        .ascent(f32::from(face.ascender()) * scale)
        .descent(f32::from(face.descender()) * scale)
        .cap_height(f32::from(face.capital_height().unwrap_or(face.ascender())) * scale)
        .stem_v(80.0)
        .font_file2(*file);
    chunk
        .stream(*file, font.bytes)
        .pair(Name(b"Length1"), font.bytes.len() as i32);
    let mut glyph_map = vec![0, 0]; // CID zero is .notdef
    let mut cmap = UnicodeCmap::new(Name(b"MaploomUnicode"), info);
    for (index, (glyph, text)) in entries.iter().enumerate() {
        glyph_map.extend(glyph.to_be_bytes());
        cmap.pair_with_multiple((index + 1) as u16, text.chars());
    }
    chunk.stream(*cid_map, &glyph_map);
    chunk.stream(*unicode, &cmap.finish());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_is_explicit_and_failed_registration_keeps_the_catalog_usable() {
        let mut fonts = FontCatalog::new();
        let bytes = font_test_data::LAST_RESORT_HE;
        assert!(matches!(
            fonts.register("bad", b"not a font"),
            Err(FontError::InvalidFont { .. })
        ));
        assert!(matches!(
            fonts.register("collection", b"ttcf"),
            Err(FontError::InvalidFont { .. })
        ));
        assert!(fonts.register(" ", bytes).is_err());
        fonts.register("Latin", bytes).unwrap();
        assert!(matches!(
            fonts.register("Latin", bytes),
            Err(FontError::DuplicateName(_))
        ));
        assert_eq!(fonts.faces.len(), 1);
        let families = vec!["Absent".into(), "Latin".into()];
        assert_eq!(fonts.resolve(&families).unwrap(), [0]);
        assert_eq!(fonts.select(&[0], "e\u{301}", &families).unwrap(), 0);
    }
}

use crate::{
    FontCatalog, FontError, Label,
    fonts::{self, EmbeddedFont},
};
use pdf_writer::{Chunk, Ref};
use std::collections::{BTreeMap, BTreeSet};

/// Document-scoped glyph encodings and shared embedded font resources.
pub struct FontResources<'fonts, 'data> {
    fonts: &'fonts FontCatalog<'data>,
    embedded: BTreeMap<usize, EmbeddedFont>,
}

/// Form ID and shared font references for its resource dictionary.
pub struct FontForm {
    pub id: Ref,
    pub fonts: Vec<(String, Ref)>,
}

impl<'fonts, 'data> FontResources<'fonts, 'data> {
    pub fn new(fonts: &'fonts FontCatalog<'data>) -> Self {
        Self {
            fonts,
            embedded: BTreeMap::new(),
        }
    }

    /// Allocate a form and assign glyph CIDs. Errors leave labels and IDs unchanged.
    /// Use the same reference cursor for every form in this document.
    pub fn prepare_form<'a>(
        &mut self,
        labels: impl IntoIterator<Item = &'a mut Label>,
        next: &mut Ref,
    ) -> Result<FontForm, FontError> {
        let mut labels: Vec<_> = labels.into_iter().collect();
        let mut needed: BTreeMap<usize, BTreeSet<(u16, String)>> = BTreeMap::new();
        for glyph in labels.iter().flat_map(|label| &label.glyphs) {
            if self
                .embedded
                .get(&glyph.font)
                .is_none_or(|font| !font.codes.contains_key(&(glyph.id, glyph.unicode.clone())))
            {
                needed
                    .entry(glyph.font)
                    .or_default()
                    .insert((glyph.id, glyph.unicode.clone()));
            }
        }
        for (font, codes) in &needed {
            let existing = self.embedded.get(font).map_or(0, |font| font.codes.len());
            if existing + codes.len() > u16::MAX as usize {
                return Err(FontError::EncodingLimit(
                    self.fonts.faces[*font].name.clone(),
                ));
            }
        }
        let id = next.bump();
        let used_fonts: BTreeSet<_> = labels
            .iter()
            .flat_map(|label| label.glyphs.iter().map(|glyph| glyph.font))
            .collect();
        for font in &used_fonts {
            self.embedded
                .entry(*font)
                .or_insert_with(|| EmbeddedFont::new(next));
        }
        for glyph in labels.iter_mut().flat_map(|label| &mut label.glyphs) {
            glyph.cid = self
                .embedded
                .get_mut(&glyph.font)
                .expect("allocated font")
                .encode(glyph.id, &glyph.unicode);
        }
        Ok(FontForm {
            id,
            fonts: used_fonts
                .into_iter()
                .map(|font| (format!("F{font}"), self.embedded[&font].reference))
                .collect(),
        })
    }

    /// Write shared fonts. Call once after preparing all forms.
    pub fn finish(&self, chunk: &mut Chunk) {
        for (index, embedded) in &self.embedded {
            fonts::embed(chunk, &self.fonts.faces[*index], *index, embedded);
        }
    }
}

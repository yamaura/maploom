use serde::{Deserialize, Serialize};

/// An opaque RGB color. Serde accepts `#rgb`/`#rrggbb` and emits `#rrggbb`.
/// Use the paint's opacity for transparency.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0, 0, 0);
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

impl std::str::FromStr for Rgb {
    type Err = String;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("expected an RGB hex color, got {input:?}");
        let hex = input.strip_prefix('#').ok_or_else(invalid)?;
        if !hex.is_ascii() {
            return Err(invalid());
        }
        let channel = |range| u8::from_str_radix(&hex[range], 16).map_err(|_| invalid());
        match hex.len() {
            3 => Ok(Self::new(
                channel(0..1)? * 17,
                channel(1..2)? * 17,
                channel(2..3)? * 17,
            )),
            6 => Ok(Self::new(channel(0..2)?, channel(2..4)?, channel(4..6)?)),
            _ => Err(invalid()),
        }
    }
}

impl TryFrom<String> for Rgb {
    type Error = String;
    fn try_from(input: String) -> Result<Self, Self::Error> {
        input.parse()
    }
}

impl From<Rgb> for String {
    fn from(color: Rgb) -> Self {
        format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
    }
}

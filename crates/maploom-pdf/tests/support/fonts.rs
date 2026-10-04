use super::FontCatalog;

pub fn catalog() -> FontCatalog<'static> {
    let mut fonts = FontCatalog::new();
    for (name, bytes) in [
        ("Latin", font_test_data::AHEM),
        ("CJK", font_test_data::NOTOSERIFTC_AUTOHINT_METRICS),
        ("Hebrew", font_test_data::NOTOSERIFHEBREW_AUTOHINT_METRICS),
        ("Ligatures", font_test_data::NOTOSERIF_AUTOHINT_SHAPING),
    ] {
        fonts.register(name, bytes).unwrap();
    }
    fonts
}

pub fn families() -> Vec<String> {
    ["Ligatures", "Latin", "CJK", "Hebrew"]
        .map(String::from)
        .to_vec()
}

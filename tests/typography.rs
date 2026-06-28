//! Integration tests for `FontFamily`, `TextElement` attached font properties,
//! the OpenType `Typography` attached DPs, and the IME `CompositionUnderline`
//! list on a `TextBox`.

use std::path::PathBuf;

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::font_provider::{FontProvider, set_font_provider};
use noesis_runtime::typography::{
    self, CompositionLineStyle, CompositionUnderline, FontCapitals, FontFamily, FontFraction,
    FontNumeralStyle, FontStretch, FontStyle, FontVariants, FontWeight,
};
use noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

/// Minimal [`FontProvider`] that resolves a single font face so the
/// per-family enumeration getters yield a positive result.
struct SingleFontProvider {
    folder: String,
    filename: String,
    bytes: Vec<u8>,
    /// Keeps the most recently opened bytes alive across the borrow that
    /// `open_font` returns.
    current: Option<Vec<u8>>,
}

impl FontProvider for SingleFontProvider {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn scan_folder(&mut self, folder_uri: &str, register: &mut dyn FnMut(&str)) {
        if folder_uri == self.folder {
            register(&self.filename);
        }
    }

    fn open_font(&mut self, folder_uri: &str, filename: &str) -> Option<&[u8]> {
        if folder_uri == self.folder && filename == self.filename {
            self.current = Some(self.bytes.clone());
            self.current.as_deref()
        } else {
            None
        }
    }
}

#[test]
fn typography_round_trips() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    // The guard is bound to the function scope so it outlives shutdown();
    // Noesis may call back into the provider during teardown.
    let sdk_dir =
        std::env::var("NOESIS_SDK_DIR").expect("NOESIS_SDK_DIR not set; required for this test");
    let mut bitter_path = PathBuf::from(&sdk_dir);
    bitter_path.push("Data/Fonts/Bitter-Regular.ttf");
    let bitter_bytes = std::fs::read(&bitter_path)
        .unwrap_or_else(|_| panic!("read failed: {}", bitter_path.display()));
    let registered = set_font_provider(SingleFontProvider {
        folder: "Fonts".to_string(),
        filename: "Bitter-Regular.ttf".to_string(),
        bytes: bitter_bytes,
        current: None,
    });
    // Eagerly register the face so the `Fonts/#Bitter` lookup below resolves
    // regardless of lazy `ScanFolder` timing.
    registered.register_font("Fonts", "Bitter-Regular.ttf");

    {
        let family = FontFamily::new("Arial, Verdana");
        assert_eq!(
            family.source().as_deref(),
            Some("Arial, Verdana"),
            "FontFamily source round-trips through the live object"
        );
        // Out-of-range names are None, proving the C-side bound check crosses.
        let n = family.num_fonts();
        assert!(
            family.font_name(n).is_none(),
            "font_name past num_fonts is None"
        );

        let bitter = FontFamily::new("Fonts/#Bitter");
        assert!(
            bitter.num_fonts() >= 1,
            "registered Bitter face resolves to at least one font (num_fonts = {})",
            bitter.num_fonts()
        );
        assert_eq!(
            bitter.font_name(0).as_deref(),
            Some("Bitter"),
            "font_name(0) reads back the registered family name from the live object"
        );

        let tb_xaml = format!("<TextBlock {NS} Text=\"Hello\"/>");
        let tb = FrameworkElement::parse(&tb_xaml).expect("parse TextBlock");

        assert!(typography::set_font_size(&tb, 37.5), "set FontSize");
        assert_eq!(
            typography::font_size(&tb),
            Some(37.5),
            "FontSize round-trips a non-default value"
        );

        assert!(typography::set_font_family(&tb, &family), "set FontFamily");
        let read_family = typography::get_font_family(&tb).expect("FontFamily set");
        assert_eq!(
            read_family.source().as_deref(),
            Some("Arial, Verdana"),
            "assigned FontFamily reads back its source"
        );
        assert_eq!(
            read_family.raw(),
            family.raw(),
            "assigned FontFamily is the exact object (AddRef, not clone)"
        );

        let brush = SolidColorBrush::new([0.2, 0.4, 0.6, 1.0]);
        assert!(typography::set_foreground(&tb, &brush), "set Foreground");
        assert_eq!(
            typography::get_foreground(&tb)
                .expect("Foreground set")
                .as_ptr(),
            brush.raw(),
            "Foreground is the exact brush we assigned"
        );

        assert!(
            typography::set_font_weight(&tb, FontWeight::Bold),
            "set FontWeight"
        );
        assert_eq!(
            typography::font_weight(&tb),
            Some(FontWeight::Bold),
            "FontWeight round-trips Bold (700)"
        );
        assert!(
            typography::set_font_style(&tb, FontStyle::Italic),
            "set FontStyle"
        );
        assert_eq!(
            typography::font_style(&tb),
            Some(FontStyle::Italic),
            "FontStyle round-trips Italic"
        );
        assert!(
            typography::set_font_stretch(&tb, FontStretch::Condensed),
            "set FontStretch"
        );
        assert_eq!(
            typography::font_stretch(&tb),
            Some(FontStretch::Condensed),
            "FontStretch round-trips Condensed"
        );

        assert!(
            typography::set_capitals(&tb, FontCapitals::SmallCaps),
            "set Capitals"
        );
        assert_eq!(
            typography::capitals(&tb),
            Some(FontCapitals::SmallCaps),
            "Typography.Capitals round-trips SmallCaps"
        );

        assert!(
            typography::set_numeral_style(&tb, FontNumeralStyle::OldStyle),
            "set NumeralStyle"
        );
        assert_eq!(
            typography::numeral_style(&tb),
            Some(FontNumeralStyle::OldStyle),
            "Typography.NumeralStyle round-trips OldStyle"
        );

        assert!(
            typography::set_fraction(&tb, FontFraction::Slashed),
            "set Fraction"
        );
        assert_eq!(
            typography::fraction(&tb),
            Some(FontFraction::Slashed),
            "Typography.Fraction round-trips Slashed"
        );

        assert!(
            typography::set_variants(&tb, FontVariants::Superscript),
            "set Variants"
        );
        assert_eq!(
            typography::variants(&tb),
            Some(FontVariants::Superscript),
            "Typography.Variants round-trips Superscript"
        );

        // Bool flags default true (StandardLigatures, Kerning); flip to false.
        assert!(
            typography::set_standard_ligatures(&tb, false),
            "set StandardLigatures"
        );
        assert_eq!(
            typography::standard_ligatures(&tb),
            Some(false),
            "Typography.StandardLigatures round-trips false"
        );
        assert!(typography::set_kerning(&tb, false), "set Kerning");
        assert_eq!(
            typography::kerning(&tb),
            Some(false),
            "Typography.Kerning round-trips false"
        );

        // A plain Border is a DependencyObject too: the attached props apply.
        let border_xaml = format!("<Border {NS}/>");
        let border = FrameworkElement::parse(&border_xaml).expect("parse Border");
        assert!(
            typography::set_font_size(&border, 22.0),
            "set FontSize on Border"
        );
        assert_eq!(typography::font_size(&border), Some(22.0));
        assert_eq!(
            typography::num_composition_underlines(&border),
            None,
            "Border is not a TextBox"
        );
        assert!(
            !typography::add_composition_underline(
                &border,
                CompositionUnderline {
                    start: 0,
                    end: 1,
                    style: CompositionLineStyle::Solid,
                    bold: false,
                }
            ),
            "add_composition_underline fails on a non-TextBox"
        );

        let tbx_xaml = format!("<TextBox {NS} Text=\"compose\"/>");
        let tbx = FrameworkElement::parse(&tbx_xaml).expect("parse TextBox");
        assert_eq!(
            typography::num_composition_underlines(&tbx),
            Some(0),
            "TextBox starts with no composition underlines"
        );

        let u0 = CompositionUnderline {
            start: 0,
            end: 3,
            style: CompositionLineStyle::Squiggle,
            bold: true,
        };
        let u1 = CompositionUnderline {
            start: 3,
            end: 7,
            style: CompositionLineStyle::Dot,
            bold: false,
        };
        assert!(typography::add_composition_underline(&tbx, u0), "add u0");
        assert!(typography::add_composition_underline(&tbx, u1), "add u1");
        assert_eq!(
            typography::num_composition_underlines(&tbx),
            Some(2),
            "two composition underlines added"
        );
        assert_eq!(
            typography::composition_underline(&tbx, 0),
            Some(u0),
            "underline 0 reads back its fields"
        );
        assert_eq!(
            typography::composition_underline(&tbx, 1),
            Some(u1),
            "underline 1 reads back its fields"
        );
        assert!(
            typography::composition_underline(&tbx, 2).is_none(),
            "out-of-range underline is None"
        );

        assert!(
            typography::clear_composition_underlines(&tbx),
            "clear underlines"
        );
        assert_eq!(
            typography::num_composition_underlines(&tbx),
            Some(0),
            "underlines cleared"
        );
    }

    noesis_runtime::shutdown();
}

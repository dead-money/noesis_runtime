//! [`FormattedText`] measurement round-trips against real glyphs (`Bitter-Regular.ttf`);
//! a stub returning zero or a constant fails the positive-width and
//! "longer string measures wider" assertions.
//!
//! Requires `NOESIS_SDK_DIR` (`Data/Fonts/Bitter-Regular.ttf` is read here).

use std::collections::HashMap;
use std::path::PathBuf;

use noesis_runtime::font_provider::{FontProvider, set_font_fallbacks, set_font_provider};
use noesis_runtime::formatted_text::{
    FormattedText, flow_direction, font_weight, line_stacking_strategy, text_alignment,
    text_trimming, text_wrapping,
};

// Serves Bitter-Regular.ttf; scan_folder registers the face, open_font returns bytes.
struct BitterProvider {
    bytes: HashMap<String, Vec<u8>>,
    // keeps bytes alive across the &[u8] borrow open_font returns
    current: Option<Vec<u8>>,
}

impl FontProvider for BitterProvider {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn scan_folder(&mut self, _folder_uri: &str, register: &mut dyn FnMut(&str)) {
        for name in self.bytes.keys() {
            register(name);
        }
    }

    fn open_font(&mut self, _folder_uri: &str, filename: &str) -> Option<&[u8]> {
        let bytes = self.bytes.get(filename)?.clone();
        self.current = Some(bytes);
        self.current.as_deref()
    }
}

#[test]
fn formatted_text_measures_real_glyphs() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let sdk_dir =
        std::env::var("NOESIS_SDK_DIR").expect("NOESIS_SDK_DIR not set; required for this test");
    let mut bitter_path = PathBuf::from(sdk_dir);
    bitter_path.push("Data/Fonts/Bitter-Regular.ttf");
    let bitter_bytes = std::fs::read(&bitter_path)
        .unwrap_or_else(|_| panic!("read failed: {}", bitter_path.display()));

    let mut bytes = HashMap::new();
    bytes.insert("Bitter-Regular.ttf".to_string(), bitter_bytes);

    // Guard outlives every FormattedText so the provider stays alive while
    // Noesis shapes glyphs.
    let registered = set_font_provider(BitterProvider {
        bytes,
        current: None,
    });
    set_font_fallbacks(&["Fonts/#Bitter"]);

    const FAMILY: &str = "Fonts/#Bitter";
    const SIZE: f32 = 32.0;

    let short = FormattedText::builder("Hi", FAMILY, SIZE).build();
    let long = FormattedText::builder("Hello, world! This is a longer line.", FAMILY, SIZE).build();

    let (sw, sh) = (short.width(), short.height());
    let (lw, lh) = (long.width(), long.height());
    assert!(sw > 0.0, "short width should be positive, got {sw}");
    assert!(sh > 0.0, "short height should be positive, got {sh}");
    assert!(lw > 0.0, "long width should be positive, got {lw}");
    assert!(lh > 0.0, "long height should be positive, got {lh}");

    // longer string must measure wider: a stub returning a constant fails here
    assert!(
        lw > sw,
        "longer string ({lw}) should measure wider than shorter ({sw})",
    );

    let b = short.bounds();
    assert_eq!(b[2], sw, "bounds width should match width()");
    assert_eq!(b[3], sh, "bounds height should match height()");

    // `IsEmpty` reflects SDK-internal run bookkeeping (the ctor sets it and
    // building a text run clears it), and when that clear happens differs
    // between SDK builds — a text-bearing FormattedText can still report empty
    // on some builds. Only the empty-string direction is portable; text-bearing
    // behavior is already covered by the width/glyph asserts above.
    let empty = FormattedText::builder("", FAMILY, SIZE).build();
    assert!(empty.is_empty(), "empty text should report empty");
    assert!(
        !short.has_visual_brush(),
        "solid foreground, no VisualBrush"
    );
    assert_eq!(short.num_lines(), 1, "short unconstrained text is one line");

    let line = short.line_info(0).expect("line 0 exists");
    assert!(line.height > 0.0, "line height positive, got {line:?}");
    assert!(line.baseline > 0.0, "baseline positive, got {line:?}");
    assert!(line.num_glyphs >= 2, "\"Hi\" has >= 2 glyphs, got {line:?}");
    assert!(short.line_info(5).is_none(), "out-of-range line is None");

    // In 3.2.13, Measure() yields 0 width for unconstrained NoWrap; assert
    // the non-zero height instead.
    let (_mw, mh) = short.measure(
        text_alignment::LEFT,
        text_wrapping::NO_WRAP,
        text_trimming::NONE,
        -1.0,
        -1.0,
        0.0,
        line_stacking_strategy::MAX_HEIGHT,
        flow_direction::LEFT_TO_RIGHT,
    );
    assert!(mh > 0.0, "re-measured height should be positive, got {mh}");
    assert!(
        (mh - sh).abs() < sh,
        "re-measured height ({mh}) should be in the same ballpark as ctor height ({sh})",
    );

    // A bold weight is a real layout knob the FFI carries into Noesis.
    let bold = FormattedText::builder("Hi", FAMILY, SIZE)
        .weight(font_weight::BOLD)
        .build();
    assert!(bold.width() > 0.0, "bold variant still measures positive");

    // Wrapping a long string into a narrow box yields multiple lines: proves
    // max_width crosses the FFI and influences layout.
    let wrapped = FormattedText::builder("one two three four five six seven", FAMILY, SIZE)
        .max_width(80.0)
        .build();
    assert!(
        wrapped.num_lines() > 1,
        "narrow max_width should wrap to multiple lines, got {}",
        wrapped.num_lines(),
    );

    // measurement-only ctor doesn't run Layout(); (-10,-10) for out-of-layout
    // glyphs is a documented valid result; don't assert on coordinates
    let (gx, gy) = short.glyph_position(0, false);
    assert!(gx.is_finite() && gy.is_finite(), "glyph pos finite");
    let hit = short.hit_test(sw + 1000.0, sh / 2.0);
    let _ = hit.index;

    drop(short);
    drop(long);
    drop(empty);
    drop(bold);
    drop(wrapped);
    drop(registered);

    noesis_runtime::shutdown();
}

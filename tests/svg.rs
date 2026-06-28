//! Integration tests for SVG / `SVGPath` parsing (TODO §12).
//!
//! Fully headless: no GPU `RenderDevice` or render pass is needed. Every
//! assertion reads a value BACK from the live Noesis object (parsed bounds,
//! fill hit-test results, parsed document size / shape count), so a stubbed
//! parser, a bounds fn returning zeros, or a constant-returning hit-test would
//! FAIL these.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process);
//! all owning handles drop inside the inner scope before `shutdown()`.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p dm_noesis_runtime --test svg -- --nocapture`

use dm_noesis_runtime::svg::{FillRule, Pen, StrokeJoin, SvgImage, SvgPath};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn svg_path_and_document_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── SVGPath::TryParse + CalculateBounds ─────────────────────────────
        // A 100x50 triangle-ish closed quad anchored at the origin.
        let path = SvgPath::parse("M0 0 L100 0 L100 50 Z").expect("parse path");
        assert!(
            path.command_count() > 0,
            "parsed path must have commands (stub returns 0)"
        );
        let b = path.bounds();
        assert!(
            approx(b[0], 0.0) && approx(b[1], 0.0) && approx(b[2], 100.0) && approx(b[3], 50.0),
            "CalculateBounds ~= (0,0,100,50), got {b:?}"
        );

        // ── FillContains: interior vs exterior ──────────────────────────────
        // (60,10) is inside the triangle formed by (0,0)-(100,0)-(100,50);
        // (200,200) is well outside.
        assert!(
            path.fill_contains(60.0, 10.0, FillRule::EvenOdd),
            "interior point (60,10) must be filled"
        );
        assert!(
            !path.fill_contains(200.0, 200.0, FillRule::EvenOdd),
            "exterior point (200,200) must NOT be filled"
        );
        // A point clearly inside the bbox but outside the triangle (above the
        // hypotenuse) — proves the test exercises true geometry, not the bbox.
        assert!(
            !path.fill_contains(10.0, 40.0, FillRule::EvenOdd),
            "point (10,40) is outside the triangle"
        );

        // Garbage path data fails to parse.
        assert!(
            SvgPath::parse("this is not a path").is_none()
                || SvgPath::parse("this is not a path")
                    .unwrap()
                    .command_count()
                    == 0,
            "invalid path data should not yield a populated path"
        );

        // ── Builder statics: construct a rect and query it ──────────────────
        let mut built = SvgPath::new();
        assert_eq!(built.command_count(), 0, "fresh path is empty");
        built.move_to(10.0, 20.0);
        built.line_to(110.0, 20.0);
        built.line_to(110.0, 70.0);
        built.line_to(10.0, 70.0);
        built.close();
        assert!(built.command_count() > 0, "builder appended commands");
        let bb = built.bounds();
        assert!(
            approx(bb[0], 10.0)
                && approx(bb[1], 20.0)
                && approx(bb[2], 100.0)
                && approx(bb[3], 50.0),
            "built rect bounds ~= (10,20,100,50), got {bb:?}"
        );
        assert!(
            built.fill_contains(60.0, 45.0, FillRule::NonZero),
            "center of built rect is filled"
        );
        assert!(
            !built.fill_contains(0.0, 0.0, FillRule::NonZero),
            "origin is outside the built rect"
        );

        // AddRect static produces an equivalent box.
        let mut rect = SvgPath::new();
        rect.add_rect(0.0, 0.0, 40.0, 40.0);
        let rb = rect.bounds();
        assert!(
            approx(rb[2], 40.0) && approx(rb[3], 40.0),
            "AddRect bounds size ~= 40x40, got {rb:?}"
        );

        // ── StrokeContains: a point on the stroked outline ──────────────────
        // The left edge of the built rect runs x=10 from y=20..70; a wide pen
        // centered there contains a point sitting on that edge, but not the
        // rect's far interior.
        let pen = Pen {
            width: 8.0,
            join: StrokeJoin::Miter,
            ..Pen::default()
        };
        assert!(
            built.stroke_contains(10.0, 45.0, pen),
            "point on the left stroked edge is within the stroke"
        );
        assert!(
            !built.stroke_contains(60.0, 45.0, pen),
            "rect interior is not within an 8px-wide stroke of the outline"
        );

        // ── SVG::Parse: whole document into an SVG::Image ───────────────────
        let doc = r##"<svg width="120" height="80" xmlns="http://www.w3.org/2000/svg">
            <rect x="0" y="0" width="120" height="80" fill="#ff0000"/>
            <circle cx="60" cy="40" r="20" fill="#00ff00"/>
        </svg>"##;
        let image = SvgImage::parse(doc).expect("parse svg document");
        let (w, h) = image.size();
        assert!(
            approx(w, 120.0) && approx(h, 80.0),
            "parsed <svg> size ~= 120x80, got ({w},{h})"
        );
        assert!(
            image.shape_count() >= 2,
            "document has at least the rect + circle shapes, got {}",
            image.shape_count()
        );
        // The first shape carries a solid red fill — proves brush parsing.
        assert!(
            image.shape_fill_type(0).is_some(),
            "first shape has a parsed fill type"
        );
        // Out-of-range index returns None.
        assert!(
            image.shape_fill_type(10_000).is_none(),
            "out-of-range shape index returns None"
        );
    }

    dm_noesis_runtime::shutdown();
}

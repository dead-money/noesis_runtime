//! Integration tests for code-built `Shape` elements (TODO §10): `Rectangle`,
//! `Ellipse`, and `Line`.
//!
//! Headless object construction + setter + read-back: no GPU is needed. Every
//! assertion reads a value BACK from the live Noesis object (`rect.radius_x()`,
//! `shape.stroke_thickness()`, `line.points()`, …) or asserts pointer identity
//! between the brush handed to `set_fill`/`set_stroke` and the one read back via
//! `fill_raw`/`stroke_raw`, so a stubbed/no-op shim fails the round-trip.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process):
//! all owning wrappers drop inside the inner scope before `shutdown()`.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p noesis_runtime --test shapes -- --nocapture`

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::shapes::{Ellipse, Line, PenLineCap, PenLineJoin, Rectangle, Shape, Stretch};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

// Exercise the full shared `Shape` surface against a concrete shape, proving
// every base-class setter crossed the FFI by reading it back.
fn exercise_shape_base<S: Shape>(shape: &mut S) {
    // Width / Height (inherited FrameworkElement DPs).
    shape.set_width(120.0);
    shape.set_height(64.0);
    assert!(approx(shape.width(), 120.0), "width round-trip");
    assert!(approx(shape.height(), 64.0), "height round-trip");

    // Fill / Stroke — reuse the brush wrappers, verify by pointer identity.
    let fill = SolidColorBrush::new([0.9, 0.1, 0.2, 1.0]);
    let stroke = SolidColorBrush::new([0.1, 0.2, 0.9, 1.0]);
    assert!(shape.fill_raw().is_null(), "fill starts unset");
    shape.set_fill(&fill);
    shape.set_stroke(&stroke);
    assert_eq!(
        shape.fill_raw(),
        fill.raw(),
        "fill brush identity round-trip"
    );
    assert_eq!(
        shape.stroke_raw(),
        stroke.raw(),
        "stroke brush identity round-trip"
    );
    shape.clear_fill();
    assert!(shape.fill_raw().is_null(), "clear_fill removes the brush");

    // Stroke scalars.
    shape.set_stroke_thickness(3.5);
    assert!(approx(shape.stroke_thickness(), 3.5), "stroke thickness");
    shape.set_stroke_miter_limit(7.0);
    assert!(approx(shape.stroke_miter_limit(), 7.0), "miter limit");
    shape.set_stroke_dash_offset(2.5);
    assert!(approx(shape.stroke_dash_offset(), 2.5), "dash offset");

    // Trim.
    shape.set_trim_start(0.1);
    shape.set_trim_end(0.8);
    shape.set_trim_offset(0.05);
    assert!(approx(shape.trim_start(), 0.1), "trim start");
    assert!(approx(shape.trim_end(), 0.8), "trim end");
    assert!(approx(shape.trim_offset(), 0.05), "trim offset");

    // Stroke enums (each distinct from the default so a stub can't pass).
    shape.set_stroke_dash_cap(PenLineCap::Round);
    shape.set_stroke_start_line_cap(PenLineCap::Square);
    shape.set_stroke_end_line_cap(PenLineCap::Triangle);
    shape.set_stroke_line_join(PenLineJoin::Bevel);
    shape.set_stretch(Stretch::Uniform);
    assert_eq!(shape.stroke_dash_cap(), Some(PenLineCap::Round), "dash cap");
    assert_eq!(
        shape.stroke_start_line_cap(),
        Some(PenLineCap::Square),
        "start cap"
    );
    assert_eq!(
        shape.stroke_end_line_cap(),
        Some(PenLineCap::Triangle),
        "end cap"
    );
    assert_eq!(
        shape.stroke_line_join(),
        Some(PenLineJoin::Bevel),
        "line join"
    );
    assert_eq!(shape.stretch(), Some(Stretch::Uniform), "stretch");

    // StrokeDashArray (string form per the SDK).
    shape.set_stroke_dash_array("2 1 3");
    assert_eq!(shape.stroke_dash_array(), "2 1 3", "dash array string");
}

#[test]
fn shapes_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // ── Rectangle: shared surface + RadiusX/RadiusY ─────────────────────
        let mut rect = Rectangle::new();
        exercise_shape_base(&mut rect);
        rect.set_radius_x(6.0);
        rect.set_radius_y(4.0);
        assert!(
            approx(rect.radius_x(), 6.0),
            "rectangle radius_x round-trip"
        );
        assert!(
            approx(rect.radius_y(), 4.0),
            "rectangle radius_y round-trip"
        );

        // ── Ellipse: shared surface only ────────────────────────────────────
        let mut ellipse = Ellipse::new();
        exercise_shape_base(&mut ellipse);

        // ── Line: shared surface + endpoints ────────────────────────────────
        let mut line = Line::new();
        exercise_shape_base(&mut line);
        line.set_points(10.0, 20.0, 30.0, 40.0);
        assert_eq!(
            line.points(),
            [10.0, 20.0, 30.0, 40.0],
            "line endpoints round-trip"
        );

        // Distinct pointers (sanity: three independent objects).
        assert_ne!(rect.raw(), ellipse.raw());
        assert_ne!(rect.raw(), line.raw());
    }

    noesis_runtime::shutdown();
}

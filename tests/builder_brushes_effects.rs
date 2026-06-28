//! Phase 5 — gradient-brush builders + `DropShadowEffect` struct-arg ctor/setters.
//!
//! Fail-if-stubbed: every assertion reads a value BACK from the live Noesis
//! object (points / radius / spread method / mapping mode / stops / shadow
//! params), and the builder result is compared against the longhand `new()` +
//! `set_*` form to prove equivalence.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).

use dm_noesis_runtime::brushes::{
    BrushMappingMode, DropShadowEffect, DropShadowParams, GradientSpreadMethod, GradientStop,
    LinearGradientBrush, RadialGradientBrush,
};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

fn approx4(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| approx(*x, *y))
}

#[test]
fn builder_brushes_effects_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── LinearGradientBrush builder ─────────────────────────────────────
        let lin = LinearGradientBrush::builder()
            .start(0.1, 0.2)
            .end(0.8, 0.9)
            .spread_method(GradientSpreadMethod::Reflect)
            .mapping_mode(BrushMappingMode::Absolute)
            .stop(0.0, [1.0, 0.0, 0.0, 1.0])
            .stop(1.0, [0.0, 0.0, 1.0, 1.0])
            .build();

        let (s, e) = lin.points();
        assert!(
            approx(s[0], 0.1) && approx(s[1], 0.2),
            "linear start round-trip"
        );
        assert!(
            approx(e[0], 0.8) && approx(e[1], 0.9),
            "linear end round-trip"
        );
        assert_eq!(
            lin.spread_method(),
            Some(GradientSpreadMethod::Reflect),
            "linear spread method round-trip"
        );
        assert_eq!(
            lin.mapping_mode(),
            Some(BrushMappingMode::Absolute),
            "linear mapping mode round-trip"
        );
        assert_eq!(lin.stop_count(), 2, "two stops appended");
        let st0 = lin.stop(0).expect("stop 0");
        let st1 = lin.stop(1).expect("stop 1");
        assert!(approx(st0.offset, 0.0) && approx4(st0.color, [1.0, 0.0, 0.0, 1.0]));
        assert!(approx(st1.offset, 1.0) && approx4(st1.color, [0.0, 0.0, 1.0, 1.0]));

        // Equivalence with the longhand form.
        let mut longhand = LinearGradientBrush::new();
        longhand.set_start_point(0.1, 0.2);
        longhand.set_end_point(0.8, 0.9);
        assert!(longhand.set_spread_method(GradientSpreadMethod::Reflect));
        assert!(longhand.set_mapping_mode(BrushMappingMode::Absolute));
        longhand.add_stop(GradientStop::new(0.0, [1.0, 0.0, 0.0, 1.0]));
        longhand.add_stop(GradientStop::new(1.0, [0.0, 0.0, 1.0, 1.0]));
        assert_eq!(
            lin.points(),
            longhand.points(),
            "builder == longhand (points)"
        );
        assert_eq!(
            lin.spread_method(),
            longhand.spread_method(),
            "builder == longhand (spread)"
        );
        assert_eq!(
            lin.mapping_mode(),
            longhand.mapping_mode(),
            "builder == longhand (mapping)"
        );
        assert_eq!(lin.stop_count(), longhand.stop_count());

        // ── RadialGradientBrush builder ─────────────────────────────────────
        let rad = RadialGradientBrush::builder()
            .center(0.5, 0.5)
            .gradient_origin(0.4, 0.6)
            .radius(0.75, 0.25)
            .spread_method(GradientSpreadMethod::Repeat)
            .mapping_mode(BrushMappingMode::RelativeToBoundingBox)
            .stop(0.5, [0.2, 0.2, 0.2, 1.0])
            .build();
        let (rx, ry) = rad.radius();
        assert!(
            approx(rx, 0.75) && approx(ry, 0.25),
            "radial radius round-trip"
        );
        assert_eq!(rad.spread_method(), Some(GradientSpreadMethod::Repeat));
        assert_eq!(
            rad.mapping_mode(),
            Some(BrushMappingMode::RelativeToBoundingBox)
        );
        assert_eq!(rad.stop_count(), 1, "one radial stop");

        // ── DropShadowEffect from_params + setters ──────────────────────────
        let params = DropShadowParams {
            color: [0.1, 0.2, 0.3, 0.8],
            blur_radius: 6.0,
            direction: 315.0,
            shadow_depth: 4.0,
            opacity: 0.7,
        };
        let shadow = DropShadowEffect::from_params(params);
        assert_eq!(shadow.params(), params, "from_params round-trip");

        // Equivalent to the 5-positional-arg constructor.
        let longhand_shadow = DropShadowEffect::new([0.1, 0.2, 0.3, 0.8], 6.0, 315.0, 4.0, 0.7);
        assert_eq!(
            shadow.params(),
            longhand_shadow.params(),
            "from_params == new() positional"
        );

        // Individual setters mutate the live object.
        let mut s2 = DropShadowEffect::from_params(DropShadowParams::default());
        s2.set_color([0.9, 0.8, 0.7, 1.0]);
        s2.set_blur_radius(12.0);
        s2.set_direction(90.0);
        s2.set_shadow_depth(3.0);
        s2.set_opacity(0.5);
        let p = s2.params();
        assert!(
            approx4(p.color, [0.9, 0.8, 0.7, 1.0]),
            "set_color round-trip"
        );
        assert!(approx(p.blur_radius, 12.0), "set_blur_radius round-trip");
        assert!(approx(p.direction, 90.0), "set_direction round-trip");
        assert!(approx(p.shadow_depth, 3.0), "set_shadow_depth round-trip");
        assert!(approx(p.opacity, 0.5), "set_opacity round-trip");

        // set_params replaces every field at once.
        s2.set_params(params);
        assert_eq!(s2.params(), params, "set_params round-trip");
    }

    dm_noesis_runtime::shutdown();
}

//! Struct-arg geometry constructors (`ArcSegment::from_fields`,
//! `RectangleGeometry::from_rect`): round-tripped and compared to the positional form.

use noesis_runtime::geometry::{ArcFields, ArcSegment, Rect, RectangleGeometry, SweepDirection};

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

#[test]
fn builder_geometry_struct_args_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let fields = ArcFields {
            point: (10.0, 20.0),
            size: (5.0, 7.0),
            rotation_deg: 45.0,
            is_large_arc: true,
            sweep: SweepDirection::Clockwise,
        };
        let arc = ArcSegment::from_fields(fields);
        let got = arc.get();
        assert!(
            approx(got.point.0, 10.0) && approx(got.point.1, 20.0),
            "arc point"
        );
        assert!(
            approx(got.size.0, 5.0) && approx(got.size.1, 7.0),
            "arc size"
        );
        assert!(approx(got.rotation_deg, 45.0), "arc rotation");
        assert!(got.is_large_arc, "arc is_large_arc");
        assert_eq!(got.sweep, SweepDirection::Clockwise, "arc sweep");
        assert_eq!(got, fields, "from_fields round-trips with get()");

        let positional =
            ArcSegment::new(10.0, 20.0, 5.0, 7.0, 45.0, true, SweepDirection::Clockwise);
        assert_eq!(
            arc.get(),
            positional.get(),
            "from_fields == new() positional"
        );

        let r = Rect {
            x: 1.0,
            y: 2.0,
            width: 30.0,
            height: 40.0,
        };
        let rg = RectangleGeometry::from_rect(r, (3.0, 4.0));
        let rect = rg.rect();
        assert!(
            approx(rect[0], 1.0)
                && approx(rect[1], 2.0)
                && approx(rect[2], 30.0)
                && approx(rect[3], 40.0),
            "from_rect rect round-trip"
        );
        let (rx, ry) = rg.radii();
        assert!(
            approx(rx, 3.0) && approx(ry, 4.0),
            "from_rect radii round-trip"
        );

        let positional_rect = RectangleGeometry::new(1.0, 2.0, 30.0, 40.0, 3.0, 4.0);
        assert_eq!(
            rg.rect(),
            positional_rect.rect(),
            "from_rect == new() (rect)"
        );
        assert_eq!(
            rg.radii(),
            positional_rect.radii(),
            "from_rect == new() (radii)"
        );
    }

    noesis_runtime::shutdown();
}

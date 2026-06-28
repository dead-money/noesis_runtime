//! Code-built Geometry object model: headless construction + read-back.
//! Bounds, figure/segment counts, enum round-trips, and Path.Data assignment.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p noesis_runtime --test geometry -- --nocapture`

use noesis_runtime::geometry::{
    ArcSegment, BezierSegment, CombinedGeometry, EllipseGeometry, FillRule, Geometry,
    GeometryCombineMode, GeometryGroup, LineGeometry, LineSegment, PathFigure, PathGeometry,
    PolyBezierSegment, PolyLineSegment, PolyQuadraticBezierSegment, QuadraticBezierSegment,
    RectangleGeometry, StreamGeometry, SweepDirection,
};
use noesis_runtime::transforms::TranslateTransform;
use noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-3
}

#[test]
fn geometry_object_model_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let ellipse = EllipseGeometry::new(50.0, 60.0, 40.0, 30.0);
        assert_eq!(ellipse.get(), [50.0, 60.0, 40.0, 30.0], "ellipse fields");
        let eb = ellipse.bounds();
        // x∈[10,90], y∈[30,90] for center (50,60) radii (40,30)
        assert!(
            approx(eb.x, 10.0) && approx(eb.y, 30.0),
            "ellipse bounds origin"
        );
        assert!(
            approx(eb.width, 80.0) && approx(eb.height, 60.0),
            "ellipse bounds size"
        );
        assert!(!ellipse.is_empty(), "ellipse non-empty");
        // GetRenderBounds with a null pen equals fill bounds
        let erb = ellipse.render_bounds();
        assert!(
            approx(erb.x, 10.0)
                && approx(erb.y, 30.0)
                && approx(erb.width, 80.0)
                && approx(erb.height, 60.0),
            "ellipse render bounds (null pen == fill bounds): {erb:?}"
        );

        let rectg = RectangleGeometry::new(5.0, 6.0, 20.0, 10.0, 2.0, 3.0);
        assert_eq!(rectg.rect(), [5.0, 6.0, 20.0, 10.0], "rectangle rect");
        assert_eq!(rectg.radii(), (2.0, 3.0), "rectangle radii");
        let rb = rectg.bounds();
        assert!(
            approx(rb.x, 5.0)
                && approx(rb.y, 6.0)
                && approx(rb.width, 20.0)
                && approx(rb.height, 10.0),
            "rectangle bounds"
        );

        let line = LineGeometry::new(0.0, 0.0, 100.0, 40.0);
        assert_eq!(line.get(), [0.0, 0.0, 100.0, 40.0], "line points");
        let lb = line.bounds();
        assert!(
            approx(lb.width, 100.0) && approx(lb.height, 40.0),
            "line bounds"
        );

        // GetBounds in 3.2.13 reports untransformed geometry; prove assignment
        // via pointer identity, not a bounds shift.
        let mut ellipse2 = EllipseGeometry::new(0.0, 0.0, 10.0, 10.0);
        let shift = TranslateTransform::new(100.0, 0.0);
        assert!(ellipse2.set_transform(&shift), "set geometry transform");
        assert_eq!(
            ellipse2.transform_raw(),
            shift.raw(),
            "geometry holds the exact transform assigned"
        );
        drop(shift);

        let mut stream = StreamGeometry::new();
        assert!(stream.is_empty(), "fresh stream geometry empty");
        {
            let ctx = stream.open();
            ctx.begin_figure(0.0, 0.0, true);
            ctx.line_to(100.0, 0.0);
            ctx.line_to(100.0, 50.0);
            ctx.line_to(0.0, 50.0);
            ctx.close(); // flush into the geometry
        }
        let sb = stream.bounds();
        assert!(
            approx(sb.width, 100.0) && approx(sb.height, 50.0),
            "stream geometry bounds after context close: {sb:?}"
        );
        assert!(!stream.is_empty(), "stream geometry non-empty after close");

        // A context dropped WITHOUT close() must leave the geometry unaltered.
        let untouched = StreamGeometry::new();
        {
            let ctx = untouched.open();
            ctx.begin_figure(0.0, 0.0, true);
            ctx.line_to(999.0, 999.0);
            // ctx dropped here without close()
        }
        assert!(
            untouched.is_empty(),
            "dropped (unclosed) context leaves geometry empty"
        );

        // no getter on context — bounds are the only observable
        let quad = StreamGeometry::new();
        {
            let ctx = quad.open();
            ctx.begin_figure(0.0, 0.0, false);
            ctx.quadratic_to((50.0, 120.0), (100.0, 0.0));
            ctx.close();
        }
        let qb = quad.bounds();
        assert!(
            approx(qb.width, 100.0) && qb.height > 50.0,
            "quadratic_to curve reaches the end point and bulges past y=50: {qb:?}"
        );

        let cubic = StreamGeometry::new();
        {
            let ctx = cubic.open();
            ctx.begin_figure(0.0, 0.0, false);
            ctx.cubic_to((0.0, 150.0), (100.0, 150.0), (100.0, 0.0));
            ctx.close();
        }
        let cub = cubic.bounds();
        assert!(
            approx(cub.width, 100.0) && cub.height > 50.0,
            "cubic_to curve reaches the end point and bulges past y=50: {cub:?}"
        );

        let arc = StreamGeometry::new();
        {
            let ctx = arc.open();
            ctx.begin_figure(0.0, 0.0, false);
            // A semicircular arc (chord 100, radii 50) bulges to y≈50.
            ctx.arc_to(
                100.0,
                0.0,
                50.0,
                50.0,
                0.0,
                false,
                SweepDirection::Clockwise,
            );
            ctx.close();
        }
        let ab = arc.bounds();
        assert!(
            approx(ab.width, 100.0) && ab.height > 10.0,
            "arc_to curve reaches the end point and bulges into a real box: {ab:?}"
        );

        // GetBounds is identical for open vs closed (3.2.13); proof of crossing
        // is that flush still produces a correctly-bounded geometry.
        let closed = StreamGeometry::new();
        {
            let ctx = closed.open();
            ctx.begin_figure(10.0, 10.0, false);
            ctx.line_to(60.0, 10.0);
            ctx.line_to(60.0, 40.0);
            ctx.set_is_closed(true);
            ctx.close();
        }
        let clb = closed.bounds();
        assert!(
            !closed.is_empty() && approx(clb.width, 50.0) && approx(clb.height, 30.0),
            "set_is_closed figure flushes a correctly-bounded geometry: {clb:?}"
        );

        assert_eq!(
            stream.fill_rule(),
            FillRule::EvenOdd,
            "stream default fill rule"
        );
        stream.set_fill_rule(FillRule::Nonzero);
        assert_eq!(stream.fill_rule(), FillRule::Nonzero, "stream fill rule");

        // set_data() rebuilds in place — bounds must follow the new path-data
        let mut reshaped = StreamGeometry::from_data("M 0,0 L 10,0 10,10 Z");
        let r0 = reshaped.bounds();
        assert!(
            approx(r0.width, 10.0) && approx(r0.height, 10.0),
            "initial path-data bounds: {r0:?}"
        );
        reshaped.set_data("M 0,0 L 40,0 40,20 0,20 Z");
        let r1 = reshaped.bounds();
        assert!(
            approx(r1.width, 40.0) && approx(r1.height, 20.0),
            "set_data rebuilds the geometry bounds: {r1:?}"
        );

        let svg = StreamGeometry::from_data("M 0,0 L 60,0 60,60 0,60 Z");
        let svb = svg.bounds();
        assert!(
            approx(svb.width, 60.0) && approx(svb.height, 60.0),
            "svg data bounds"
        );

        let mut figure = PathFigure::new();
        figure.set_start_point(10.0, 10.0);
        assert_eq!(figure.start_point(), (10.0, 10.0), "figure start point");
        figure.set_is_closed(true);
        figure.set_is_filled(false);
        assert!(figure.is_closed(), "figure closed");
        assert!(!figure.is_filled(), "figure not filled");

        let lseg = LineSegment::new(110.0, 10.0);
        assert_eq!(lseg.point(), (110.0, 10.0), "line segment point");
        assert_eq!(figure.add_segment(&lseg), 0, "add line segment index");

        let qseg = QuadraticBezierSegment::new((120.0, 30.0), (110.0, 60.0));
        assert_eq!(
            qseg.points(),
            [(120.0, 30.0), (110.0, 60.0)],
            "quadratic points"
        );
        assert_eq!(figure.add_segment(&qseg), 1, "add quadratic segment index");

        let bseg = BezierSegment::new((90.0, 70.0), (40.0, 70.0), (10.0, 40.0));
        assert_eq!(
            bseg.points(),
            [(90.0, 70.0), (40.0, 70.0), (10.0, 40.0)],
            "bezier points"
        );
        assert_eq!(figure.add_segment(&bseg), 2, "add bezier segment index");

        let aseg = ArcSegment::new(
            10.0,
            10.0,
            20.0,
            25.0,
            30.0,
            true,
            SweepDirection::Clockwise,
        );
        let af = aseg.get();
        assert_eq!(af.point, (10.0, 10.0), "arc point");
        assert_eq!(af.size, (20.0, 25.0), "arc size");
        assert!(approx(af.rotation_deg, 30.0), "arc rotation");
        assert!(af.is_large_arc, "arc large flag");
        assert_eq!(af.sweep, SweepDirection::Clockwise, "arc sweep");
        assert_eq!(figure.add_segment(&aseg), 3, "add arc segment index");

        let poly = PolyLineSegment::new(&[(5.0, 5.0), (6.0, 7.0), (8.0, 9.0)]);
        assert_eq!(poly.point_count(), 3, "poly line point count");
        assert_eq!(poly.point(1), Some((6.0, 7.0)), "poly line point read back");
        assert_eq!(poly.point(9), None, "poly line point out of range");
        assert_eq!(figure.add_segment(&poly), 4, "add poly line segment index");

        let pbez = PolyBezierSegment::new(&[(120.0, 10.0), (140.0, 30.0), (120.0, 50.0)]);
        assert_eq!(pbez.point_count(), 3, "poly bezier point count");
        assert_eq!(
            pbez.point(2),
            Some((120.0, 50.0)),
            "poly bezier point read back"
        );
        assert_eq!(pbez.point(3), None, "poly bezier point out of range");
        assert_eq!(
            figure.add_segment(&pbez),
            5,
            "add poly bezier segment index"
        );

        let pquad = PolyQuadraticBezierSegment::new(&[(130.0, 20.0), (140.0, 45.0)]);
        assert_eq!(pquad.point_count(), 2, "poly quadratic point count");
        assert_eq!(
            pquad.point(0),
            Some((130.0, 20.0)),
            "poly quadratic point read back"
        );
        assert_eq!(pquad.point(2), None, "poly quadratic point out of range");
        assert_eq!(
            figure.add_segment(&pquad),
            6,
            "add poly quadratic segment index"
        );

        assert_eq!(figure.segment_count(), 7, "figure has seven segments");

        let mut path = PathGeometry::new();
        assert_eq!(path.figure_count(), 0, "path starts with no figures");
        assert_eq!(path.add_figure(&figure), 0, "add figure index");
        assert_eq!(path.figure_count(), 1, "path has one figure");
        assert_eq!(
            path.fill_rule(),
            FillRule::EvenOdd,
            "path default fill rule"
        );
        path.set_fill_rule(FillRule::Nonzero);
        assert_eq!(path.fill_rule(), FillRule::Nonzero, "path fill rule");
        assert!(!path.is_empty(), "path geometry non-empty");
        // Segments/figure dropped after add: Noesis holds its own references.
        drop(figure);
        drop(lseg);
        drop(qseg);
        drop(bseg);
        drop(aseg);
        drop(poly);
        drop(pbez);
        drop(pquad);
        assert_eq!(
            path.figure_count(),
            1,
            "figure survives builder drop (AddRef)"
        );

        let a = RectangleGeometry::new(0.0, 0.0, 50.0, 50.0, 0.0, 0.0);
        let b = RectangleGeometry::new(25.0, 25.0, 50.0, 50.0, 0.0, 0.0);
        let combined = CombinedGeometry::new(GeometryCombineMode::Union, &a, &b);
        assert_eq!(
            combined.mode(),
            Some(GeometryCombineMode::Union),
            "combine mode"
        );
        // Union bounds span both rectangles: x∈[0,75], y∈[0,75].
        let cb = combined.bounds();
        assert!(
            approx(cb.width, 75.0) && approx(cb.height, 75.0),
            "combined union bounds: {cb:?}"
        );
        assert_eq!(
            combined.geometry1_raw(),
            a.geometry_raw(),
            "combined operand 1 identity"
        );
        assert_eq!(
            combined.geometry2_raw(),
            b.geometry_raw(),
            "combined operand 2 identity"
        );
        let mut combined = combined;
        combined.set_mode(GeometryCombineMode::Intersect);
        assert_eq!(
            combined.mode(),
            Some(GeometryCombineMode::Intersect),
            "combine mode updated"
        );
        // Intersection bounds: x∈[25,50], y∈[25,50] => 25x25.
        let ib = combined.bounds();
        assert!(
            approx(ib.width, 25.0) && approx(ib.height, 25.0),
            "combined intersect bounds: {ib:?}"
        );

        let c = RectangleGeometry::new(0.0, 0.0, 30.0, 30.0, 0.0, 0.0);
        let d = RectangleGeometry::new(0.0, 0.0, 40.0, 40.0, 0.0, 0.0);
        combined.set_geometry1(&c);
        combined.set_geometry2(&d);
        assert_eq!(
            combined.geometry1_raw(),
            c.geometry_raw(),
            "set_geometry1 replaced operand 1 (identity)"
        );
        assert_eq!(
            combined.geometry2_raw(),
            d.geometry_raw(),
            "set_geometry2 replaced operand 2 (identity)"
        );
        let ncb = combined.bounds();
        assert!(
            approx(ncb.width, 30.0) && approx(ncb.height, 30.0),
            "bounds follow the replaced operands: {ncb:?}"
        );
        drop(a);
        drop(b);
        drop(c);
        drop(d);

        let mut group = GeometryGroup::new();
        assert_eq!(group.child_count(), 0, "group starts empty");
        let g1 = RectangleGeometry::new(0.0, 0.0, 10.0, 10.0, 0.0, 0.0);
        let g2 = EllipseGeometry::new(100.0, 100.0, 10.0, 10.0);
        assert_eq!(group.add_child(&g1), 0, "add child 0");
        assert_eq!(group.add_child(&g2), 1, "add child 1");
        assert_eq!(group.child_count(), 2, "group has two children");
        assert_eq!(
            group.fill_rule(),
            FillRule::EvenOdd,
            "group default fill rule"
        );
        group.set_fill_rule(FillRule::Nonzero);
        assert_eq!(group.fill_rule(), FillRule::Nonzero, "group fill rule");
        // GetBounds is lazy in 3.2.13 (empty until rendered); child_count is the proof
        drop(g1);
        drop(g2);
        assert_eq!(
            group.child_count(),
            2,
            "children survive builder drop (AddRef)"
        );

        let path_xaml = format!("<Path {NS} Stroke=\"Black\"/>");
        let mut path_el = FrameworkElement::parse(&path_xaml).expect("parse Path");
        assert!(
            path_el.get_component("Data").is_none(),
            "Path Data starts unset"
        );
        let data = EllipseGeometry::new(20.0, 20.0, 15.0, 15.0);
        // SAFETY: path_el is a live Path element; the geometry pointer is borrowed
        // and Noesis takes its own reference (AddRef).
        assert!(
            unsafe { path_el.set_component("Data", data.geometry_raw()) },
            "set Path Data"
        );
        let read = path_el
            .get_component("Data")
            .expect("Path Data set after assignment");
        // Noesis stores the *same* object (AddRef, not clone): pointer identity.
        assert_eq!(
            read.as_ptr(),
            data.geometry_raw(),
            "Path Data is the exact geometry assigned"
        );
    }

    noesis_runtime::shutdown();
}

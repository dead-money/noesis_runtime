//! Integration tests for code-built brushes, transforms, effects, and
//! `RenderOptions` (TODO §11).
//!
//! Headless object construction + assignment + read-back: no GPU is needed. The
//! assertions are written to fail if any constructor/setter were stubbed —
//! every feature reads at least one value BACK from the live Noesis object
//! (`brush.color()`, `effect.radius()`, `transform.get()`, …) and/or asserts
//! pointer identity between the object handed to Noesis and the one read back
//! through `get_component`.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process):
//! all owning wrappers drop inside the inner scope before `shutdown()`.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p dm_noesis_runtime --test brushes_transforms -- --nocapture`

use dm_noesis_runtime::brushes::{
    BlurEffect, DropShadowEffect, GradientStop, ImageBrush, LinearGradientBrush,
    RadialGradientBrush, SolidColorBrush,
};
use dm_noesis_runtime::transforms::{
    CompositeFields, CompositeTransform, MatrixTransform, RotateTransform, ScaleTransform,
    SkewTransform, TransformGroup, TranslateTransform,
};
use dm_noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

fn approx4(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| approx(*x, *y))
}

#[test]
fn brushes_transforms_effects_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── SolidColorBrush ─────────────────────────────────────────────────
        let red = [0.9_f32, 0.1, 0.2, 1.0];
        let mut brush = SolidColorBrush::new(red);
        // Read color BACK from the live Noesis object — fails if create stubbed.
        assert!(approx4(brush.color(), red), "solid color round-trip");
        brush.set_color([0.0, 0.5, 1.0, 0.5]);
        assert!(
            approx4(brush.color(), [0.0, 0.5, 1.0, 0.5]),
            "solid color set_color round-trip"
        );
        brush.set_color(red);

        // A fresh Border has no Background (discriminator for the assignment test).
        let border_xaml = format!("<Border {NS}/>");
        let mut border = FrameworkElement::parse(&border_xaml).expect("parse Border");
        assert!(
            border.get_component("Background").is_none(),
            "Border Background starts unset"
        );
        assert!(border.set_background(&brush), "set Background");
        let bg = border
            .get_component("Background")
            .expect("Background set after set_background");
        // Noesis stores the *same* object (AddRef, not clone): pointer identity.
        assert_eq!(
            bg.as_ptr(),
            brush.raw(),
            "Background is the exact brush we assigned"
        );

        // ── LinearGradientBrush + stops ─────────────────────────────────────
        let mut grad = LinearGradientBrush::new();
        grad.set_start_point(0.0, 0.0);
        grad.set_end_point(1.0, 1.0);
        let (s, e) = grad.points();
        assert!(
            approx(s[0], 0.0) && approx(e[0], 1.0) && approx(e[1], 1.0),
            "gradient start/end points round-trip"
        );
        assert_eq!(
            grad.add_stop(GradientStop::new(0.0, [0.0, 0.0, 1.0, 1.0])),
            Some(0)
        );
        assert_eq!(
            grad.add_stop(GradientStop::new(1.0, [1.0, 0.0, 0.0, 1.0])),
            Some(1)
        );
        assert_eq!(grad.stop_count(), 2, "two gradient stops");
        let stop0 = grad.stop(0).expect("stop 0");
        let stop1 = grad.stop(1).expect("stop 1");
        assert!(approx(stop0.offset, 0.0) && approx4(stop0.color, [0.0, 0.0, 1.0, 1.0]));
        assert!(approx(stop1.offset, 1.0) && approx4(stop1.color, [1.0, 0.0, 0.0, 1.0]));
        assert!(grad.stop(2).is_none(), "out-of-range stop is None");

        // Assign the gradient to a Rectangle's Fill.
        let rect_xaml = format!("<Rectangle {NS} Width=\"50\" Height=\"50\"/>");
        let mut rect = FrameworkElement::parse(&rect_xaml).expect("parse Rectangle");
        assert!(
            rect.get_component("Fill").is_none(),
            "Rectangle Fill starts unset"
        );
        assert!(rect.set_fill(&grad), "set Fill");
        assert_eq!(
            rect.get_component("Fill").expect("Fill set").as_ptr(),
            grad.raw(),
            "Fill is the exact gradient brush"
        );
        // Border has no Fill DP — the typed sugar must report failure.
        assert!(!border.set_fill(&brush), "Border has no Fill");

        // ── RadialGradientBrush ─────────────────────────────────────────────
        let mut radial = RadialGradientBrush::new();
        radial.set_radius(0.75, 0.25);
        radial.set_center(0.5, 0.5);
        radial.set_gradient_origin(0.4, 0.6);
        let (rx, ry) = radial.radius();
        assert!(
            approx(rx, 0.75) && approx(ry, 0.25),
            "radial radius round-trip"
        );
        radial.add_stop(GradientStop::new(0.5, [0.2, 0.2, 0.2, 1.0]));
        assert_eq!(radial.stop_count(), 1);

        // ── ImageBrush ──────────────────────────────────────────────────────
        // No GPU/imaging surface needed: construct, read the source back through
        // Noesis GetImageSource (None proves it's a real ImageBrush, not a stub),
        // then assign it and verify pointer identity through get_component.
        let ib = ImageBrush::new();
        assert!(
            ib.image_source().is_none(),
            "fresh ImageBrush has no source"
        );
        let border2_xaml = format!("<Border {NS}/>");
        let mut border2 = FrameworkElement::parse(&border2_xaml).expect("parse Border2");
        assert!(
            border2.get_component("Background").is_none(),
            "Border2 Background starts unset"
        );
        assert!(border2.set_background(&ib), "set Background (ImageBrush)");
        assert_eq!(
            border2
                .get_component("Background")
                .expect("Background set after set_background")
                .as_ptr(),
            ib.raw(),
            "Background is the exact ImageBrush we assigned"
        );
        // Source-wiring read-back (setting a real ImageSource* and reading it
        // back) is deferred to §12 imaging, which provides a headless way to
        // construct an ImageSource. `set_image_source` is exercised there.

        // ── Foreground / Stroke typed sugar ─────────────────────────────────
        // set_background and set_fill are covered above; close the gap on the
        // remaining thin wrappers with pointer-identity read-back.
        let tb_xaml = format!("<TextBlock {NS} Text=\"x\"/>");
        let mut tb = FrameworkElement::parse(&tb_xaml).expect("parse TextBlock");
        assert!(tb.set_foreground(&brush), "set Foreground");
        assert_eq!(
            tb.get_component("Foreground")
                .expect("Foreground set")
                .as_ptr(),
            brush.raw(),
            "Foreground is the exact brush we assigned"
        );
        assert!(rect.set_stroke(&radial), "set Stroke");
        assert_eq!(
            rect.get_component("Stroke").expect("Stroke set").as_ptr(),
            radial.raw(),
            "Stroke is the exact radial brush we assigned"
        );

        // ── Transforms ──────────────────────────────────────────────────────
        let translate = TranslateTransform::new(3.0, -4.0);
        assert_eq!(translate.get(), (3.0, -4.0), "translate round-trip");

        let scale = ScaleTransform::new(2.0, 3.0, 10.0, 20.0);
        let s = scale.get();
        assert!(
            approx(s[0], 2.0) && approx(s[1], 3.0) && approx(s[2], 10.0) && approx(s[3], 20.0),
            "scale round-trip"
        );

        let mut rotate = RotateTransform::new(45.0, 5.0, 6.0);
        assert!(approx(rotate.angle(), 45.0), "rotate angle round-trip");
        rotate.set_angle(90.0);
        assert!(approx(rotate.angle(), 90.0), "rotate set_angle round-trip");
        let r = rotate.get();
        assert!(
            approx(r[1], 5.0) && approx(r[2], 6.0),
            "rotate center round-trip"
        );

        let skew = SkewTransform::new(15.0, 25.0, 1.0, 2.0);
        let sk = skew.get();
        assert!(
            approx(sk[0], 15.0) && approx(sk[1], 25.0),
            "skew round-trip"
        );

        let m = [1.0, 0.0, 0.0, 1.0, 7.0, 8.0];
        let matrix = MatrixTransform::new(m);
        let got = matrix.get();
        assert!(
            got.iter().zip(m.iter()).all(|(a, b)| approx(*a, *b)),
            "matrix round-trip"
        );

        let composite = CompositeTransform::new(CompositeFields {
            scale_x: 2.0,
            scale_y: 2.0,
            rotation: 30.0,
            translate_x: 11.0,
            ..CompositeFields::default()
        });
        let cf = composite.get();
        assert!(
            approx(cf.scale_x, 2.0) && approx(cf.rotation, 30.0) && approx(cf.translate_x, 11.0),
            "composite round-trip"
        );

        let mut group = TransformGroup::new();
        assert_eq!(group.child_count(), 0, "group starts empty");
        assert!(group.add_child(&scale));
        assert!(group.add_child(&rotate));
        assert_eq!(group.child_count(), 2, "group has two children");

        // Assign a transform as RenderTransform; pump a layout pass.
        let panel_xaml =
            format!("<Border {NS} Width=\"100\" Height=\"100\"><TextBlock Text=\"x\"/></Border>");
        let mut panel = FrameworkElement::parse(&panel_xaml).expect("parse panel");
        // RenderTransform defaults to a non-null identity transform, so prove the
        // assignment took by checking the read-back pointer is OUR group below.
        assert!(panel.set_render_transform(&group), "set RenderTransform");
        assert_eq!(
            panel
                .get_component("RenderTransform")
                .expect("RenderTransform set")
                .as_ptr(),
            group.raw(),
            "RenderTransform is the exact group we assigned"
        );

        // ── Effects ─────────────────────────────────────────────────────────
        let mut blur = BlurEffect::new(8.0);
        assert!(approx(blur.radius(), 8.0), "blur radius round-trip");
        blur.set_radius(3.5);
        assert!(approx(blur.radius(), 3.5), "blur set_radius round-trip");
        assert!(panel.set_effect(&blur), "set Effect");
        assert_eq!(
            panel.get_component("Effect").expect("Effect set").as_ptr(),
            blur.raw(),
            "Effect is the exact blur we assigned"
        );

        let shadow = DropShadowEffect::new([0.1, 0.2, 0.3, 0.8], 6.0, 315.0, 4.0, 0.7);
        let p = shadow.params();
        assert!(
            approx4(p.color, [0.1, 0.2, 0.3, 0.8]),
            "shadow color round-trip"
        );
        assert!(
            approx(p.blur_radius, 6.0)
                && approx(p.direction, 315.0)
                && approx(p.shadow_depth, 4.0)
                && approx(p.opacity, 0.7),
            "shadow params round-trip"
        );

        // ── RenderOptions ───────────────────────────────────────────────────
        // Default before any set should not equal our HighQuality(2) write.
        assert!(panel.set_bitmap_scaling_mode(2), "set BitmapScalingMode");
        assert_eq!(
            panel.bitmap_scaling_mode(),
            Some(2),
            "BitmapScalingMode round-trip"
        );
        assert!(
            panel.set_bitmap_scaling_mode(1),
            "set BitmapScalingMode again"
        );
        assert_eq!(panel.bitmap_scaling_mode(), Some(1));
    }

    dm_noesis_runtime::shutdown();
}

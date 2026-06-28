//! Integration tests for the remaining TODO §11 visual surface: `VisualBrush`,
//! the full `TileBrush` tiling knobs (shared by `ImageBrush` + `VisualBrush`),
//! and the 3D transforms (`CompositeTransform3D` / `MatrixTransform3D`) assigned
//! to an element via `UIElement::SetTransform3D`.
//!
//! Every assertion reads at least one value BACK from the live Noesis object
//! (enum read-back, `Viewport`/`Viewbox` Rect read-back, 3D float fields, or
//! pointer identity through `get_component` / `GetVisual` / `GetTransform3D`), so
//! a stubbed constructor/setter would fail the round-trip. No GPU is needed.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p dm_noesis_runtime --test visual_brushes_3d -- --nocapture`

use dm_noesis_runtime::brushes::{
    AlignmentX, AlignmentY, BrushMappingMode, ImageBrush, Stretch, TileBrush, TileMode, VisualBrush,
};
use dm_noesis_runtime::transforms::{Composite3DFields, CompositeTransform3D, MatrixTransform3D};
use dm_noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

fn approx4(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(b.iter()).all(|(x, y)| approx(*x, *y))
}

#[test]
fn visual_brush_tile_knobs_and_3d_transforms() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── VisualBrush: source wiring round-trip ───────────────────────────
        // A fresh VisualBrush has no visual; GetVisual returns null (proves a
        // real VisualBrush, not a stub returning a dangling pointer).
        let mut vb = VisualBrush::new();
        assert!(vb.visual().is_none(), "fresh VisualBrush has no visual");

        // Any element is a Visual: wire one and read the pointer back. The
        // Visual subobject sits at offset 0 of the element (single-inheritance
        // chain to BaseComponent), so GetVisual round-trips the exact element
        // pointer.
        let source_xaml = format!("<Border {NS} Width=\"40\" Height=\"40\"/>");
        let source = FrameworkElement::parse(&source_xaml).expect("parse source");
        assert!(vb.set_visual(&source), "set_visual");
        assert_eq!(
            vb.visual().expect("visual set after set_visual").as_ptr(),
            source.raw(),
            "GetVisual returns the exact element we assigned"
        );

        // from_element constructor wires the source at creation time.
        let vb2 = VisualBrush::from_element(&source);
        assert_eq!(
            vb2.visual().expect("from_element visual").as_ptr(),
            source.raw(),
            "from_element wires the visual"
        );

        // clear_visual unsets it.
        assert!(vb.clear_visual(), "clear_visual");
        assert!(vb.visual().is_none(), "visual cleared");
        assert!(vb.set_visual(&source), "setter should succeed");

        // Assign the VisualBrush to a target's Background; pointer identity.
        let target_xaml = format!("<Border {NS}/>");
        let mut target = FrameworkElement::parse(&target_xaml).expect("parse target");
        assert!(
            target.get_component("Background").is_none(),
            "target Background starts unset"
        );
        assert!(target.set_background(&vb), "set Background (VisualBrush)");
        assert_eq!(
            target
                .get_component("Background")
                .expect("Background set")
                .as_ptr(),
            vb.raw(),
            "Background is the exact VisualBrush we assigned"
        );

        // ── TileBrush tiling knobs on VisualBrush ───────────────────────────
        // Defaults differ from our writes, so the read-backs prove the setters
        // crossed the FFI.
        vb.set_alignment_x(AlignmentX::Right);
        vb.set_alignment_y(AlignmentY::Bottom);
        vb.set_stretch(Stretch::UniformToFill);
        vb.set_tile_mode(TileMode::FlipXY);
        vb.set_viewport([0.0, 0.0, 0.5, 0.5]);
        vb.set_viewport_units(BrushMappingMode::Absolute);
        vb.set_viewbox([0.1, 0.2, 0.7, 0.8]);
        vb.set_viewbox_units(BrushMappingMode::RelativeToBoundingBox);

        assert_eq!(vb.alignment_x(), Some(AlignmentX::Right), "vb alignment_x");
        assert_eq!(vb.alignment_y(), Some(AlignmentY::Bottom), "vb alignment_y");
        assert_eq!(vb.stretch(), Some(Stretch::UniformToFill), "vb stretch");
        assert_eq!(vb.tile_mode(), Some(TileMode::FlipXY), "vb tile_mode");
        assert!(approx4(vb.viewport(), [0.0, 0.0, 0.5, 0.5]), "vb viewport");
        assert_eq!(
            vb.viewport_units(),
            Some(BrushMappingMode::Absolute),
            "vb viewport_units"
        );
        assert!(approx4(vb.viewbox(), [0.1, 0.2, 0.7, 0.8]), "vb viewbox");
        assert_eq!(
            vb.viewbox_units(),
            Some(BrushMappingMode::RelativeToBoundingBox),
            "vb viewbox_units"
        );

        // ── Same tiling knobs on ImageBrush (it is also a TileBrush) ────────
        let mut ib = ImageBrush::new();
        // Sanity: a fresh ImageBrush reads back the SDK's default AlignmentX
        // (proves the TileBrush cast works on ImageBrush too, and that we read a
        // real value, not just a valid variant). TileBrush.AlignmentXProperty
        // defaults to AlignmentX::Center in Noesis (matching WPF).
        assert_eq!(
            ib.alignment_x(),
            Some(AlignmentX::Center),
            "fresh ImageBrush should default to AlignmentX::Center"
        );
        ib.set_alignment_x(AlignmentX::Left);
        ib.set_alignment_y(AlignmentY::Top);
        ib.set_stretch(Stretch::None);
        ib.set_tile_mode(TileMode::Tile);
        ib.set_viewport([1.0, 2.0, 3.0, 4.0]);
        ib.set_viewport_units(BrushMappingMode::Absolute);
        ib.set_viewbox([5.0, 6.0, 7.0, 8.0]);
        ib.set_viewbox_units(BrushMappingMode::Absolute);

        assert_eq!(ib.alignment_x(), Some(AlignmentX::Left), "ib alignment_x");
        assert_eq!(ib.alignment_y(), Some(AlignmentY::Top), "ib alignment_y");
        assert_eq!(ib.stretch(), Some(Stretch::None), "ib stretch");
        assert_eq!(ib.tile_mode(), Some(TileMode::Tile), "ib tile_mode");
        assert!(approx4(ib.viewport(), [1.0, 2.0, 3.0, 4.0]), "ib viewport");
        assert_eq!(
            ib.viewport_units(),
            Some(BrushMappingMode::Absolute),
            "ib viewport_units"
        );
        assert!(approx4(ib.viewbox(), [5.0, 6.0, 7.0, 8.0]), "ib viewbox");
        assert_eq!(
            ib.viewbox_units(),
            Some(BrushMappingMode::Absolute),
            "ib viewbox_units"
        );

        // Mutate an enum a second time to prove setters aren't write-once.
        ib.set_stretch(Stretch::Uniform);
        assert_eq!(ib.stretch(), Some(Stretch::Uniform), "ib stretch re-set");

        // ── CompositeTransform3D: 12-float round-trip ───────────────────────
        let fields = Composite3DFields {
            center_x: 200.0,
            center_y: 100.0,
            center_z: 5.0,
            rotation_x: 10.0,
            rotation_y: -40.0,
            rotation_z: 25.0,
            scale_x: 2.0,
            scale_y: 3.0,
            scale_z: 0.5,
            translate_x: 11.0,
            translate_y: 12.0,
            translate_z: 13.0,
        };
        let mut ct3d = CompositeTransform3D::new(fields);
        assert_eq!(ct3d.get(), fields, "composite3d round-trip");

        // Mutate via set() and re-read.
        let fields2 = Composite3DFields {
            rotation_y: 90.0,
            translate_z: -7.0,
            ..fields
        };
        ct3d.set(fields2);
        assert_eq!(ct3d.get(), fields2, "composite3d set round-trip");

        // ── MatrixTransform3D: 12-float (Transform3) round-trip ─────────────
        // An affine 3D matrix: identity rotation rows + a translation row.
        let m = [
            1.0, 0.0, 0.0, // row 0
            0.0, 1.0, 0.0, // row 1
            0.0, 0.0, 1.0, // row 2
            7.0, 8.0, 9.0, // row 3 (translation)
        ];
        let mut mt3d = MatrixTransform3D::new(m);
        let got = mt3d.get();
        assert!(
            got.iter().zip(m.iter()).all(|(a, b)| approx(*a, *b)),
            "matrix3d round-trip"
        );
        let m2 = [
            2.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 2.0, -1.0, -2.0, -3.0,
        ];
        mt3d.set(m2);
        let got2 = mt3d.get();
        assert!(
            got2.iter().zip(m2.iter()).all(|(a, b)| approx(*a, *b)),
            "matrix3d set round-trip"
        );

        // ── Element Transform3D assignment (UIElement::SetTransform3D) ───────
        let el_xaml = format!("<Border {NS} Width=\"100\" Height=\"100\"/>");
        let mut el = FrameworkElement::parse(&el_xaml).expect("parse el");
        // No 3D transform set initially.
        assert!(el.transform3d().is_none(), "no Transform3D initially");

        assert!(el.set_transform3d(&ct3d), "set Transform3D (composite)");
        let read = el.transform3d().expect("Transform3D set");
        assert_eq!(
            read.raw(),
            ct3d.raw(),
            "GetTransform3D returns the exact composite we assigned"
        );

        // Re-apply the type-erased handle to another element.
        let mut el2 = FrameworkElement::parse(&el_xaml).expect("parse el2");
        assert!(el2.set_transform3d(&read), "re-apply AnyTransform3D");
        assert_eq!(
            el2.transform3d().expect("el2 Transform3D").raw(),
            ct3d.raw(),
            "re-applied transform identity"
        );

        // Replace with the matrix transform, then clear.
        assert!(el.set_transform3d(&mt3d), "set Transform3D (matrix)");
        assert_eq!(
            el.transform3d().expect("matrix Transform3D set").raw(),
            mt3d.raw(),
            "Transform3D replaced by the matrix transform"
        );
        assert!(el.clear_transform3d(), "clear Transform3D");
        assert!(el.transform3d().is_none(), "Transform3D cleared");

        // Keep vb2 alive until here so its source ref is exercised.
        let _ = &vb2;
    }

    dm_noesis_runtime::shutdown();
}

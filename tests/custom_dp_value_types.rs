//! TODO §9 — custom dependency-property value types beyond the original
//! scalar/struct/string set: `Point`, `Size`, `Vector` (Noesis::Vector2), and a
//! runtime-`enum`-typed DP.
//!
//! Every assertion reads the value back THROUGH the live Noesis object (either
//! the `Instance` handle on a code-created instance, or the name-keyed
//! `FrameworkElement` accessors on a parsed instance), so a stub that did not
//! actually marshal the struct/enum across the FFI would fail. We also verify
//! registered defaults apply, and that the dynamic tag inference
//! (`FrameworkElement::property_tag` / `get_dynamic`) classifies the new types.

use dm_noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyOptions, PropertyValue,
};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::reflection::register_enum;
use dm_noesis_runtime::view::{DynValue, FrameworkElement};

struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&mut self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

const THING_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:dm="clr-namespace:DmVT">
  <dm:Thing x:Name="T"/>
</Grid>"##;

#[test]
fn custom_dp_value_types() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();
    {
        // A runtime enum used as a DP value type.
        let mode = register_enum("DmVT.Mode", &[("Off", 0), ("On", 1), ("Auto", 2)])
            .expect("register_enum failed");
        assert_eq!(mode.value_from_name("Auto"), Some(2));

        let mut b = ClassBuilder::new("DmVT.Thing", ClassBase::FrameworkElement, Noop);
        let pt = b.add_property("Pt", PropType::Point);
        let sz = b.add_property("Sz", PropType::Size);
        let vec = b.add_property("Vec", PropType::Vector);
        // Enum DP with a non-zero default so the default round-trip is meaningful.
        let m = b.add_enum_property("Mode", "DmVT.Mode", 2, PropertyOptions::default());
        assert_eq!((pt, sz, vec, m), (0, 1, 2, 3));

        let reg = b.register().expect("class registration failed");
        assert_eq!(reg.num_properties(), 4);

        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        // Defaults: Point/Size/Vector default to zero; enum to its registered 2.
        assert_eq!(h.get_point(pt), Some((0.0, 0.0)));
        assert_eq!(h.get_enum(m), Some(2), "enum DP default did not apply");

        // Round-trip each new value type through the live object.
        h.set_point(pt, 3.5, -7.25);
        assert_eq!(h.get_point(pt), Some((3.5, -7.25)), "Point DP round-trip");

        h.set_size(sz, 64.0, 48.0);
        assert_eq!(h.get_size(sz), Some((64.0, 48.0)), "Size DP round-trip");

        h.set_vector(vec, -1.5, 2.0);
        assert_eq!(h.get_vector(vec), Some((-1.5, 2.0)), "Vector DP round-trip");

        h.set_enum(m, mode.value_from_name("On").unwrap());
        assert_eq!(h.get_enum(m), Some(1), "enum DP round-trip");

        drop(inst);
        drop(reg);

        // ── Name-keyed FrameworkElement access on a parsed instance ──────────
        register_enum("DmVT2.Mode", &[("A", 0), ("B", 5)]).expect("register_enum");

        let mut b = ClassBuilder::new("DmVT.Thing2", ClassBase::FrameworkElement, Noop);
        let _pt = b.add_property("Pt", PropType::Point);
        let _sz = b.add_property("Sz", PropType::Size);
        let _vec = b.add_property("Vec", PropType::Vector);
        let _m = b.add_enum_property("Mode", "DmVT2.Mode", 0, PropertyOptions::default());
        let _reg = b.register().expect("class registration failed");

        // Parse + instantiate via the factory; drive the name-keyed
        // FrameworkElement accessors (dm_noesis_dependency_object_*).
        let root = {
            let xaml = THING_XAML.replace("dm:Thing", "dm:Thing2");
            FrameworkElement::parse(&xaml).expect("parse returned None")
        };
        let mut el = root.find_name("T").expect("find_name(T) returned None");

        // Dynamic tag inference classifies the new value types.
        assert_eq!(el.property_tag("Pt"), Some(PropType::Point));
        assert_eq!(el.property_tag("Sz"), Some(PropType::Size));
        assert_eq!(el.property_tag("Vec"), Some(PropType::Vector));
        assert_eq!(el.property_tag("Mode"), Some(PropType::Enum));

        assert!(el.set_point("Pt", [1.0, 2.0]));
        assert_eq!(el.get_point("Pt"), Some([1.0, 2.0]));
        assert!(matches!(
            el.get_dynamic("Pt"),
            Some(DynValue::Point([1.0, 2.0]))
        ));

        assert!(el.set_size("Sz", [10.0, 20.0]));
        assert_eq!(el.get_size("Sz"), Some([10.0, 20.0]));

        assert!(el.set_vector("Vec", [-3.0, 4.0]));
        assert_eq!(el.get_vector("Vec"), Some([-3.0, 4.0]));

        assert!(el.set_enum("Mode", 5));
        assert_eq!(el.get_enum("Mode"), Some(5));
        assert!(matches!(el.get_dynamic("Mode"), Some(DynValue::Enum(5))));

        // A type-mismatched access is rejected (Point DP read as a Rect).
        assert_eq!(el.get_rect("Pt"), None, "tag mismatch must be rejected");

        // ── SetCurrentValue / GetBaseValue for the new value types ───────────
        // Mirrors the scalar pattern: the local value stays the *base* while the
        // effective getter returns the SetCurrentValue override (TODO §9 —
        // exercises apply_set(Current)/apply_get(Base) for Point/Size/Vector/Enum).
        // `el` already has Pt=[1,2], Sz=[10,20], Vec=[-3,4], Mode=5 set locally.
        assert!(el.set_current_point("Pt", [9.0, 8.0]));
        assert_eq!(el.get_point("Pt"), Some([9.0, 8.0]), "Point current value");
        assert_eq!(
            el.get_base_point("Pt"),
            Some([1.0, 2.0]),
            "Point base value unaffected by SetCurrentValue"
        );

        assert!(el.set_current_size("Sz", [33.0, 44.0]));
        assert_eq!(el.get_size("Sz"), Some([33.0, 44.0]), "Size current value");
        assert_eq!(
            el.get_base_size("Sz"),
            Some([10.0, 20.0]),
            "Size base value unaffected by SetCurrentValue"
        );

        assert!(el.set_current_vector("Vec", [7.0, -7.0]));
        assert_eq!(
            el.get_vector("Vec"),
            Some([7.0, -7.0]),
            "Vector current value"
        );
        assert_eq!(
            el.get_base_vector("Vec"),
            Some([-3.0, 4.0]),
            "Vector base value unaffected by SetCurrentValue"
        );

        assert!(el.set_current_enum("Mode", 0));
        assert_eq!(el.get_enum("Mode"), Some(0), "Enum current value");
        assert_eq!(
            el.get_base_enum("Mode"),
            Some(5),
            "Enum base value unaffected by SetCurrentValue"
        );
    }
    dm_noesis_runtime::shutdown();
}

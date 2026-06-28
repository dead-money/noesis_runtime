//! Read-only dependency properties for `Point` and enum types: ordinary setter
//! is a no-op; privileged key-path setter writes and reads back through Noesis.

use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyDefault, PropertyOptions, PropertyValue,
};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::reflection::register_enum;

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn custom_dp_readonly_value_types() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();
    {
        // The enum type must exist when the class is registered.
        register_enum("Meta.RoMode", &[("A", 0), ("B", 7)]).expect("register_enum failed");

        let mut b = ClassBuilder::new("Meta.RoTypes", ClassBase::FrameworkElement, NoopChange);
        let pt = b.add_property_ex(
            "RoPoint",
            PropType::Point,
            PropertyDefault::Point { x: 1.0, y: 2.0 },
            PropertyOptions {
                read_only: true,
                ..Default::default()
            },
        );
        let mode = b.add_enum_property(
            "RoMode",
            "Meta.RoMode",
            0,
            PropertyOptions {
                read_only: true,
                ..Default::default()
            },
        );
        assert_eq!((pt, mode), (0, 1));

        let reg = b.register().expect("registration failed");
        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        h.set_point(pt, 9.0, 9.0); // ordinary path: ignored
        assert_eq!(
            h.get_point(pt),
            Some((1.0, 2.0)),
            "ordinary setter must not write a read-only Point DP"
        );
        assert!(
            h.set_readonly_point(pt, 5.0, 6.0),
            "privileged Point setter returned false"
        );
        assert_eq!(
            h.get_point(pt),
            Some((5.0, 6.0)),
            "read-only Point DP did not update via key path"
        );

        h.set_enum(mode, 7); // ordinary path: ignored
        assert_eq!(
            h.get_enum(mode),
            Some(0),
            "ordinary setter must not write a read-only enum DP"
        );
        assert!(
            h.set_readonly_enum(mode, 7),
            "privileged enum setter returned false"
        );
        assert_eq!(
            h.get_enum(mode),
            Some(7),
            "read-only enum DP did not update via key path"
        );

        drop(inst);
        drop(reg);
    }
    noesis_runtime::shutdown();
}

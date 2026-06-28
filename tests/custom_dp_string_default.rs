//! Regression test for the `PropertyDefault::String` silent-drop bug: a custom
//! dependency property registered with a string default must read that default
//! back THROUGH Noesis on a freshly-created instance (previously the borrow
//! path computed the pointer then discarded it, so the C++ "" default applied).

use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyDefault, PropertyValue,
};
use noesis_runtime::ffi::{ClassBase, PropType};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn custom_dp_string_default() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut b = ClassBuilder::new(
            "StringDefault.Thing",
            ClassBase::FrameworkElement,
            NoopChange,
        );
        let titled =
            b.add_property_with("Title", PropType::String, PropertyDefault::String("hello"));
        let empty = b.add_property_with("Empty", PropType::String, PropertyDefault::None);
        assert_eq!((titled, empty), (0, 1));

        let reg = b.register().expect("registration failed");
        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        // The string default must survive registration and read back unchanged.
        assert_eq!(
            h.get_string(titled).as_deref(),
            Some("hello"),
            "string DP default was silently dropped"
        );
        // A type-default (no default) reads back as the empty string.
        assert_eq!(
            h.get_string(empty).as_deref(),
            Some(""),
            "type-default string should be empty"
        );

        h.set_string(titled, "world");
        assert_eq!(h.get_string(titled).as_deref(), Some("world"));

        drop(inst);
        drop(reg);
    }

    noesis_runtime::shutdown();
}

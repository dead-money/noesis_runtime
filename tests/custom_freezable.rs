//! Custom `Freezable` subclass: DP round-trip and the freeze state.
//!
//! The property-changed callback is not asserted because it does not fire on
//! code-created (unparsed, tree-detached) instances.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};

struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn custom_freezable() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();
    {
        let mut b = ClassBuilder::new("NzFz.Recipe", ClassBase::Freezable, Noop);
        let amount = b.add_property("Amount", PropType::Int32);
        let reg = b.register().expect("Freezable class registration failed");
        assert_eq!(reg.num_properties(), 1);

        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        h.set_int32(amount, 42);
        assert_eq!(h.get_int32(amount), Some(42), "Freezable DP round-trip");
        h.set_int32(amount, -9);
        assert_eq!(h.get_int32(amount), Some(-9), "Freezable DP second write");

        assert!(!inst.is_frozen(), "instance should start unfrozen");
        assert!(inst.can_freeze(), "instance should be freezable");
        assert!(inst.freeze(), "freeze() failed");
        assert!(
            inst.is_frozen(),
            "instance did not report frozen after freeze()"
        );

        drop(inst);
        drop(reg);
    }
    noesis_runtime::shutdown();
}

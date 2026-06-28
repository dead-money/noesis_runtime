//! TODO §9 — custom `Freezable` base class.
//!
//! Proves the synthetic-`TypeClass` reflection machinery extends to the
//! non-`UIElement` `DependencyObject` side of the hierarchy: a Rust-backed
//! `Noesis::Freezable` subclass with custom DPs. We assert (all read back
//! THROUGH the live Noesis object):
//!   * registration of a `ClassBase::Freezable` succeeds and is creatable,
//!   * a DP round-trips through Noesis DP storage on a code-created instance,
//!   * the freeze state machine works: `can_freeze` -> `freeze` -> `is_frozen`.
//!
//! Note: the property-changed callback is NOT asserted here — it does not fire
//! on a code-created (un-parsed, tree-detached) `Freezable`, the same SDK
//! constraint documented for the element bases (see TODO.md "Known SDK
//! limitations"). The handler below is a `Noop` for that reason.
//!
//! The sibling `Animatable` subtrees (`Brush`/`Geometry`/`Transform`/`Effect`)
//! are NOT subclassable this way — see TODO.md "Known SDK limitations".

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};

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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();
    {
        let mut b = ClassBuilder::new("DmFz.Recipe", ClassBase::Freezable, Noop);
        let amount = b.add_property("Amount", PropType::Int32);
        let reg = b.register().expect("Freezable class registration failed");
        assert_eq!(reg.num_properties(), 1);

        let inst = reg.create_instance().expect("create_instance");
        let h = inst.handle();

        // DP round-trip through the live Freezable's DP storage: a stubbed
        // trampoline (no real synthetic TypeClass / DependencyData) could not
        // store and read this back.
        h.set_int32(amount, 42);
        assert_eq!(h.get_int32(amount), Some(42), "Freezable DP round-trip");
        h.set_int32(amount, -9);
        assert_eq!(h.get_int32(amount), Some(-9), "Freezable DP second write");

        // Freeze state machine, read back through Noesis::Freezable.
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
    dm_noesis_runtime::shutdown();
}

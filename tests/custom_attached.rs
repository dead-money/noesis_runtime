//! Phase C — attached-property REGISTRATION on a Rust-backed owner type
//! (TODO §9 "Custom dependency properties: attached").
//!
//! Registers a Rust-backed owner class carrying an Int32 DP, then sets/gets it
//! as an attached property (`owner:Prop`) on an unrelated built-in element and
//! reads the value back THROUGH Noesis. Also checks the negative case (an
//! unknown owner type is rejected).

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::FrameworkElement;

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&mut self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

const XAML: &str = r##"<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" x:Name="B"/>"##;

#[test]
fn custom_attached_property() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // Register the owner type + its attached DP BEFORE resolving it.
        let mut b = ClassBuilder::new("Att.Owner", ClassBase::FrameworkElement, NoopChange);
        let slot = b.add_property("Slot", PropType::Int32);
        assert_eq!(slot, 0);
        let reg = b.register().expect("owner registration failed");

        // An unrelated built-in element carries the attached DP.
        let mut border = FrameworkElement::parse(XAML).expect("parse Border");

        // Set the attached DP via the Rust owner type name, read back via Noesis.
        assert!(
            border.set_attached_i32("Att.Owner", "Slot", 9),
            "set attached on Rust-registered owner failed"
        );
        assert_eq!(
            border.get_attached_i32("Att.Owner", "Slot"),
            Some(9),
            "attached DP did not round-trip"
        );

        // Negative: an unknown owner type must be rejected.
        assert!(
            !border.set_attached_i32("Att.DoesNotExist", "Slot", 1),
            "unknown owner type must be rejected"
        );

        drop(border);
        drop(reg);
    }

    dm_noesis_runtime::shutdown();
}

//! Attached-property registration on a Rust-backed owner type: sets and gets an
//! Int32 DP as an attached property on a built-in element, and verifies that an
//! unknown owner type is rejected.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::FrameworkElement;

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

const XAML: &str = r##"<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" x:Name="B"/>"##;

#[test]
fn custom_attached_property() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut b = ClassBuilder::new("Att.Owner", ClassBase::FrameworkElement, NoopChange);
        let slot = b.add_property("Slot", PropType::Int32);
        assert_eq!(slot, 0);
        let reg = b.register().expect("owner registration failed");

        // An unrelated built-in element carries the attached DP.
        let mut border = FrameworkElement::parse(XAML).expect("parse Border");

        assert!(
            border.set_attached_i32("Att.Owner", "Slot", 9),
            "set attached on Rust-registered owner failed"
        );
        assert_eq!(
            border.get_attached_i32("Att.Owner", "Slot"),
            Some(9),
            "attached DP did not round-trip"
        );

        assert!(
            !border.set_attached_i32("Att.DoesNotExist", "Slot", 1),
            "unknown owner type must be rejected"
        );

        drop(border);
        drop(reg);
    }

    noesis_runtime::shutdown();
}

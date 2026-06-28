//! `DependsOn` and `ContentProperty` metadata round-trip through Noesis reflection; both can
//! coexist on a single type.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::reflection::{
    add_depends_on, get_content_property, get_depends_on, set_content_property,
};

// NOTE: convention is one `#[test]` per integration binary (Noesis `init` once
// per process), so the ContentProperty/DependsOn coexistence proof lives in the
// single test below alongside the original DependsOn round-trip.

struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

#[test]
fn depends_on_metadata() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();
    {
        let mut b = ClassBuilder::new("NzDep.Widget", ClassBase::FrameworkElement, Noop);
        b.add_property("First", PropType::Int32);
        b.add_property("Second", PropType::Int32);
        let _reg = b.register().expect("class registration failed");

        assert_eq!(get_depends_on("NzDep.Widget"), None);

        assert!(
            add_depends_on("NzDep.Widget", "First"),
            "add_depends_on returned false"
        );
        assert_eq!(
            get_depends_on("NzDep.Widget").as_deref(),
            Some("First"),
            "DependsOn metadata did not round-trip through reflection"
        );

        assert!(!add_depends_on("NzDep.NoSuchType", "First"));
        assert_eq!(get_depends_on("NzDep.NoSuchType"), None);

        assert_eq!(get_depends_on("Button"), None);

        // FindMeta is keyed by metadata TypeClass so a type carries both independently.
        let mut b2 = ClassBuilder::new("NzDep.Both", ClassBase::FrameworkElement, Noop);
        b2.add_property("Content", PropType::Int32);
        b2.add_property("Trigger", PropType::Int32);
        let _reg2 = b2.register().expect("class registration failed");

        assert_eq!(get_content_property("NzDep.Both"), None);
        assert_eq!(get_depends_on("NzDep.Both"), None);

        assert!(set_content_property("NzDep.Both", "Content"));
        assert!(add_depends_on("NzDep.Both", "Trigger"));

        assert_eq!(
            get_content_property("NzDep.Both").as_deref(),
            Some("Content"),
            "ContentProperty did not round-trip alongside DependsOn"
        );
        assert_eq!(
            get_depends_on("NzDep.Both").as_deref(),
            Some("Trigger"),
            "DependsOn did not round-trip alongside ContentProperty"
        );
    }
    noesis_runtime::shutdown();
}

//! TODO §9 — `DependsOn` metadata attribution, the analogue of the
//! `ContentProperty` path (`Noesis::DependsOnMetaData`, NsGui/DependsOnMetaData.h,
//! attached at the type level like `ContentPropertyMetaData`).
//!
//! The round-trip proof reads the recorded property name back THROUGH the live
//! reflection metadata (`TypeMeta::FindMeta<DependsOnMetaData>` +
//! `GetDependsOnProperty`): a stub that did not actually attach the metadata
//! would return `None` from `get_depends_on`.

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::reflection::{
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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();
    {
        let mut b = ClassBuilder::new("DmDep.Widget", ClassBase::FrameworkElement, Noop);
        b.add_property("First", PropType::Int32);
        b.add_property("Second", PropType::Int32);
        let _reg = b.register().expect("class registration failed");

        // No metadata yet.
        assert_eq!(get_depends_on("DmDep.Widget"), None);

        // Attach DependsOn(First) and read it straight back through reflection.
        assert!(
            add_depends_on("DmDep.Widget", "First"),
            "add_depends_on returned false"
        );
        assert_eq!(
            get_depends_on("DmDep.Widget").as_deref(),
            Some("First"),
            "DependsOn metadata did not round-trip through reflection"
        );

        // Unknown type fails for both attach and query.
        assert!(!add_depends_on("DmDep.NoSuchType", "First"));
        assert_eq!(get_depends_on("DmDep.NoSuchType"), None);

        // A type with no DependsOn metadata returns None (built-in Button).
        assert_eq!(get_depends_on("Button"), None);

        // ── ContentProperty + DependsOn coexist on one type ──────────────────
        // FindMeta is keyed by the metadata TypeClass and AddMeta only appends,
        // so a type carries BOTH and each reads back independently —
        // contradicting the stale "can't have both" note in
        // ContentPropertyMetaData.h (which names a non-existent
        // DependsOnAttributeMetaData type).
        let mut b2 = ClassBuilder::new("DmDep.Both", ClassBase::FrameworkElement, Noop);
        b2.add_property("Content", PropType::Int32);
        b2.add_property("Trigger", PropType::Int32);
        let _reg2 = b2.register().expect("class registration failed");

        assert_eq!(get_content_property("DmDep.Both"), None);
        assert_eq!(get_depends_on("DmDep.Both"), None);

        assert!(set_content_property("DmDep.Both", "Content"));
        assert!(add_depends_on("DmDep.Both", "Trigger"));

        assert_eq!(
            get_content_property("DmDep.Both").as_deref(),
            Some("Content"),
            "ContentProperty did not round-trip alongside DependsOn"
        );
        assert_eq!(
            get_depends_on("DmDep.Both").as_deref(),
            Some("Trigger"),
            "DependsOn did not round-trip alongside ContentProperty"
        );
    }
    dm_noesis_runtime::shutdown();
}

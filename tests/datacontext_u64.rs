//! `u64` row identity on bound view models + the `DataContext` accessor that
//! recovers it for per-row event routing. One Noesis lifecycle covers three
//! checks:
//!
//!   1. A `ClassInstance` (the `ClassBuilder` row object) carries a real `uint64`
//!      DP: `set_u64` / `get_u64` round-trip a full 64-bit value.
//!   2. `noesis_element_datacontext_get_u64` (via `FrameworkElement::
//!      data_context_u64`) reads that DP off the element's bound `DataContext`.
//!   3. The same accessor reads a `uint64` off a plain-VM (`PlainInstance`)
//!      `DataContext`, whose field is a boxed `BoxedValue<uint64_t>`.
//!
//! `0xDEAD_BEEF_0000_0001` is chosen so a truncation to 32 bits would be caught.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use noesis_runtime::view::FrameworkElement;

const ROW_ID: u64 = 0xDEAD_BEEF_0000_0001;

const BORDER_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        x:Name="Root"/>"##;

// A no-op change handler: this test drives DPs directly, it doesn't observe
// callbacks.
struct Noop;
impl PropertyChangeHandler for Noop {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn datacontext_u64() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    // ── 1 + 2: ClassInstance with a real uint64 DP ──────────────────────────
    {
        let mut builder = ClassBuilder::new("DmTest.U64Row", ClassBase::ContentControl, Noop);
        let row_id = builder.add_property("RowId", PropType::UInt64);
        let reg = builder.register().expect("register U64Row");

        let inst = reg.create_instance().expect("create_instance");
        let handle = inst.handle();

        // Default uint64 DP reads back 0.
        assert_eq!(handle.get_u64(row_id), Some(0));

        // Round-trip a full 64-bit value through the DP.
        handle.set_u64(row_id, ROW_ID);
        assert_eq!(
            handle.get_u64(row_id),
            Some(ROW_ID),
            "set_u64 / get_u64 round-trips the full 64-bit value"
        );

        // Bind it as an element's DataContext and recover the id via the
        // borrowed-DataContext accessor (the per-row event-routing path).
        let mut element = FrameworkElement::parse(BORDER_XAML).expect("parse Border");
        assert!(element.set_data_context(&inst), "set_data_context");

        assert_eq!(
            element.data_context_u64("RowId"),
            Some(ROW_ID),
            "data_context_u64 reads the uint64 DP off the bound row object"
        );
        // A field that doesn't exist (or isn't uint64) yields None.
        assert_eq!(element.data_context_u64("Missing"), None);

        // Release the element's DataContext ref before the instance / reg drop.
        assert!(element.clear_data_context());
        drop(element);
        drop(inst);
        drop(reg);
    }

    // ── 3: plain-VM DataContext (boxed uint64) ──────────────────────────────
    {
        let mut builder = PlainVmBuilder::new("DmTest.U64PlainRow");
        let row_id = builder.add_property("RowId", PlainType::U64);
        let class = builder.register().expect("register U64PlainRow");

        let vm = class.create_instance().expect("create_instance");
        assert!(vm.set(row_id, PlainValue::U64(ROW_ID)));
        assert_eq!(
            vm.get_u64(row_id),
            Some(ROW_ID),
            "plain-VM set / get_u64 round-trips the boxed value"
        );

        let mut element = FrameworkElement::parse(BORDER_XAML).expect("parse Border");
        assert!(vm.set_data_context(&mut element), "set_data_context");

        assert_eq!(
            element.data_context_u64("RowId"),
            Some(ROW_ID),
            "data_context_u64 unboxes the uint64 off a plain-VM DataContext"
        );

        assert!(element.clear_data_context());
        drop(element);
        drop(vm);
        drop(class);
    }

    noesis_runtime::shutdown();
}

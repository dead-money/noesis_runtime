//! `u64` row ids on bound view models, recovered through
//! `FrameworkElement::data_context_u64`:
//!
//! 1. A `ClassInstance` `uint64` DP round-trips a full 64-bit value through
//!    `set_u64` / `get_u64`.
//! 2. `data_context_u64` reads that DP off the element's bound `DataContext`.
//! 3. It also reads a boxed `uint64` field off a `PlainInstance` `DataContext`.
//!
//! `ROW_ID` has high bits set so a truncation to 32 bits fails.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use noesis_runtime::view::FrameworkElement;

const ROW_ID: u64 = 0xDEAD_BEEF_0000_0001;

const BORDER_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Border xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
        xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
        x:Name="Root"/>"##;

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

    // Cases 1 and 2: ClassInstance.
    {
        let mut builder = ClassBuilder::new("DmTest.U64Row", ClassBase::ContentControl, Noop);
        let row_id = builder.add_property("RowId", PropType::UInt64);
        let reg = builder.register().expect("register U64Row");

        let inst = reg.create_instance().expect("create_instance");
        let handle = inst.handle();

        assert_eq!(handle.get_u64(row_id), Some(0));

        handle.set_u64(row_id, ROW_ID);
        assert_eq!(
            handle.get_u64(row_id),
            Some(ROW_ID),
            "set_u64 / get_u64 round-trips the full 64-bit value"
        );

        let mut element = FrameworkElement::parse(BORDER_XAML).expect("parse Border");
        assert!(element.set_data_context(&inst), "set_data_context");

        assert_eq!(
            element.data_context_u64("RowId"),
            Some(ROW_ID),
            "data_context_u64 reads the uint64 DP off the bound row object"
        );
        assert_eq!(element.data_context_u64("Missing"), None);

        // Release the element's DataContext ref before the instance / reg drop.
        assert!(element.clear_data_context());
        drop(element);
        drop(inst);
        drop(reg);
    }

    // Case 3: plain-VM DataContext.
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

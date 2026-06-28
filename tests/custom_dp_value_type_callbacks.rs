//! TODO §9 — property-change callback DECODE for the new DP value types
//! (`Point`, `Size`, `Vector`, `Enum`).
//!
//! The change callback only fires on a parsed / tree-attached instance (not on a
//! code-created `create_instance` object — see TODO.md "Known SDK limitations"),
//! so this mirrors `tests/classes.rs`: register the class, load it inside a live
//! `View`, settle the tree, then mutate each new-type DP through the parsed
//! instance and assert the recording handler decoded
//! `PropertyValue::{Point|Size|Vector|Enum}` with the right payload. This is the
//! only test exercising the `decode_value` arms for these tags.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyOptions, PropertyValue,
};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::reflection::register_enum;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:dm="clr-namespace:DmCb"
      Width="100" Height="100">
  <dm:Widget x:Name="W"/>
</Grid>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Rec {
    Point([f32; 2]),
    Size([f32; 2]),
    Vector([f32; 2]),
    Enum(i32),
    Other,
}

#[derive(Clone, Default)]
struct Recorder {
    inner: Arc<Mutex<Vec<(u32, Rec)>>>,
}

struct Handler {
    recorder: Recorder,
}
impl PropertyChangeHandler for Handler {
    fn on_changed(&self, _instance: Instance, prop_index: u32, value: PropertyValue<'_>) {
        let v = match value {
            PropertyValue::Point { x, y } => Rec::Point([x, y]),
            PropertyValue::Size { width, height } => Rec::Size([width, height]),
            PropertyValue::Vector { x, y } => Rec::Vector([x, y]),
            PropertyValue::Enum(v) => Rec::Enum(v),
            _ => Rec::Other,
        };
        self.recorder.inner.lock().unwrap().push((prop_index, v));
    }
}

#[test]
fn custom_dp_value_type_callbacks() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let recorder = Recorder::default();
    {
        register_enum("DmCb.Mode", &[("Off", 0), ("On", 1), ("Auto", 2)])
            .expect("register_enum failed");

        let mut b = ClassBuilder::new(
            "DmCb.Widget",
            ClassBase::FrameworkElement,
            Handler {
                recorder: recorder.clone(),
            },
        );
        let pt = b.add_property("Pt", PropType::Point);
        let sz = b.add_property("Sz", PropType::Size);
        let vec = b.add_property("Vec", PropType::Vector);
        let mode = b.add_enum_property("Mode", "DmCb.Mode", 0, PropertyOptions::default());
        let reg = b.register().expect("class registration failed");

        let mut bytes = HashMap::new();
        bytes.insert("cb.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("cb.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(100, 100);
        view.activate();
        assert!(view.update(0.0));

        let content = view.content().expect("View::content returned None");
        let widget = content.find_name("W").expect("find_name(W) returned None");

        // The parsed instance is the raw FrameworkElement pointer.
        let inst = unsafe { Instance::from_raw(std::ptr::NonNull::new(widget.raw()).unwrap()) };

        // Mutate each new-type DP; the change callback fires on the
        // tree-attached instance and decodes the matching PropertyValue variant.
        inst.set_point(pt, 3.5, -7.25);
        inst.set_size(sz, 64.0, 48.0);
        inst.set_vector(vec, -1.5, 2.0);
        inst.set_enum(mode, 2);

        let recorded = recorder.inner.lock().unwrap().clone();
        assert!(
            recorded.contains(&(pt, Rec::Point([3.5, -7.25]))),
            "expected Point change [3.5,-7.25]; got {recorded:?}"
        );
        assert!(
            recorded.contains(&(sz, Rec::Size([64.0, 48.0]))),
            "expected Size change [64,48]; got {recorded:?}"
        );
        assert!(
            recorded.contains(&(vec, Rec::Vector([-1.5, 2.0]))),
            "expected Vector change [-1.5,2.0]; got {recorded:?}"
        );
        assert!(
            recorded.contains(&(mode, Rec::Enum(2))),
            "expected Enum change 2; got {recorded:?}"
        );

        drop(widget);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }
    noesis_runtime::shutdown();
}

//! Custom XAML class registration: property-changed callbacks, `Rect` round-trips,
//! and Noesis reflection resolving `<sample:NineSlicer>` to a Rust trampoline.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SLICER_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:sample="clr-namespace:Sample"
      Background="#FF202020" Width="200" Height="200">
  <sample:NineSlicer x:Name="MySlicer" SliceThickness="4,5,6,7"/>
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

#[derive(Clone, Default)]
struct Recorder {
    inner: Arc<Mutex<Vec<(u32, RecordedValue)>>>,
}
#[derive(Clone, Debug, PartialEq)]
enum RecordedValue {
    Thickness([f32; 4]),
    Rect([f32; 4]),
    Other,
}

struct Handler {
    recorder: Recorder,
}
impl PropertyChangeHandler for Handler {
    fn on_changed(&self, _instance: Instance, prop_index: u32, value: PropertyValue<'_>) {
        let v = match value {
            PropertyValue::Thickness {
                left,
                top,
                right,
                bottom,
            } => RecordedValue::Thickness([left, top, right, bottom]),
            PropertyValue::Rect {
                x,
                y,
                width,
                height,
            } => RecordedValue::Rect([x, y, width, height]),
            _ => RecordedValue::Other,
        };
        self.recorder.inner.lock().unwrap().push((prop_index, v));
    }
}

#[test]
fn class_registration_roundtrip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let recorder = Recorder::default();

    {
        // Register the class BEFORE the XAML provider so the parser can
        // resolve `<sample:NineSlicer>` on first load.
        let mut builder = ClassBuilder::new(
            "Sample.NineSlicer",
            ClassBase::ContentControl,
            Handler {
                recorder: recorder.clone(),
            },
        );
        let source_idx = builder.add_property("Source", PropType::ImageSource);
        let thickness_idx = builder.add_property("SliceThickness", PropType::Thickness);
        let viewbox_idx = builder.add_property("TopLeftViewbox", PropType::Rect);

        let registration = builder
            .register()
            .expect("class registration returned None");

        assert_eq!(source_idx, 0);
        assert_eq!(thickness_idx, 1);
        assert_eq!(viewbox_idx, 2);
        assert_eq!(registration.num_properties(), 3);

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SLICER_XAML.as_bytes().to_vec());
        let _provider_guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");

        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");
        let slicer = content
            .find_name("MySlicer")
            .expect("find_name returned None for MySlicer");
        assert_eq!(slicer.name().as_deref(), Some("MySlicer"));

        // First Update settles the visual tree; SliceThickness="4,5,6,7"
        // from XAML should already have fired the change callback.
        assert!(view.update(0.0));

        let recorded = recorder.inner.lock().unwrap().clone();
        let saw_thickness = recorded.iter().any(|(i, v)| {
            *i == thickness_idx
                && matches!(v, RecordedValue::Thickness(arr) if *arr == [4.0, 5.0, 6.0, 7.0])
        });
        assert!(
            saw_thickness,
            "expected SliceThickness change with [4,5,6,7]; got {:?}",
            recorded
        );

        let instance = unsafe { Instance::from_raw(std::ptr::NonNull::new(slicer.raw()).unwrap()) };
        instance.set_rect(viewbox_idx, 1.0, 2.0, 3.0, 4.0);
        let read = instance
            .get_rect(viewbox_idx)
            .expect("get_rect returned None");
        assert_eq!(read, (1.0, 2.0, 3.0, 4.0));

        let recorded2 = recorder.inner.lock().unwrap().clone();
        let saw_rect = recorded2.iter().any(|(i, v)| {
            *i == viewbox_idx
                && matches!(v, RecordedValue::Rect(arr) if *arr == [1.0, 2.0, 3.0, 4.0])
        });
        assert!(
            saw_rect,
            "expected Rect change from set_rect; got {:?}",
            recorded2
        );

        // Drop instance handles before the view, then drop the view, then
        // drop the registration. Registration must outlive every instance.
        drop(slicer);
        drop(content);
        view.deactivate();
        drop(view);
        drop(registration);
    }

    noesis_runtime::shutdown();
}

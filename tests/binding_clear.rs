//! `clear_binding` detaches a code-built binding: after the clear, source
//! mutations stop propagating to the target DP. Guards the removal-teardown
//! path (a consumer un-wiring a binding must actually stop the data flow, not
//! just drop its Rust handle).

use std::collections::HashMap;

use noesis_runtime::binding::{Binding, clear_binding, set_binding};
use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="80">
  <TextBlock x:Name="Label"/>
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

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn clear_binding_stops_source_propagation() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder =
            ClassBuilder::new("Sample.ClearVM", ClassBase::ContentControl, NoopHandler);
        let title_idx = builder.add_property("Title", PropType::String);
        let registration = builder.register().expect("VM class registration failed");

        let vm = registration
            .create_instance()
            .expect("create_instance returned None");
        vm.handle().set_string(title_idx, "Hello");

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        assert!(content.set_data_context(&vm), "set_data_context failed");

        let label = content.find_name("Label").expect("find_name(Label) failed");
        let binding = Binding::new("Title");
        assert!(set_binding(&label, "Text", &binding), "set_binding failed");
        assert!(view.update(0.0));
        assert_eq!(
            label.text().as_deref(),
            Some("Hello"),
            "control case: the binding never delivered the initial value"
        );

        // Clearing must detach the live expression, not just this Rust handle.
        assert!(clear_binding(&label, "Text"), "clear_binding failed");
        vm.handle().set_string(title_idx, "World");
        assert!(view.update(0.016));
        assert_ne!(
            label.text().as_deref(),
            Some("World"),
            "source mutation still propagated after clear_binding"
        );

        // Unbound clear reports success: callers need no bound-ness tracking.
        assert!(
            clear_binding(&label, "Text"),
            "clear_binding on an unbound DP reported failure"
        );

        drop(binding);
        drop(label);
        drop(content);
        view.deactivate();
        drop(view);
        drop(vm);
        drop(registration);
    }

    noesis_runtime::shutdown();
}

//! Rust-backed view model drives XAML bindings; asserts data flows both ways
//! across `View::update` (initial value delivery and post-load mutation).

use std::collections::HashMap;

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const BINDING_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="80">
  <TextBlock x:Name="Label" Text="{Binding Title}"/>
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

// The VM doesn't need to react to its own DP writes for this test, but a class
// registration requires a handler; this one just records nothing.
struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn rust_view_model_drives_binding() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder =
            ClassBuilder::new("Sample.BindingVM", ClassBase::ContentControl, NoopHandler);
        let title_idx = builder.add_property("Title", PropType::String);
        assert_eq!(title_idx, 0);
        let registration = builder.register().expect("VM class registration failed");

        let vm = registration
            .create_instance()
            .expect("create_instance returned None");
        // Seed the bound value before the binding is wired up.
        vm.handle().set_string(title_idx, "Hello");

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), BINDING_XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        // SAFETY: vm is alive for the rest of this scope; its raw() is a live
        // BaseComponent*. Noesis stores its own reference.
        assert!(
            content.set_data_context(&vm),
            "set_data_context returned false (content not a FrameworkElement?)"
        );

        assert!(view.update(0.0));

        let label = content.find_name("Label").expect("find_name(Label) failed");
        assert_eq!(
            label.text().as_deref(),
            Some("Hello"),
            "binding did not deliver the initial VM value to the TextBlock"
        );

        // Mutate the VM after the view is live; the binding must propagate the
        // change to the TextBlock on the next update. This is the part that
        // only works if the DependencyObject change notification reached the
        // binding — a static one-shot read would leave the text at "Hello".
        vm.handle().set_string(title_idx, "World");
        assert!(view.update(0.0));
        assert_eq!(
            label.text().as_deref(),
            Some("World"),
            "binding did not propagate the post-load VM mutation"
        );

        // Teardown: release element handles + view (which drops its DataContext
        // ref on the VM) before the VM and the registration.
        drop(label);
        drop(content);
        view.deactivate();
        drop(view);
        drop(vm);
        drop(registration);
    }

    noesis_runtime::shutdown();
}

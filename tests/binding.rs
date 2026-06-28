//! TODO §3 — data-binding bridge: a Rust-backed view model drives XAML.
//!
//! Registers a Rust class with a `Title` string DP, creates an instance from
//! Rust (no XAML reference), sets it as the root's `DataContext`, and binds a
//! `TextBlock.Text` to `{Binding Title}`. Then it asserts the binding engine
//! actually moved the data both ways across `View::update`:
//!
//!   * the value set on the VM *before* binding shows up in the bound `TextBlock`;
//!   * mutating the VM's DP *after* the view is live updates the `TextBlock` —
//!     which only happens if the `DependencyObject` change notification reached
//!     the binding (the INotifyPropertyChanged-equivalent for a DO source).
//!
//! Reading the rendered value back through `TextBlock::Text` (not through the
//! VM) is the point: it proves the data crossed the binding, not just that our
//! own setter round-trips.
//!
//!   `cargo test -p dm_noesis_runtime --test binding -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

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
    fn on_changed(&mut self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn rust_view_model_drives_binding() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // A Rust-backed view model: one string DP, `Title`.
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
        let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        // Wire the VM as the DataContext — the TextBlock's {Binding Title}
        // resolves against it.
        // SAFETY: vm is alive for the rest of this scope; its raw() is a live
        // BaseComponent*. Noesis stores its own reference.
        assert!(
            content.set_data_context(&vm),
            "set_data_context returned false (content not a FrameworkElement?)"
        );

        // First layout pass settles the binding.
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

    dm_noesis_runtime::shutdown();
}

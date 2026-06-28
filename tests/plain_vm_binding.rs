//! TODO §9 + §3 — a non-`DependencyObject` Rust view model drives a binding.
//!
//! This is the bevy-bridge unblocker: a plain `BaseComponent` view model (NOT a
//! `DependencyObject`) whose `Title` property resolves through *reflection* to a
//! Rust-pushed value, and which raises `INotifyPropertyChanged.PropertyChanged`
//! so a bound UI target refreshes.
//!
//! The assertions are deliberately fail-if-stubbed:
//!
//!   * The value pushed *before* binding shows up in the bound `TextBlock` —
//!     proves the reflected property is actually *read* through Noesis (a stub
//!     that never reads the store would leave the text empty).
//!   * After mutating the store *without* notifying, `view.update` leaves the
//!     `TextBlock` UNCHANGED — proves the refresh is driven by the notification,
//!     not an every-frame re-poll (so the next step is meaningful).
//!   * After `notify("Title")`, the `TextBlock` shows the new value — proves the
//!     `PropertyChanged` path reaches the binding. A stubbed notify leaves the
//!     old value and fails here.
//!
//! Everything is read BACK through `TextBlock::Text` (the live DP), never
//! through the Rust store, so it proves the data crossed the binding.

use std::collections::HashMap;

use dm_noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
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

#[test]
fn plain_view_model_drives_binding_with_inpc() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // A plain (non-DO) view model with a single reflected String property.
        let mut builder = PlainVmBuilder::new("Sample.PlainVM");
        let title = builder.add_property("Title", PlainType::String);
        assert_eq!(title, 0);
        let class = builder.register().expect("plain VM registration failed");

        let vm = class
            .create_instance()
            .expect("create_instance returned None");

        // Seed the reflected value BEFORE binding. Read it straight back out of
        // the reflection store to prove the set landed.
        assert!(vm.set(title, PlainValue::String("Hello".into())));
        assert_eq!(vm.get_string(title).as_deref(), Some("Hello"));

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        assert!(
            vm.set_data_context(&mut content),
            "set_data_context returned false"
        );

        // First pass settles the binding — proves the reflected property READ.
        view.update(0.0);
        let label = content.find_name("Label").expect("find_name(Label) failed");
        assert_eq!(
            label.text().as_deref(),
            Some("Hello"),
            "binding did not deliver the initial reflected value (reflection read broken)"
        );

        // Mutate the store WITHOUT notifying: the UI must NOT change on update.
        // This makes the next step meaningful (it isn't an every-frame re-poll).
        assert!(vm.set(title, PlainValue::String("World".into())));
        view.update(0.0);
        assert_eq!(
            label.text().as_deref(),
            Some("Hello"),
            "TextBlock changed without a PropertyChanged notification (test can't isolate notify)"
        );

        // Now raise PropertyChanged: the binding re-reads and the UI updates.
        assert!(vm.notify("Title"));
        view.update(0.0);
        assert_eq!(
            label.text().as_deref(),
            Some("World"),
            "PropertyChanged did not propagate the mutation (INotifyPropertyChanged path broken)"
        );

        drop(label);
        drop(content);
        view.deactivate();
        drop(view);
        drop(vm);
        drop(class);
    }

    dm_noesis_runtime::shutdown();
}

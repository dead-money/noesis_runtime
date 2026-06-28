//! A non-`DependencyObject` view model drives a binding and raises `PropertyChanged`.

use std::collections::HashMap;

use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

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
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = PlainVmBuilder::new("Sample.PlainVM");
        let title = builder.add_property("Title", PlainType::String);
        assert_eq!(title, 0);
        let class = builder.register().expect("plain VM registration failed");

        let vm = class
            .create_instance()
            .expect("create_instance returned None");

        assert!(vm.set(title, PlainValue::String("Hello".into())));
        assert_eq!(vm.get_string(title).as_deref(), Some("Hello"));

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        assert!(
            vm.set_data_context(&mut content),
            "set_data_context returned false"
        );

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

    noesis_runtime::shutdown();
}

//! `MultiBinding` with a Rust `IMultiValueConverter`: two-source combination,
//! initial value and source-change tracking.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use noesis_runtime::binding::Binding;
use noesis_runtime::converters::{ConvertArg, Converted};
use noesis_runtime::multi_binding::{MultiBinding, MultiConverter};
use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainVmBuilder};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="300" Height="80">
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

#[test]
fn multi_binding_combines_two_sources_through_rust_converter() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = PlainVmBuilder::new("Sample.NameVM");
        let first = builder.add_property("First", PlainType::String);
        let last = builder.add_property("Last", PlainType::String);
        let class = builder.register().expect("plain VM registration failed");

        let vm = class
            .create_instance()
            .expect("create_instance returned None");
        assert!(vm.set(first, PlainValue::String("Ada".into())));
        assert!(vm.set(last, PlainValue::String("Lovelace".into())));

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_cb = Arc::clone(&calls);
        let converter = MultiConverter::new(move |values: &[ConvertArg], _p: &ConvertArg| {
            calls_cb.fetch_add(1, Ordering::SeqCst);
            let a = values.first().and_then(ConvertArg::as_str)?;
            let b = values.get(1).and_then(ConvertArg::as_str)?;
            Some(Converted::String(format!("{a} {b}")))
        });

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(300, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        assert!(
            vm.set_data_context(&mut content),
            "set_data_context returned false"
        );

        let label = content.find_name("Label").expect("find_name(Label) failed");

        let mb = MultiBinding::new()
            .converter(&converter)
            .add_binding(Binding::new("First"))
            .add_binding(Binding::new("Last"));
        assert!(mb.set_on(&label, "Text"), "set_on(Text) returned false");

        view.update(0.0);
        assert_eq!(
            label.text().as_deref(),
            Some("Ada Lovelace"),
            "MultiBinding did not combine both sources through the Rust converter"
        );
        assert!(
            calls.load(Ordering::SeqCst) >= 1,
            "the multi-value converter never ran"
        );

        let before = calls.load(Ordering::SeqCst);
        assert!(vm.set_and_notify(first, "First", PlainValue::String("Grace".into())));
        view.update(0.0);
        assert_eq!(
            label.text().as_deref(),
            Some("Grace Lovelace"),
            "MultiBinding did not track the source change"
        );
        assert!(
            calls.load(Ordering::SeqCst) > before,
            "converter did not re-run after the source change"
        );

        drop(label);
        drop(content);
        view.deactivate();
        drop(view);
        drop(mb);
        drop(converter);
        drop(vm);
        drop(class);
    }

    noesis_runtime::shutdown();
}

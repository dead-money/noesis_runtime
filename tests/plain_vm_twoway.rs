//! `TwoWay` binding writeback from a `TextBox` into a plain-VM reflected property; verifies
//! the `on_set` hook fires and the reflected store is updated.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use noesis_runtime::binding::{Binding, BindingMode, UpdateSourceTrigger, set_binding};
use noesis_runtime::plain_vm::{PlainType, PlainValue, PlainValueRef, PlainVmBuilder};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="80">
  <TextBox x:Name="Box"/>
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

static HITS: AtomicUsize = AtomicUsize::new(0);
static LAST: Mutex<Option<(u32, String)>> = Mutex::new(None);

#[test]
fn plain_view_model_twoway_writeback() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = PlainVmBuilder::new("Sample.TwoWayVM");
        let title = builder.add_property("Title", PlainType::String);
        let class = builder
            .on_set(|idx: u32, value: &PlainValueRef| {
                HITS.fetch_add(1, Ordering::SeqCst);
                *LAST.lock().unwrap() = Some((idx, value.as_str().unwrap_or_default().to_owned()));
            })
            .register()
            .expect("plain VM registration failed");

        let vm = class
            .create_instance()
            .expect("create_instance returned None");
        assert!(vm.set(title, PlainValue::String("Seed".into())));

        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 80);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        assert!(vm.set_data_context(&mut content), "set_data_context failed");

        let mut textbox = content.find_name("Box").expect("find_name(Box) failed");

        let binding = Binding::new("Title")
            .mode(BindingMode::TwoWay)
            .update_source_trigger(UpdateSourceTrigger::PropertyChanged);
        assert!(
            set_binding(&textbox, "Text", &binding),
            "set_binding failed"
        );

        view.update(0.0);
        assert_eq!(textbox.text().as_deref(), Some("Seed"));
        assert_eq!(HITS.load(Ordering::SeqCst), 0, "no writeback expected yet");

        assert!(textbox.set_text("Edited"));
        view.update(0.0);

        assert_eq!(
            vm.get_string(title).as_deref(),
            Some("Edited"),
            "TwoWay writeback did not reach the reflected store (SetComponent broken)"
        );
        assert!(HITS.load(Ordering::SeqCst) >= 1, "on_set hook never fired");
        assert_eq!(
            *LAST.lock().unwrap(),
            Some((title, "Edited".to_owned())),
            "on_set saw the wrong index/value"
        );

        drop(textbox);
        drop(content);
        view.deactivate();
        drop(view);
        drop(vm);
        drop(class);
    }

    noesis_runtime::shutdown();
}

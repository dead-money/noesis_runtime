//! Refcount-driven teardown safety: `ClassRegistration` dropped before View must not
//! free the handler box while live instances still hold a `ClassData` ref.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

const CLASS_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:sample="clr-namespace:Sample"
      Background="#FF202020" Width="200" Height="200">
  <sample:Probe x:Name="ProbeInstance"/>
</Grid>"##;

/// Handler that bumps a shared counter on Drop. The counter sits behind
/// an `Arc` so the test still owns it after the registration is gone.
struct ClassDropProbe {
    drop_count: Arc<AtomicU32>,
}
impl PropertyChangeHandler for ClassDropProbe {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}
impl Drop for ClassDropProbe {
    fn drop(&mut self) {
        self.drop_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn class_handler_drops_when_last_instance_dies_not_at_unregister() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let drop_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), CLASS_XAML.as_bytes().to_vec());
        let _xaml = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let registration = {
            let handler = ClassDropProbe {
                drop_count: Arc::clone(&drop_count),
            };
            let mut b = ClassBuilder::new("Sample.Probe", ClassBase::ContentControl, handler);
            b.add_property("Source", PropType::ImageSource);
            b.register()
                .expect("ClassBuilder::register returned None for Sample.Probe")
        };

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));

        let content = view.content().expect("content");
        let probe = content
            .find_name("ProbeInstance")
            .expect("find_name ProbeInstance");
        drop(probe);

        // Drop registration before view. Previously segfaulted: ClassRegistration::drop
        // freed the handler box while the View still owned an instance whose destructor
        // fired a callback into that box.
        drop(registration);

        // Handler must still be alive: View holds an instance, keeping ClassData
        // refcount > 0, so the deferred free has not yet run.
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            0,
            "handler dropped at unregister; should have been deferred until last instance dies"
        );

        view.deactivate();
        drop(view);
    }

    assert_eq!(
        drop_count.load(Ordering::SeqCst),
        1,
        "handler must drop exactly once after the View tears down"
    );

    noesis_runtime::shutdown();
}

// Markup-extension counterpart lives in `tests/teardown_safety_markup.rs`
// because Noesis is process-singleton; each integration-test binary gets
// exactly one init/shutdown cycle.

//! Markup-extension counterpart of `teardown_safety.rs`.
//!
//! Same intrusive-refcount contract as the class case, applied to
//! `MarkupClassData`. The handler box must outlive the
//! `MarkupExtensionRegistration` if any live extension instance still
//! references it. See `tests/teardown_safety.rs` for the full rationale.
//!
//! Lives in its own integration-test binary so it gets a fresh Noesis
//! init/shutdown cycle (Noesis is process-singleton).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::markup::{MarkupExtensionHandler, MarkupExtensionRegistration, MarkupValue};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

const MARKUP_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:sample="clr-namespace:Sample"
      Background="#FF202020" Width="200" Height="200">
  <TextBlock Text="{sample:Echo hello}"/>
</Grid>"##;

struct MarkupDropProbe {
    drop_count: Arc<AtomicU32>,
    scratch: String,
}
impl MarkupExtensionHandler for MarkupDropProbe {
    fn provide_value(&mut self, key: &str) -> MarkupValue<'_> {
        self.scratch.clear();
        self.scratch.push_str(key);
        MarkupValue::String(&self.scratch)
    }
}
impl Drop for MarkupDropProbe {
    fn drop(&mut self) {
        self.drop_count.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn markup_handler_drops_exactly_once_after_view_teardown() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let drop_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), MARKUP_XAML.as_bytes().to_vec());
        let _xaml = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let registration = {
            let handler = MarkupDropProbe {
                drop_count: Arc::clone(&drop_count),
                scratch: String::new(),
            };
            MarkupExtensionRegistration::new("Sample.Echo", handler)
                .expect("MarkupExtensionRegistration::new returned None for Sample.Echo")
        };

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));

        // Drop the registration before the view is torn down — exercises
        // the deferred-free path through the C++ MarkupClassData refcount
        // when an extension instance is still live in the visual tree.
        drop(registration);

        view.deactivate();
        drop(view);
    }

    assert_eq!(
        drop_count.load(Ordering::SeqCst),
        1,
        "markup handler must drop exactly once across the teardown sequence"
    );

    dm_noesis_runtime::shutdown();
}

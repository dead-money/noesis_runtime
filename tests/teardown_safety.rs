//! Refcount-driven teardown safety for class + markup-extension
//! registrations.
//!
//! Pinpointed regression: when Bevy 0.18 drops the main-app `Resource`
//! holding a `ClassRegistration` BEFORE the render-app `NonSendResource`
//! holding the `View`, the View later releases instances of the
//! registered class — and those instance destructions can fire
//! property-change callbacks at the (formerly) Rust-owned handler box.
//! If the Rust side freed the handler at `ClassRegistration::drop`, this
//! is a use-after-free and segfaults during process teardown.
//!
//! The C++ side now holds an intrusive refcount on `ClassData` /
//! `MarkupClassData`. Each live instance bumps the count; the Rust
//! caller's registration holds the +1 created at register time.
//! `dm_noesis_class_unregister` releases that ref but defers the actual
//! free + handler-box drop to the moment the last instance dies.
//!
//! Each test exercises that contract:
//!   * register a class / markup with a handler that bumps a counter on
//!     its `Drop`,
//!   * load XAML that constructs an instance of the registered type,
//!   * drop the registration FIRST,
//!   * assert the handler is still alive (counter unchanged),
//!   * drop the View / FrameworkElement,
//!   * assert the handler dropped exactly once (counter == 1).
//!
//! Run with `NOESIS_SDK_DIR` set; no licence env vars required:
//!   `cargo test -p dm_noesis_runtime --test teardown_safety`

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
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

// ── Class refcount ──────────────────────────────────────────────────────────

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
    fn on_changed(&mut self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let drop_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), CLASS_XAML.as_bytes().to_vec());
        let _xaml = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        // Register the class. Box the handler with the shared counter; we
        // expect the box to live until the View drops.
        let registration = {
            let handler = ClassDropProbe {
                drop_count: Arc::clone(&drop_count),
            };
            let mut b = ClassBuilder::new("Sample.Probe", ClassBase::ContentControl, handler);
            b.add_property("Source", PropType::ImageSource);
            b.register()
                .expect("ClassBuilder::register returned None for Sample.Probe")
        };

        // Load the scene — this constructs a live `Sample.Probe` instance
        // inside the XAML's visual tree.
        let element = FrameworkElement::load("scene.xaml").expect("load_xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));

        // Sanity: the named instance exists in the scene.
        let content = view.content().expect("content");
        let probe = content
            .find_name("ProbeInstance")
            .expect("find_name ProbeInstance");
        drop(probe);

        // ── ACT: drop the registration BEFORE the view. This used to
        //         segfault — ClassRegistration::drop freed the handler
        //         box, then View teardown released the instance whose
        //         destructor fired a callback at the dangling box.
        drop(registration);

        // The handler MUST still be alive — the View still owns an
        // instance of the class, which holds the only remaining ref on
        // ClassData.
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            0,
            "handler dropped at unregister; should have been deferred until last instance dies"
        );

        // Now drop the view. This releases the instance, which releases
        // its ClassData ref, which (being the last one) finally frees
        // the handler box via the C++ → Rust free trampoline.
        view.deactivate();
        drop(view);
    }

    // Handler dropped exactly once.
    assert_eq!(
        drop_count.load(Ordering::SeqCst),
        1,
        "handler must drop exactly once after the View tears down"
    );

    dm_noesis_runtime::shutdown();
}

// Markup-extension counterpart lives in `tests/teardown_safety_markup.rs`
// because Noesis is process-singleton — each integration-test binary gets
// exactly one init/shutdown cycle.

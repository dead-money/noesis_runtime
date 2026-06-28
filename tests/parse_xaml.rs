//! TODO §15 — `GUI::ParseXaml` + `GUI::LoadComponent` integration test.
//!
//! Exercises [`FrameworkElement::parse`] (in-memory XAML, no `XamlProvider` URI)
//! and [`dm_noesis_runtime::gui::load_component`]. Noesis can't be re-init'd
//! per process, so both surfaces share one `#[test]` inside a single
//! init/shutdown, with every owning wrapper dropped before `shutdown`.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test parse_xaml -- --nocapture`

use std::collections::HashMap;
use std::ptr;

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::ClassBase;
use dm_noesis_runtime::gui::load_component;
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// A Grid hosting a single named Button — a real FrameworkElement tree.
const GRID_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="X" Content="Hi" Width="100" Height="40"
          HorizontalAlignment="Center" VerticalAlignment="Center"/>
</Grid>"##;

// A bare ResourceDictionary — a valid XAML object tree whose root is NOT a
// FrameworkElement, so `parse` must reject it.
const DICT_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <SolidColorBrush x:Key="Accent" Color="#FF00FF00"/>
</ResourceDictionary>"##;

// Not XAML at all → the parser must fail and `parse` must return None rather
// than crash.
const BROKEN_XAML: &str = "this is definitely not xaml @@@ <<< >>>";

// XAML served by URI for the LoadComponent smoke. A ContentControl root so it
// is at least shape-compatible with the registered ContentControl-based class.
const COMPONENT_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ContentControl xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"/>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

// A no-op property handler — LoadComponent only needs a registered type to
// instantiate; we don't assert on property changes here.
struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&mut self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn parse_xaml_and_load_component() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── ParseXaml: well-formed FrameworkElement tree ───────────────────
        let element = FrameworkElement::parse(GRID_XAML)
            .expect("parse() returned None for a well-formed Grid");
        let named = element
            .find_name("X")
            .expect("find_name(\"X\") returned None on a parsed tree");
        assert_eq!(
            named.name().as_deref(),
            Some("X"),
            "named element's x:Name did not round-trip"
        );
        drop(named);

        // The parsed element must be a usable view root: build a View and pump
        // a couple of updates to prove layout runs against it.
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(
            view.update(0.0),
            "first Update on parsed tree reported no change"
        );
        let _ = view.update(0.016);
        // The button must still be findable through the live view content.
        let content = view.content().expect("View::content returned None");
        assert!(
            content.find_name("X").is_some(),
            "named button not reachable through live view content"
        );
        drop(content);
        view.deactivate();
        drop(view);

        // ── ParseXaml: non-FrameworkElement root → None ────────────────────
        assert!(
            FrameworkElement::parse(DICT_XAML).is_none(),
            "parse() must reject a ResourceDictionary root (not a FrameworkElement)"
        );

        // ── ParseXaml: malformed XAML → None (no crash) ────────────────────
        assert!(
            FrameworkElement::parse(BROKEN_XAML).is_none(),
            "parse() must reject malformed XAML"
        );

        // ── LoadComponent ──────────────────────────────────────────────────
        // Null component → false, no FFI call into Noesis.
        // SAFETY: null is an explicitly-handled input for load_component.
        assert!(
            !unsafe { load_component(ptr::null_mut(), "component.xaml") },
            "load_component(null, ...) must be false"
        );

        // A real, type-registered instance + a resolvable URI: confirm the
        // call links and runs and returns true. Deeper assertions (that the
        // parsed tree is grafted onto the instance) aren't feasible here —
        // that requires the instance's reflected type to carry an x:Class
        // matching the XAML root, which this entry point doesn't wire up.
        let mut provider = HashMap::new();
        provider.insert(
            "component.xaml".to_string(),
            COMPONENT_XAML.as_bytes().to_vec(),
        );
        let _registered_provider =
            dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(provider));

        let registration =
            ClassBuilder::new("DM.LoadTarget", ClassBase::ContentControl, NoopHandler)
                .register()
                .expect("class registration failed");
        let instance = registration
            .create_instance()
            .expect("create_instance returned None");

        // SAFETY: instance.raw() is a live BaseComponent* for the lifetime of
        // `instance`; the URI resolves through the installed provider.
        let ran = unsafe { load_component(instance.raw(), "component.xaml") };
        assert!(
            ran,
            "load_component on a live instance + valid URI returned false"
        );

        // A missing URI still returns true (the call ran); Noesis logs the
        // unresolved-resource error internally. We assert the null-arg guard
        // above for the meaningful negative case.

        drop(instance);
        drop(registration);
    }

    dm_noesis_runtime::shutdown();
}

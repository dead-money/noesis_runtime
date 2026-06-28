//! `FrameworkElement::parse` (in-memory XAML) and `gui::load_component`:
//! parse well-formed / non-FrameworkElement / malformed XAML, and verify
//! `LoadComponent` grafts a named child onto a registered class instance.

use std::collections::HashMap;
use std::ffi::{CString, c_void};
use std::ptr;

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{
    ClassBase, noesis_base_component_release, noesis_framework_element_find_name,
    noesis_gui_load_component,
};
use noesis_runtime::gui::load_component;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

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

// XAML served by URI for the LoadComponent test. The `x:Class` names the
// Rust-registered class (`Nz.LoadTarget`, a ContentControl) so Noesis maps the
// root onto the supplied instance by type identity and grafts the parsed body
// — here a single named Button — onto it. We then assert that named child is
// reachable from the instance, which is the observable proof LoadComponent ran.
const COMPONENT_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ContentControl x:Class="Nz.LoadTarget"
                xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="GRAFTED" Content="grafted"/>
</ContentControl>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

/// Resolve an `x:Name` directly against a borrowed `BaseComponent*` (a live
/// class instance) without taking ownership of it. Returns whether the name
/// resolves to a `FrameworkElement`, releasing the `+1` ref the lookup hands
/// out. Used to observe `LoadComponent`'s grafting effect on the instance.
fn instance_has_named_child(raw: *mut c_void, name: &str) -> bool {
    let c = CString::new(name).expect("name contained NUL");
    // SAFETY: `raw` is a live BaseComponent* (a FrameworkElement subclass) for
    // the duration of the call; find_name returns NULL or a +1 ref we release.
    let found = unsafe { noesis_framework_element_find_name(raw, c.as_ptr()) };
    if found.is_null() {
        false
    } else {
        // SAFETY: `found` is a freshly-AddRef'd BaseComponent* we now own and
        // must release exactly once.
        unsafe { noesis_base_component_release(found) };
        true
    }
}

#[test]
fn parse_xaml_and_load_component() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
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
        let content = view.content().expect("View::content returned None");
        assert!(
            content.find_name("X").is_some(),
            "named button not reachable through live view content"
        );
        drop(content);
        view.deactivate();
        drop(view);

        assert!(
            FrameworkElement::parse(DICT_XAML).is_none(),
            "parse() must reject a ResourceDictionary root (not a FrameworkElement)"
        );

        assert!(
            FrameworkElement::parse(BROKEN_XAML).is_none(),
            "parse() must reject malformed XAML"
        );

        // Null component is short-circuited in the Rust wrapper before any FFI call.
        // SAFETY: null is an explicitly-handled input for load_component.
        assert!(
            !unsafe { load_component(ptr::null_mut(), "component.xaml") },
            "load_component(null, ...) must be false"
        );

        // Null URI exercises the C-layer `if (!component || !uri)` guard that
        // the Rust wrapper never reaches (it CStrings every &str). Call the raw
        // FFI directly with a valid component but a null uri.
        let registration =
            ClassBuilder::new("Nz.LoadTarget", ClassBase::ContentControl, NoopHandler)
                .register()
                .expect("class registration failed");
        let instance = registration
            .create_instance()
            .expect("create_instance returned None");
        // SAFETY: instance.raw() is live; uri is intentionally null to hit the
        // C-side guard, which must return false without dereferencing it.
        assert!(
            !unsafe { noesis_gui_load_component(instance.raw(), ptr::null()) },
            "noesis_gui_load_component(instance, null uri) must be false"
        );

        // With `x:Class="Nz.LoadTarget"` matching the registered class, LoadComponent
        // grafts the parsed body onto the existing instance. Removing the call
        // leaves the instance empty and the assertion below fails.
        let mut provider = HashMap::new();
        provider.insert(
            "component.xaml".to_string(),
            COMPONENT_XAML.as_bytes().to_vec(),
        );
        let _registered_provider =
            noesis_runtime::xaml_provider::set_xaml_provider(InMem(provider));

        assert!(
            !instance_has_named_child(instance.raw(), "GRAFTED"),
            "instance must not contain the named child before LoadComponent"
        );

        // SAFETY: instance.raw() is a live BaseComponent* for the lifetime of
        // `instance`; the URI resolves through the installed provider.
        let ran = unsafe { load_component(instance.raw(), "component.xaml") };
        assert!(
            ran,
            "load_component on a live instance + valid URI returned false"
        );

        assert!(
            instance_has_named_child(instance.raw(), "GRAFTED"),
            "LoadComponent did not graft the named child onto the instance"
        );

        drop(instance);
        drop(registration);
    }

    noesis_runtime::shutdown();
}

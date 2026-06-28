//! TODO §3 — `RelativeSource FindAncestor` (with `AncestorType` + `AncestorLevel`).
//!
//! One headless `#[test]` (Noesis inits once per process). Builds a tree with
//! two nested `Border` ancestors over a `StackPanel` holding two leaf
//! `TextBlock`s, then wires — from Rust — code-built bindings whose source is a
//! `RelativeSource FindAncestor` resolving the `Border` type by name:
//!
//!   * `Leaf1.Text` ← `FindAncestor` `Border`, level 1, path `Name` → the
//!     *nearest* Border ("Inner").
//!   * `Leaf2.Text` ← `FindAncestor` `Border`, level 2, path `Name` → the *next*
//!     Border up ("Outer").
//!
//! The assertions distinguish a working `FindAncestor` (and its level handling)
//! from a Self / no-ancestor binding: a Self binding on `Name` would read each
//! leaf's own name, and a wrong level would read the wrong Border. Also checks
//! the graceful-failure contract: an unknown ancestor type name does not resolve.

use std::collections::HashMap;

use noesis_runtime::binding::{Binding, set_binding};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="240" Height="160">
  <Border x:Name="Outer">
    <Border x:Name="Inner">
      <StackPanel>
        <TextBlock x:Name="Leaf1"/>
        <TextBlock x:Name="Leaf2"/>
      </StackPanel>
    </Border>
  </Border>
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
fn relative_source_find_ancestor_resolves_by_type_and_level() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(240, 160);
        view.activate();

        let content = view.content().expect("View::content returned None");
        let leaf1 = content.find_name("Leaf1").expect("find Leaf1");
        let leaf2 = content.find_name("Leaf2").expect("find Leaf2");

        // Graceful failure: an unknown / unregistered type name must NOT resolve,
        // while the genuine "Border" (referenced by the XAML, so registered)
        // does. This proves the reflection lookup gate, not just "didn't crash".
        let probe = Binding::new("Name");
        assert!(
            !probe.try_relative_source_find_ancestor("NoSuchAncestorType", 1),
            "unknown ancestor type name should not resolve"
        );
        assert!(
            probe.try_relative_source_find_ancestor("Border", 1),
            "Border ancestor type should resolve"
        );

        // Leaf1 ← nearest Border (level 1) Name == "Inner".
        let b1 = Binding::new("Name").relative_source_find_ancestor("Border", 1);
        assert!(
            set_binding(&leaf1, "Text", &b1),
            "set_binding Leaf1.Text failed"
        );

        // Leaf2 ← next Border up (level 2) Name == "Outer".
        let b2 = Binding::new("Name").relative_source_find_ancestor("Border", 2);
        assert!(
            set_binding(&leaf2, "Text", &b2),
            "set_binding Leaf2.Text failed"
        );

        assert!(view.update(0.0), "first Update should report change");
        let _ = view.update(0.016);

        assert_eq!(
            leaf1.get_string("Text").as_deref(),
            Some("Inner"),
            "FindAncestor level 1 should resolve the nearest Border (Inner)"
        );
        assert_eq!(
            leaf2.get_string("Text").as_deref(),
            Some("Outer"),
            "FindAncestor level 2 should resolve the next Border up (Outer)"
        );

        drop(leaf1);
        drop(leaf2);
        drop(content);
        view.deactivate();
        drop(view);
        drop(_guard);
    }

    noesis_runtime::shutdown();
}

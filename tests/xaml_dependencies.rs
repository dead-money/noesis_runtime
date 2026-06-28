//! Integration tests for `get_xaml_dependencies` and `load_xaml_component`.
//!
//! `get_xaml_dependencies` is exercised with a XAML whose dependencies are
//! fully predictable so the C++ trampoline and int→enum mapping are both
//! load-bearing. `load_xaml_component` is exercised with a bare
//! `<ResourceDictionary>` that `FrameworkElement::load` would reject.

use std::collections::HashMap;

use noesis_runtime::view::FrameworkElement;
use noesis_runtime::xaml::{XamlDependencyKind, get_xaml_dependencies, load_xaml_component};
use noesis_runtime::xaml_provider::{XamlProvider, set_xaml_provider};

// `local` maps to a clr-namespace so Noesis reports CustomControl as a
// UserControl dependency rather than trying to resolve it as a known built-in.
const DEPS_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<UserControl xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
             xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
             xmlns:local="clr-namespace:DepTest"
             FontFamily="./Fonts/#Bitter">
  <Grid>
    <Image Source="images/logo.png"/>
    <local:CustomControl/>
  </Grid>
</UserControl>"##;

// Root is NOT a FrameworkElement; load_xaml_component accepts it while
// FrameworkElement::load rejects it.
const DICT_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <SolidColorBrush x:Key="Accent" Color="#FF00FF00"/>
</ResourceDictionary>"##;

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
fn xaml_dependencies_and_typed_load() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let deps = get_xaml_dependencies(DEPS_XAML.as_bytes(), "test.xaml");
        assert!(
            !deps.is_empty(),
            "GetXamlDependencies returned no dependencies for a XAML that has several"
        );

        let root = deps
            .iter()
            .find(|d| d.kind == XamlDependencyKind::Root)
            .unwrap_or_else(|| panic!("no Root dependency reported; deps = {deps:?}"));
        assert!(
            root.uri.contains("UserControl"),
            "Root dependency uri {:?} should name the root type UserControl; deps = {deps:?}",
            root.uri
        );

        let font = deps
            .iter()
            .find(|d| d.kind == XamlDependencyKind::Font)
            .unwrap_or_else(|| panic!("no Font dependency reported; deps = {deps:?}"));
        assert!(
            font.uri.contains("Bitter"),
            "Font dependency uri {:?} should reference Bitter; deps = {deps:?}",
            font.uri
        );

        // The Image Source resolves relative to the base URI, so the uri ends
        // with the filename rather than matching it exactly.
        let file = deps
            .iter()
            .find(|d| d.kind == XamlDependencyKind::Filename && d.uri.contains("logo.png"))
            .unwrap_or_else(|| {
                panic!("no Filename dependency for images/logo.png; deps = {deps:?}")
            });
        assert!(
            file.uri.ends_with("logo.png"),
            "Filename dependency uri {:?} should end with logo.png; deps = {deps:?}",
            file.uri
        );

        let uc = deps
            .iter()
            .find(|d| d.kind == XamlDependencyKind::UserControl)
            .unwrap_or_else(|| panic!("no UserControl dependency reported; deps = {deps:?}"));
        assert!(
            uc.uri.contains("CustomControl"),
            "UserControl dependency uri {:?} should name CustomControl; deps = {deps:?}",
            uc.uri
        );

        // Malformed XAML → no dependencies, no crash.
        let none = get_xaml_dependencies(b"this is not xaml @@@ <<<", "");
        assert!(
            none.is_empty(),
            "malformed XAML should yield no dependencies, got {none:?}"
        );

        let mut map = HashMap::new();
        map.insert("dict.xaml".to_string(), DICT_XAML.as_bytes().to_vec());
        let _provider = set_xaml_provider(InMem(map));

        assert!(
            FrameworkElement::load("dict.xaml").is_none(),
            "FrameworkElement::load must reject a ResourceDictionary root"
        );

        let loaded = load_xaml_component("dict.xaml")
            .expect("load_xaml_component returned None for a valid ResourceDictionary");
        assert!(
            loaded.type_name().contains("ResourceDictionary"),
            "typed-load reported type {:?}, expected to contain ResourceDictionary",
            loaded.type_name()
        );
        assert!(
            !loaded.raw().is_null(),
            "LoadedComponent::raw must be non-null for a successful load"
        );
        drop(loaded);

        // Unknown URI → None.
        assert!(
            load_xaml_component("missing.xaml").is_none(),
            "load_xaml_component must return None for a URI the provider does not know"
        );
    }

    noesis_runtime::shutdown();
}

//! `ResourceDictionary::set_source` composes a dependency-ordered chain leaf
//! by leaf in Rust (the open-coded `gui::install_app_resources_chain`) while
//! the same parent also carries code-built base entries. Guards the
//! mixed-config application-resources scenario: a later leaf's
//! `{StaticResource}` into an earlier leaf must resolve, and a code-built
//! brush in the parent must be reachable from the scene alongside the chain.

use std::collections::HashMap;

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::resources::{ResourceDictionary, set_application_resources};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

const COLORS_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Color x:Key="Color.Gold">#FFFF0000</Color>
</ResourceDictionary>"##;

const BRUSHES_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <SolidColorBrush x:Key="Brush.Gold" Color="{StaticResource Color.Gold}"/>
</ResourceDictionary>"##;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:test="clr-namespace:Test"
      Background="#FF202020" Width="200" Height="200">
  <test:ColorProbe x:Name="ChainProbe"
                   Direct="#FF000000"
                   Indirect="{Binding Color, Source={StaticResource Brush.Gold}}"/>
  <test:ColorProbe x:Name="CodeProbe"
                   Direct="#FF000000"
                   Indirect="{Binding Color, Source={StaticResource Brush.Code}}"/>
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

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn set_source_chain_with_code_built_base_entries() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder =
            ClassBuilder::new("Test.ColorProbe", ClassBase::ContentControl, NoopHandler);
        let _direct_idx = builder.add_property("Direct", PropType::Color);
        let indirect_idx = builder.add_property("Indirect", PropType::Color);
        let registration = builder
            .register()
            .expect("class registration returned None");

        let mut bytes = HashMap::new();
        bytes.insert("Colors.xaml".to_string(), COLORS_XAML.as_bytes().to_vec());
        bytes.insert("Brushes.xaml".to_string(), BRUSHES_XAML.as_bytes().to_vec());
        bytes.insert("Scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _provider_guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        // Order is the contract: parent installed before any leaf loads.
        let mut parent = ResourceDictionary::new();
        set_application_resources(&parent);
        for uri in ["Colors.xaml", "Brushes.xaml"] {
            let mut child = ResourceDictionary::new();
            assert!(parent.add_merged(&child), "add_merged({uri}) failed");
            assert!(child.set_source(uri), "set_source({uri}) failed");
        }
        let code_brush = SolidColorBrush::new([0.0, 0.0, 1.0, 1.0]);
        assert!(
            parent.add_brush("Brush.Code", &code_brush),
            "add_brush(Brush.Code) failed"
        );

        let element = FrameworkElement::load("Scene.xaml")
            .expect("FrameworkElement::load(\"Scene.xaml\") returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0));
        let _ = view.update(0.016);

        let content = view.content().expect("View::content returned None");

        let chain_probe = content
            .find_name("ChainProbe")
            .expect("find_name(\"ChainProbe\") returned None");
        let chain_instance =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(chain_probe.raw()).unwrap()) };
        let chain_color = chain_instance
            .get_color(indirect_idx)
            .expect("get_color(ChainProbe.Indirect) returned None");
        // A transparent color here means the cross-leaf StaticResource
        // null-resolved: the leaf parsed without the parent scope.
        assert_color_eq(chain_color, (1.0, 0.0, 0.0, 1.0), "ChainProbe.Indirect");

        let code_probe = content
            .find_name("CodeProbe")
            .expect("find_name(\"CodeProbe\") returned None");
        let code_instance =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(code_probe.raw()).unwrap()) };
        let code_color = code_instance
            .get_color(indirect_idx)
            .expect("get_color(CodeProbe.Indirect) returned None");
        assert_color_eq(code_color, (0.0, 0.0, 1.0, 1.0), "CodeProbe.Indirect");

        drop(chain_probe);
        drop(code_probe);
        drop(content);
        view.deactivate();
        drop(view);
        drop(code_brush);
        drop(parent);
        drop(registration);
    }

    noesis_runtime::shutdown();
}

fn assert_color_eq(actual: (f32, f32, f32, f32), expected: (f32, f32, f32, f32), label: &str) {
    let (r, g, b, a) = actual;
    let (er, eg, eb, ea) = expected;
    let eps = 1e-3;
    let close = (r - er).abs() < eps
        && (g - eg).abs() < eps
        && (b - eb).abs() < eps
        && (a - ea).abs() < eps;
    assert!(close, "{label}: expected {expected:?}, got {actual:?}");
}

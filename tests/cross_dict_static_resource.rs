//! Regression: `install_app_resources_chain` must make each sibling merged
//! dictionary visible to subsequent siblings at parse time so that a
//! `StaticResource` in `Brushes.xaml` resolving into `Colors.xaml` does not
//! null-resolve and leave the brush color transparent.

use std::collections::HashMap;

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
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
  <test:ColorProbe x:Name="DirectProbe"
                   Direct="{StaticResource Color.Gold}"
                   Indirect="#FF000000"/>
  <test:ColorProbe x:Name="IndirectProbe"
                   Direct="#FF000000"
                   Indirect="{Binding Color, Source={StaticResource Brush.Gold}}"/>
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
fn cross_dict_static_resource_resolves_at_install_time() {
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
        let direct_idx = builder.add_property("Direct", PropType::Color);
        let indirect_idx = builder.add_property("Indirect", PropType::Color);
        let registration = builder
            .register()
            .expect("class registration returned None");

        let mut bytes = HashMap::new();
        bytes.insert("Colors.xaml".to_string(), COLORS_XAML.as_bytes().to_vec());
        bytes.insert("Brushes.xaml".to_string(), BRUSHES_XAML.as_bytes().to_vec());
        bytes.insert("Scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _provider_guard = noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        assert!(
            noesis_runtime::gui::install_app_resources_chain(&["Colors.xaml", "Brushes.xaml"]),
            "install_app_resources_chain returned false",
        );

        let element = FrameworkElement::load("Scene.xaml")
            .expect("FrameworkElement::load(\"Scene.xaml\") returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        // First update settles the visual tree and bindings; the second tick is a no-op.
        assert!(view.update(0.0));
        let _ = view.update(0.016);

        let content = view.content().expect("View::content returned None");

        let direct_probe = content
            .find_name("DirectProbe")
            .expect("find_name(\"DirectProbe\") returned None");
        let direct_instance =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(direct_probe.raw()).unwrap()) };
        let direct_color = direct_instance
            .get_color(direct_idx)
            .expect("get_color(Direct) returned None");
        // Control case: a failure here indicates a problem outside the cross-dict path.
        assert_color_eq(direct_color, (1.0, 0.0, 0.0, 1.0), "DirectProbe.Direct");

        let indirect_probe = content
            .find_name("IndirectProbe")
            .expect("find_name(\"IndirectProbe\") returned None");
        let indirect_instance =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(indirect_probe.raw()).unwrap()) };
        let indirect_color = indirect_instance
            .get_color(indirect_idx)
            .expect("get_color(Indirect) returned None");
        // If the chain installer null-resolved the cross-sibling StaticResource,
        // the brush color is transparent and this assertion fails.
        assert_color_eq(
            indirect_color,
            (1.0, 0.0, 0.0, 1.0),
            "IndirectProbe.Indirect",
        );

        drop(direct_probe);
        drop(indirect_probe);
        drop(content);
        view.deactivate();
        drop(view);
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
    assert!(
        close,
        "{label}: expected {expected:?}, got {actual:?} \
         (cross-merged-dict StaticResource lookup likely null-resolved)",
    );
}

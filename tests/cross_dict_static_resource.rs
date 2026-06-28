//! Regression test for cross-merged-dictionary `StaticResource` lookup
//! at the `application_resources` install site.
//!
//! ## What's being tested
//!
//! When a parent `ResourceDictionary` (`Theme.xaml`) merges sibling
//! dictionaries `[Colors.xaml, Brushes.xaml]`, references inside
//! `Brushes.xaml` like:
//!
//! ```xml
//! <SolidColorBrush x:Key="Brush.Gold" Color="{StaticResource Color.Gold}"/>
//! ```
//!
//! must resolve `Color.Gold` from the sibling `Colors.xaml`. WPF allows
//! this — sibling-merged dictionaries form one resolution scope. The
//! historical hommlet experience was that this **silently null-resolved**
//! at install time, leaving `Brush.Gold.Color` transparent and any text
//! painted with it invisible.
//!
//! ## How we observe the bug
//!
//! `Test.ColorProbe` is a custom `ContentControl` with two `Color` DPs:
//!
//! - `Direct` — set from the scene XAML directly via
//!   `{StaticResource Color.Gold}`. This walks scene → app resources →
//!   `Theme.xaml` → merged dicts → `Color.Gold`. Always works (control
//!   case for the test harness).
//! - `Indirect` — set via a `Binding` whose `Source` is
//!   `{StaticResource Brush.Gold}` and whose `Path` is `Color`. This
//!   reads back the `Color` of the brush itself. If the brush's color
//!   never resolved at parse time, this returns `Transparent` (or
//!   whatever Noesis defaults a null `Color` to).
//!
//! Both probes should report `(1, 0, 0, 1)` — pure red. If `Indirect` is
//! transparent / black-with-zero-alpha, the install-path bug is live.
//!
//! ## What this test pins
//!
//! `gui::install_app_resources_chain(&[uri, ...])` builds the merged-
//! dictionary tree leaf-by-leaf in C++: the parent is installed as
//! application resources up front (empty), then each leaf is added to
//! `parent.MergedDictionaries` and its `Source` assigned — so each
//! parse runs with the parent + previously-loaded siblings already
//! visible to the StaticResource walker. A regression here would
//! surface as `IndirectProbe.Indirect` reading back transparent (the
//! null-Color default).
//!
//! `gui::load_application_resources(uri)` — the simpler `LoadXaml +
//! SetApplicationResources` path — still has the bug; consumers with
//! cross-sibling `StaticResource` references must use the chain
//! installer.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test cross_dict_static_resource -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

/// Colors.xaml — defines a single `Color` resource the sibling brushes
/// dictionary references. `#FFFF0000` = pure opaque red.
const COLORS_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Color x:Key="Color.Gold">#FFFF0000</Color>
</ResourceDictionary>"##;

/// Brushes.xaml — defines a `SolidColorBrush` whose `Color` is a
/// `StaticResource` reference into the sibling `Colors.xaml`. This is
/// the construct the bug breaks.
const BRUSHES_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
                    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <SolidColorBrush x:Key="Brush.Gold" Color="{StaticResource Color.Gold}"/>
</ResourceDictionary>"##;

/// Scene.xaml — has two probes:
///
/// * `DirectProbe` reads `Color.Gold` straight from app resources.
/// * `IndirectProbe` binds its `Indirect` color to `Brush.Gold.Color`
///   via a `Binding`, surfacing whatever the brush's `Color` actually
///   ended up as after the merged-dictionary parse.
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
    fn on_changed(&mut self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[test]
fn cross_dict_static_resource_resolves_at_install_time() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // Register `Test.ColorProbe` with two Color DPs. Black initial
        // values so an unset DP reads as black (clearly distinguishable
        // from the expected red).
        let mut builder =
            ClassBuilder::new("Test.ColorProbe", ClassBase::ContentControl, NoopHandler);
        let direct_idx = builder.add_property("Direct", PropType::Color);
        let indirect_idx = builder.add_property("Indirect", PropType::Color);
        let registration = builder
            .register()
            .expect("class registration returned None");

        // The chain installer requests each leaf URI via the provider.
        // No `Theme.xaml` here — the chain installer constructs the
        // parent dictionary in C++ rather than parsing a manifest file.
        let mut bytes = HashMap::new();
        bytes.insert("Colors.xaml".to_string(), COLORS_XAML.as_bytes().to_vec());
        bytes.insert("Brushes.xaml".to_string(), BRUSHES_XAML.as_bytes().to_vec());
        bytes.insert("Scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _provider_guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        // Build the merged-dictionary chain leaf-by-leaf so each leaf
        // parses with the parent + previously-loaded siblings already
        // visible. Equivalent to Theme.xaml's MergedDictionaries entries.
        assert!(
            dm_noesis_runtime::gui::install_app_resources_chain(&["Colors.xaml", "Brushes.xaml"]),
            "install_app_resources_chain returned false",
        );

        let element = FrameworkElement::load("Scene.xaml")
            .expect("FrameworkElement::load(\"Scene.xaml\") returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        // First Update settles the visual tree + bindings. `Update`
        // returns `true` only when there are pending invalidations to
        // process; the second tick is a no-op here, hence not asserted.
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
        // Sanity check: the control case must work; if this fails the
        // problem is something else entirely (Color DP wiring, app
        // resources install, ...).
        assert_color_eq(direct_color, (1.0, 0.0, 0.0, 1.0), "DirectProbe.Direct");

        let indirect_probe = content
            .find_name("IndirectProbe")
            .expect("find_name(\"IndirectProbe\") returned None");
        let indirect_instance =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(indirect_probe.raw()).unwrap()) };
        let indirect_color = indirect_instance
            .get_color(indirect_idx)
            .expect("get_color(Indirect) returned None");
        // The bug-relevant assertion: `Brush.Gold.Color` should be the
        // red defined by `Color.Gold` in the sibling merged dictionary.
        // If the install path null-resolves the cross-sibling
        // StaticResource, the brush's color is transparent and this
        // assertion fails.
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

    dm_noesis_runtime::shutdown();
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

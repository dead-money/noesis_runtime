//! Safety smoke test — exercises the dm_noesis_runtime FFI surface against a
//! catalogue of valid-but-edge-case inputs and asserts no crashes.
//!
//! The whole file is a single `#[test]` because Noesis's `Init` /
//! `Shutdown` are once-per-process; sub-blocks isolated by `{}` scopes
//! cover one bug class each. Each block is preceded by an
//! `eprintln!("=== block name ===")` so a process abort tells you where
//! it died.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test safety_smoke -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::markup::MarkupExtensionRegistration;
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// Quiet handler that records nothing; we just want to make sure the FFI
// doesn't crash when callbacks fire.
struct QuietHandler;
impl PropertyChangeHandler for QuietHandler {
    fn on_changed(&mut self, _instance: Instance, _idx: u32, _value: PropertyValue<'_>) {}
}

#[derive(Default)]
struct Provider(HashMap<String, Vec<u8>>);
impl XamlProvider for Provider {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

/// Quickly build a one-shot view from a XAML string. Returns `(view,
/// content)` so the caller can `find_name` / drop in the right order.
fn build_view(xaml: &str) -> (View, FrameworkElement) {
    let mut bytes = HashMap::new();
    bytes.insert("scene.xaml".to_string(), xaml.as_bytes().to_vec());
    let provider = Provider(bytes);
    let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(provider);
    // _guard immediately drops at end of scope — but we need it alive for
    // the load. Hold it across the load by leaking; the next call to
    // set_xaml_provider replaces it.
    std::mem::forget(_guard);

    let element = FrameworkElement::load("scene.xaml")
        .expect("FrameworkElement::load returned None — scene.xaml not parseable");
    let mut view = View::create(element);
    view.set_size(200, 200);
    view.activate();
    let content = view.content().expect("View::content returned None");
    assert!(view.update(0.0));
    (view, content)
}

#[test]
fn safety_smoke() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    // ── Block 1: every PropType, registered and round-tripped ──────────────
    eprintln!("=== Block 1: PropType registration matrix ===");
    {
        let mut b = ClassBuilder::new("Smoke.AllProps", ClassBase::ContentControl, QuietHandler);
        b.add_property("Int", PropType::Int32);
        b.add_property("Flt", PropType::Float);
        b.add_property("Dbl", PropType::Double);
        b.add_property("Bln", PropType::Bool);
        b.add_property("Str", PropType::String);
        b.add_property("Thk", PropType::Thickness);
        b.add_property("Clr", PropType::Color);
        b.add_property("Rct", PropType::Rect);
        b.add_property("Img", PropType::ImageSource);
        b.add_property("Cmp", PropType::BaseComponent);
        let reg = b.register().expect("AllProps registration failed");
        assert_eq!(reg.num_properties(), 10);
        // Drop without ever instantiating — registry teardown should be clean.
        drop(reg);
    }

    // ── Block 2: instantiate a class with NO properties set ────────────────
    // Tests that DependencyObject::Init can box defaults for every PropType
    // without crashing. The motivating bug: ImageSource / BaseComponent
    // DPs created with `PropertyMetadata::Create()` (no default) crash
    // inside `ValueStorageManagerImpl<Ptr<BaseComponent>>::Box(null)` when
    // the visual-tree init walks deep enough to invoke a typed Box on the
    // missing default. The crash needs an *implicit Style with a
    // ControlTemplate* applied to the synthetic class — without that, the
    // init shortcut never invokes the typed Box. This block reproduces
    // hommlet's menu structure: app-resources Style with a binding-heavy
    // ControlTemplate, instance is a deeply-nested child of a Grid.
    eprintln!("=== Block 2: all-default props inside a templated Style ===");
    {
        let mut b = ClassBuilder::new("Smoke.AllDefaults", ClassBase::ContentControl, QuietHandler);
        b.add_property("Int", PropType::Int32);
        b.add_property("Flt", PropType::Float);
        b.add_property("Dbl", PropType::Double);
        b.add_property("Bln", PropType::Bool);
        b.add_property("Str", PropType::String);
        b.add_property("Thk", PropType::Thickness);
        b.add_property("Clr", PropType::Color);
        b.add_property("Rct", PropType::Rect);
        b.add_property("Img", PropType::ImageSource);
        b.add_property("Cmp", PropType::BaseComponent);
        let reg = b.register().expect("AllDefaults registration failed");

        // Implicit Style + ControlTemplate matching the synthetic class —
        // this is what makes the visual-tree init walk through the
        // typed-Box code path that crashes when defaults are missing.
        let theme_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    xmlns:smoke="clr-namespace:Smoke">
  <Style TargetType="{x:Type smoke:AllDefaults}">
    <Setter Property="Template">
      <Setter.Value>
        <ControlTemplate TargetType="{x:Type smoke:AllDefaults}">
          <Grid>
            <Rectangle>
              <Rectangle.Fill>
                <ImageBrush ImageSource="{Binding Img, RelativeSource={RelativeSource TemplatedParent}}"/>
              </Rectangle.Fill>
            </Rectangle>
          </Grid>
        </ControlTemplate>
      </Setter.Value>
    </Setter>
  </Style>
</ResourceDictionary>"##;
        // Two instances: one at root, one with Img set to a string URI
        // (hits the TypeConverter path); both use the templated style.
        let scene_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <Grid>
    <Grid>
      <smoke:AllDefaults x:Name="X"/>
      <smoke:AllDefaults x:Name="Y" Img="Images/missing.png"/>
    </Grid>
  </Grid>
</Grid>"##;
        let mut bytes = HashMap::new();
        bytes.insert("theme.xaml".into(), theme_xaml.as_bytes().to_vec());
        bytes.insert("scene.xaml".into(), scene_xaml.as_bytes().to_vec());
        let _g = dm_noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);

        assert!(
            dm_noesis_runtime::gui::load_application_resources("theme.xaml"),
            "load_application_resources(theme.xaml) returned false"
        );

        let element = FrameworkElement::load("scene.xaml")
            .expect("FrameworkElement::load returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        // Two updates so the render-tree visibility / layout passes settle.
        assert!(view.update(0.0));
        let _ = view.update(0.016);
        let content = view.content().expect("View::content returned None");
        let _inst_x = content.find_name("X").expect("X not found in scene");
        let _inst_y = content.find_name("Y").expect("Y not found in scene");
        drop(_inst_x);
        drop(_inst_y);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 3: Style TargetType matches synthetic class ──────────────────
    // Exercises Noesis's `IsAssignableFrom` + `GetAncestorInfo` against our
    // synthetic TypeClass.
    eprintln!("=== Block 3: Style TargetType against synthetic class ===");
    {
        let mut b = ClassBuilder::new("Smoke.Styled", ClassBase::ContentControl, QuietHandler);
        let _ = b.add_property("Width2", PropType::Float);
        let reg = b.register().expect("Styled registration failed");

        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <Grid.Resources>
    <Style TargetType="{x:Type smoke:Styled}">
      <Setter Property="Width" Value="32"/>
    </Style>
  </Grid.Resources>
  <smoke:Styled x:Name="S"/>
</Grid>"##;
        let (mut view, content) = build_view(xaml);
        let inst = content.find_name("S").expect("S not found");
        drop(inst);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 4: ControlTemplate with TemplatedParent bindings to synthetic DP ──
    // This is the crash path the menu hits — NineSlicer's ControlTemplate
    // uses `<Binding ... RelativeSource={RelativeSource TemplatedParent}>`
    // against the slicer's own DPs. Tests that the binding system can read
    // synthetic DPs without exploding.
    eprintln!("=== Block 4: ControlTemplate w/ TemplatedParent bindings ===");
    {
        let mut b = ClassBuilder::new("Smoke.Templated", ClassBase::ContentControl, QuietHandler);
        b.add_property("Source", PropType::ImageSource);
        b.add_property("SliceThickness", PropType::Thickness);
        b.add_property("LeftRect", PropType::Rect);
        let reg = b.register().expect("Templated registration failed");

        // Mirrors the structure of `assets/Controls/NineSlicer.xaml` —
        // ControlTemplate that binds back to the templated parent's DPs
        // for both layout (ColumnDefinition.Width) and visual
        // (ImageBrush.Viewbox) properties.
        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <Grid.Resources>
    <Style TargetType="{x:Type smoke:Templated}">
      <Setter Property="Template">
        <Setter.Value>
          <ControlTemplate TargetType="{x:Type smoke:Templated}">
            <Grid>
              <Grid.ColumnDefinitions>
                <ColumnDefinition Width="{Binding SliceThickness.Left, RelativeSource={RelativeSource TemplatedParent}}"/>
                <ColumnDefinition Width="*"/>
              </Grid.ColumnDefinitions>
              <Rectangle Grid.Column="0">
                <Rectangle.Fill>
                  <ImageBrush ImageSource="{Binding Source, RelativeSource={RelativeSource TemplatedParent}}"
                              Viewbox="{Binding LeftRect, RelativeSource={RelativeSource TemplatedParent}}"
                              ViewboxUnits="Absolute"/>
                </Rectangle.Fill>
              </Rectangle>
            </Grid>
          </ControlTemplate>
        </Setter.Value>
      </Setter>
    </Style>
  </Grid.Resources>
  <smoke:Templated x:Name="T" SliceThickness="4,5,6,7"/>
</Grid>"##;
        let (mut view, content) = build_view(xaml);
        let inst = content.find_name("T").expect("T not found");
        drop(inst);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 5: multiple classes registered + cross-referenced in one XAML ──
    // Mirrors hommlet's "register NineSlicer + ThreeSlicer + Localize before
    // the menu loads" pattern. Catches any cross-talk between synthetic
    // TypeClass entries.
    eprintln!("=== Block 5: multi-class registration + cross-reference ===");
    {
        let mut b1 = ClassBuilder::new("Smoke.Outer", ClassBase::ContentControl, QuietHandler);
        b1.add_property("Title", PropType::String);
        let reg1 = b1.register().expect("Outer registration failed");

        let mut b2 = ClassBuilder::new("Smoke.Inner", ClassBase::ContentControl, QuietHandler);
        b2.add_property("Value", PropType::Int32);
        let reg2 = b2.register().expect("Inner registration failed");

        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:Outer x:Name="O" Title="hello">
    <smoke:Inner x:Name="I" Value="42"/>
  </smoke:Outer>
</Grid>"##;
        let (mut view, content) = build_view(xaml);
        let outer = content.find_name("O").expect("O not found");
        let inner = content.find_name("I").expect("I not found");
        drop(outer);
        drop(inner);
        drop(content);
        view.deactivate();
        drop(view);
        // Drop in reverse registration order (LIFO).
        drop(reg2);
        drop(reg1);
    }

    // ── Block 6: MarkupExtension feeding into a synthetic-class property ────
    // Localize-style markup providing the value for a registered DP.
    eprintln!("=== Block 6: markup extension feeding synthetic DP ===");
    {
        let mut b = ClassBuilder::new("Smoke.Labeled", ClassBase::ContentControl, QuietHandler);
        b.add_property("Caption", PropType::String);
        let reg = b.register().expect("Labeled registration failed");

        let ext = MarkupExtensionRegistration::from_closure("Smoke.Loc", |key| match key {
            "greeting" => Some("Hello, world!".to_string()),
            _ => None,
        })
        .expect("Loc extension registration failed");

        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:Labeled x:Name="L" Caption="{smoke:Loc greeting}"/>
</Grid>"##;
        let (mut view, content) = build_view(xaml);
        let inst = content.find_name("L").expect("L not found");
        drop(inst);
        drop(content);
        view.deactivate();
        drop(view);
        drop(ext);
        drop(reg);
    }

    // ── Block 7: ResourceDictionary as application resources ────────────────
    // Mirrors hommlet's `load_application_resources("Theme/Theme.xaml")` —
    // a ResourceDictionary that holds a Style TargetType="{x:Type smoke:X}".
    // The application-resources install happens BEFORE the scene XAML
    // loads. This is the most-likely source of the menu's crash.
    eprintln!("=== Block 7: app-resources w/ synthetic-class style ===");
    {
        let mut b = ClassBuilder::new("Smoke.AppRes", ClassBase::ContentControl, QuietHandler);
        let _ = b.add_property("V", PropType::Float);
        let reg = b.register().expect("AppRes registration failed");

        let theme_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    xmlns:smoke="clr-namespace:Smoke">
  <Style TargetType="{x:Type smoke:AppRes}">
    <Setter Property="Width" Value="32"/>
  </Style>
</ResourceDictionary>"##;
        let scene_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:AppRes x:Name="A"/>
</Grid>"##;
        let mut bytes = HashMap::new();
        bytes.insert("theme.xaml".to_string(), theme_xaml.as_bytes().to_vec());
        bytes.insert("scene.xaml".to_string(), scene_xaml.as_bytes().to_vec());
        let _g = dm_noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);
        // Application resources install BEFORE the View ever exists.
        let installed = dm_noesis_runtime::gui::load_application_resources("theme.xaml");
        assert!(installed, "load_application_resources returned false");

        let element = FrameworkElement::load("scene.xaml")
            .expect("FrameworkElement::load returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        let content = view.content().expect("View::content returned None");
        assert!(view.update(0.0));
        let inst = content.find_name("A").expect("A not found");
        drop(inst);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 7b: app-resources with MergedDictionaries indirection ────────
    // Hommlet's Theme.xaml is a ResourceDictionary that pulls in
    // Controls/NineSlicer.xaml via MergedDictionaries; the inner
    // dictionary holds the `{x:Type aor:NineSlicer}` Style + a real
    // ControlTemplate. This replicates that layout.
    eprintln!("=== Block 7b: app-resources via MergedDictionaries ===");
    {
        let mut b = ClassBuilder::new("Smoke.MdNine", ClassBase::ContentControl, QuietHandler);
        b.add_property("Source", PropType::ImageSource);
        b.add_property("SliceThickness", PropType::Thickness);
        b.add_property("LeftRect", PropType::Rect);
        let reg = b.register().expect("MdNine registration failed");

        let inner_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    xmlns:smoke="clr-namespace:Smoke">
  <Style TargetType="{x:Type smoke:MdNine}">
    <Setter Property="Template">
      <Setter.Value>
        <ControlTemplate TargetType="{x:Type smoke:MdNine}">
          <Grid>
            <Grid.ColumnDefinitions>
              <ColumnDefinition Width="{Binding SliceThickness.Left, RelativeSource={RelativeSource TemplatedParent}}"/>
              <ColumnDefinition Width="*"/>
            </Grid.ColumnDefinitions>
            <Rectangle Grid.Column="0">
              <Rectangle.Fill>
                <ImageBrush ImageSource="{Binding Source, RelativeSource={RelativeSource TemplatedParent}}"
                            Viewbox="{Binding LeftRect, RelativeSource={RelativeSource TemplatedParent}}"
                            ViewboxUnits="Absolute"/>
              </Rectangle.Fill>
            </Rectangle>
          </Grid>
        </ControlTemplate>
      </Setter.Value>
    </Setter>
  </Style>
</ResourceDictionary>"##;
        let theme_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <ResourceDictionary.MergedDictionaries>
    <ResourceDictionary Source="inner.xaml"/>
  </ResourceDictionary.MergedDictionaries>
</ResourceDictionary>"##;
        let scene_xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:MdNine x:Name="N" SliceThickness="4,5,6,7"/>
</Grid>"##;
        let mut bytes = HashMap::new();
        bytes.insert("inner.xaml".into(), inner_xaml.as_bytes().to_vec());
        bytes.insert("theme.xaml".into(), theme_xaml.as_bytes().to_vec());
        bytes.insert("scene.xaml".into(), scene_xaml.as_bytes().to_vec());
        let _g = dm_noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);

        let installed = dm_noesis_runtime::gui::load_application_resources("theme.xaml");
        assert!(
            installed,
            "load_application_resources(theme.xaml) returned false"
        );

        let element = FrameworkElement::load("scene.xaml")
            .expect("FrameworkElement::load returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        let content = view.content().expect("View::content returned None");
        assert!(view.update(0.0));
        let inst = content.find_name("N").expect("N not found");
        drop(inst);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 7c: handler re-enters Instance::set_* during its own callback ─
    // A handler that writes back to a DP it just observed (the slicer
    // pattern: SliceThickness changes → recompute viewboxes → set_rect).
    // Verifies the C++ trampoline + Rust side-table can be re-entered
    // without deadlocking the registry mutex or corrupting state.
    eprintln!("=== Block 7c: re-entrant Instance::set_* in handler ===");
    {
        struct ReentrantHandler {
            input_idx: u32,
            output_idx: u32,
        }
        impl PropertyChangeHandler for ReentrantHandler {
            fn on_changed(&mut self, instance: Instance, idx: u32, value: PropertyValue<'_>) {
                if idx == self.input_idx {
                    if let PropertyValue::Float(f) = value {
                        // Mirror to output_idx — fires another callback for
                        // output_idx, which the matches above ignore (no
                        // infinite loop).
                        instance.set_float(self.output_idx, f * 2.0);
                    }
                }
            }
        }

        let mut b = ClassBuilder::new(
            "Smoke.Reentrant",
            ClassBase::ContentControl,
            ReentrantHandler {
                input_idx: 0,
                output_idx: 1,
            },
        );
        b.add_property("In", PropType::Float);
        b.add_property("Out", PropType::Float);
        let reg = b.register().expect("Reentrant registration failed");

        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:Reentrant x:Name="R" In="3.5"/>
</Grid>"##;
        let (mut view, content) = build_view(xaml);
        let inst_handle = content.find_name("R").expect("R not found");
        let inst =
            unsafe { Instance::from_raw(std::ptr::NonNull::new(inst_handle.raw()).unwrap()) };
        // Out should have been set to 7.0 by the re-entrant callback.
        assert_eq!(inst.get_float(1), Some(7.0));
        drop(inst_handle);
        drop(content);
        view.deactivate();
        drop(view);
        drop(reg);
    }

    // ── Block 8: drop-order regression — registration outlives view ────────
    eprintln!("=== Block 7: registration outlives view ===");
    {
        let mut b = ClassBuilder::new("Smoke.Long", ClassBase::ContentControl, QuietHandler);
        b.add_property("V", PropType::Int32);
        let reg = b.register().expect("Long registration failed");
        let xaml = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:smoke="clr-namespace:Smoke">
  <smoke:Long x:Name="L" V="7"/>
</Grid>"##;
        // Build + drop view BEFORE reg.
        {
            let (mut view, content) = build_view(xaml);
            drop(content);
            view.deactivate();
            drop(view);
        }
        drop(reg);
    }

    dm_noesis_runtime::shutdown();
}

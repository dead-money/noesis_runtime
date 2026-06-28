//! Safety smoke: edge-case FFI inputs (all `PropType`s, template bindings, re-entrant handlers, drop order) asserted to not crash.
//! Single `#[test]`: Noesis init/shutdown is once-per-process; each `{}` block covers one crash class.

use std::collections::HashMap;

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::markup::MarkupExtensionRegistration;
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

// Quiet handler that records nothing; just confirms the FFI doesn't crash
// when callbacks fire.
struct QuietHandler;
impl PropertyChangeHandler for QuietHandler {
    fn on_changed(&self, _instance: Instance, _idx: u32, _value: PropertyValue<'_>) {}
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

/// Build a one-shot view from a XAML string. Returns `(view, content)` so
/// the caller can `find_name` / drop in the right order.
fn build_view(xaml: &str) -> (View, FrameworkElement) {
    let mut bytes = HashMap::new();
    bytes.insert("scene.xaml".to_string(), xaml.as_bytes().to_vec());
    let provider = Provider(bytes);
    let _guard = noesis_runtime::xaml_provider::set_xaml_provider(provider);
    // _guard would otherwise drop at end of scope, but we need it alive for
    // the load. Hold it across the load by leaking; the next call to
    // set_xaml_provider replaces it.
    std::mem::forget(_guard);

    let element = FrameworkElement::load("scene.xaml")
        .expect("FrameworkElement::load returned None : scene.xaml not parseable");
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
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

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
        // Drop without ever instantiating; registry teardown should be clean.
        drop(reg);
    }

    // The motivating crash: `ImageSource` / `BaseComponent` DPs created with
    // `PropertyMetadata::Create()` (no default) crash inside
    // `ValueStorageManagerImpl<Ptr<BaseComponent>>::Box(null)` when the
    // visual-tree init walks deep enough to invoke a typed Box on the missing
    // default. Reproducing it requires an *implicit Style + ControlTemplate*
    // applied to the synthetic class; without that the init shortcut never
    // invokes the typed Box.
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

        // Implicit Style + ControlTemplate matching the synthetic class. This
        // makes the visual-tree init walk through the typed-Box code path that
        // crashes when defaults are missing.
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
        let _g = noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);

        assert!(
            noesis_runtime::gui::load_application_resources("theme.xaml"),
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

    // Exercises `IsAssignableFrom` + `GetAncestorInfo` against a synthetic TypeClass.
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

    // The binding system must read synthetic DPs through a ControlTemplate's
    // `RelativeSource TemplatedParent` without crashing.
    eprintln!("=== Block 4: ControlTemplate w/ TemplatedParent bindings ===");
    {
        let mut b = ClassBuilder::new("Smoke.Templated", ClassBase::ContentControl, QuietHandler);
        b.add_property("Source", PropType::ImageSource);
        b.add_property("SliceThickness", PropType::Thickness);
        b.add_property("LeftRect", PropType::Rect);
        let reg = b.register().expect("Templated registration failed");

        // ControlTemplate that binds back to the templated parent's DPs for both
        // layout (ColumnDefinition.Width) and visual (ImageBrush.Viewbox).
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

    // Multiple synthetic classes registered before the XAML loads; catches
    // cross-talk between TypeClass entries.
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

    // Markup extension providing the value for a registered synthetic DP.
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
        let _g = noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);
        // Application resources install BEFORE the View ever exists.
        let installed = noesis_runtime::gui::load_application_resources("theme.xaml");
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

    // App resources via `MergedDictionaries`; the inner dictionary holds the
    // Style + ControlTemplate for the synthetic class.
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
        let _g = noesis_runtime::xaml_provider::set_xaml_provider(Provider(bytes));
        std::mem::forget(_g);

        let installed = noesis_runtime::gui::load_application_resources("theme.xaml");
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

    // A handler that writes back to a DP during its own callback (set_* on
    // `instance` while the callback is running). Verifies the C++ trampoline
    // and Rust side-table tolerate re-entrance without deadlock or corruption.
    eprintln!("=== Block 7c: re-entrant Instance::set_* in handler ===");
    {
        struct ReentrantHandler {
            input_idx: u32,
            output_idx: u32,
        }
        impl PropertyChangeHandler for ReentrantHandler {
            fn on_changed(&self, instance: Instance, idx: u32, value: PropertyValue<'_>) {
                if idx == self.input_idx
                    && let PropertyValue::Float(f) = value
                {
                    // Mirror to output_idx; fires another callback for
                    // output_idx, which the matches above ignore (no
                    // infinite loop).
                    instance.set_float(self.output_idx, f * 2.0);
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

    noesis_runtime::shutdown();
}

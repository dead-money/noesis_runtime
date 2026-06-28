//! TODO §7 — parse a `<ControlTemplate>` / `<DataTemplate>` from a string and
//! assign it, asserting an observable consequence.
//!
//! `ControlTemplate`: a `Button` is given a code-parsed template containing a
//! named `Border` part. After a layout pump the part becomes resolvable both
//! via `FrameworkElement::GetTemplateChild` and via `FrameworkTemplate::FindName`
//! — and `GetTemplate` reads the assigned template back. A button WITHOUT the
//! template finds no such part (negative discriminator). A stubbed `set_template`
//! leaves the part unfindable and `control_template` empty.
//!
//! `DataTemplate`: a `ContentControl` gets a code-parsed `ContentTemplate`; the
//! assignment round-trips through Noesis (`get_component("ContentTemplate")`
//! returns the exact same object). A fresh control has no `ContentTemplate`.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test templates -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::styles::{ControlTemplate, DataTemplate};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="300" Height="300">
  <StackPanel>
    <Button x:Name="Templated" Width="120" Height="40"/>
    <Button x:Name="Bare" Width="120" Height="40"/>
    <ContentControl x:Name="Host" Content="payload" Width="120" Height="40"/>
  </StackPanel>
</Grid>"##;

const CONTROL_TEMPLATE: &str = r##"<ControlTemplate
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    TargetType="Button">
  <Border x:Name="PART_Root" Background="#FF112233">
    <ContentPresenter/>
  </Border>
</ControlTemplate>"##;

const DATA_TEMPLATE: &str = r##"<DataTemplate
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <TextBlock x:Name="DT_Text" Text="templated-content"/>
</DataTemplate>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

#[test]
fn parse_and_assign_templates() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let root = FrameworkElement::load("scene.xaml").expect("scene load");
        let mut view = View::create(root);
        view.set_size(300, 300);
        view.activate();
        view.update(0.0);

        let mut templated = view
            .content()
            .and_then(|c| c.find_name("Templated"))
            .expect("find Templated");
        let bare = view
            .content()
            .and_then(|c| c.find_name("Bare"))
            .expect("find Bare");
        let mut host = view
            .content()
            .and_then(|c| c.find_name("Host"))
            .expect("find Host");

        // ── ControlTemplate ──────────────────────────────────────────────────
        // Parsing the wrong root type fails cleanly.
        assert!(
            ControlTemplate::parse(DATA_TEMPLATE).is_none(),
            "a <DataTemplate> is not a ControlTemplate"
        );

        let template = ControlTemplate::parse(CONTROL_TEMPLATE).expect("parse ControlTemplate");
        // A themed Button carries a default template, so the discriminator is
        // not "had no template" but "our named part appears only where we
        // assigned our template" (checked below). GetTemplate must, however,
        // return the exact ControlTemplate object we just assigned.
        assert!(
            templated.set_control_template(&template),
            "assign template to a Button"
        );
        view.update(0.016);

        let read_back_template = templated
            .control_template()
            .expect("GetTemplate returns a ControlTemplate");
        assert_eq!(
            read_back_template.raw(),
            template.raw(),
            "GetTemplate returns the exact assigned ControlTemplate"
        );

        // The named part is reachable through the applied template.
        let part = templated
            .template_child("PART_Root")
            .expect("PART_Root resolvable via GetTemplateChild after apply");
        assert_eq!(
            part.name().as_deref(),
            Some("PART_Root"),
            "resolved part carries the expected x:Name"
        );

        // FrameworkTemplate::FindName resolves the same part against the parent.
        assert!(
            template.find_name("PART_Root", &templated).is_some(),
            "FrameworkTemplate::FindName finds PART_Root"
        );

        // The bare button never had the template — no such part.
        assert!(
            bare.template_child("PART_Root").is_none(),
            "untemplated button has no PART_Root"
        );

        // ── DataTemplate ─────────────────────────────────────────────────────
        assert!(
            DataTemplate::parse(CONTROL_TEMPLATE).is_none(),
            "a <ControlTemplate> is not a DataTemplate"
        );

        let data_template = DataTemplate::parse(DATA_TEMPLATE).expect("parse DataTemplate");
        assert!(
            host.get_component("ContentTemplate").is_none(),
            "ContentControl has no ContentTemplate yet"
        );
        // SAFETY: data_template.raw() is a live DataTemplate* (BaseComponent*);
        // the DP setter (SetContentTemplate) AddRefs it.
        assert!(
            unsafe { host.set_component("ContentTemplate", data_template.raw()) },
            "assign ContentTemplate via the component DP path"
        );
        let read_back = host
            .get_component("ContentTemplate")
            .expect("ContentTemplate reads back non-null");
        assert_eq!(
            read_back.as_ptr(),
            data_template.raw(),
            "ContentTemplate round-trips through Noesis to the same object"
        );
        view.update(0.032);
    }

    dm_noesis_runtime::shutdown();
}

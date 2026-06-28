//! TODO §9 (C) — Factory registration + `ContentProperty` metadata.
//!
//! A Rust-backed class registers a Factory creator (so XAML can instantiate
//! `<ns:Card/>`). This test asserts the factory introspection surface and that
//! `ContentProperty` attribution actually redirects XAML child content into the
//! named property: it parses `<dm:Card>hello</dm:Card>` and reads the text back
//! out of the redirected `Caption` property THROUGH Noesis. A stubbed
//! `set_content_property` leaves content on the inherited `Content` property, so
//! `Caption` stays empty and the assertion fails. A type with no factory is not
//! instantiated, so its named element is absent from the parsed tree.

use noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use noesis_runtime::ffi::{ClassBase, PropType};
use noesis_runtime::reflection::{is_component_registered, set_content_property};
use noesis_runtime::view::FrameworkElement;

struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

const CARD_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:dm="clr-namespace:DmTest">
  <dm:Card x:Name="TheCard">hello world</dm:Card>
</Grid>"##;

// References a type that was never registered → no factory → the element is not
// instantiated (Noesis parses the rest of the tree but skips the unknown tag).
const MISSING_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:dm="clr-namespace:DmTest">
  <dm:GhostCard x:Name="Nope"/>
</Grid>"##;

#[test]
fn factory_and_content_property() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut builder = ClassBuilder::new("DmTest.Card", ClassBase::ContentControl, NoopHandler);
        builder.add_property("Caption", PropType::String);
        let _reg = builder.register().expect("class registration failed");

        // Factory introspection: the registered class is creatable from XAML;
        // an unregistered name is not.
        assert!(
            is_component_registered("DmTest.Card"),
            "registered class should be in the Factory"
        );
        assert!(
            !is_component_registered("DmTest.GhostCard"),
            "unregistered class should not be in the Factory"
        );

        // Redirect XAML child content into our own `Caption` property.
        assert!(
            set_content_property("DmTest.Card", "Caption"),
            "set_content_property returned false"
        );
        assert!(
            !set_content_property("DmTest.NoSuchType", "Caption"),
            "set_content_property on unknown type should fail"
        );

        // Parse + instantiate via the factory, then read Caption back THROUGH
        // Noesis. The text content landed there because of the ContentProperty.
        let root = FrameworkElement::parse(CARD_XAML).expect("parse(CARD_XAML) returned None");
        let card = root
            .find_name("TheCard")
            .expect("find_name(TheCard) returned None");
        assert_eq!(
            card.get_string("Caption").as_deref(),
            Some("hello world"),
            "ContentProperty did not redirect text content into Caption"
        );

        // A type with no factory is not instantiated: the unknown tag is
        // skipped, so the named ghost element is absent from the tree.
        let ghost_root =
            FrameworkElement::parse(MISSING_XAML).expect("parse(MISSING_XAML) returned None");
        assert!(
            ghost_root.find_name("Nope").is_none(),
            "an unregistered type must not be instantiated"
        );
    }

    noesis_runtime::shutdown();
}

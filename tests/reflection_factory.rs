//! Factory registration and `ContentProperty` metadata: XAML text content redirects into a
//! named property; unregistered types are silently skipped (not errored) during parsing.

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
      xmlns:nz="clr-namespace:NzTest">
  <nz:Card x:Name="TheCard">hello world</nz:Card>
</Grid>"##;

// References a type that was never registered → no factory → the element is not
// instantiated (Noesis parses the rest of the tree but skips the unknown tag).
const MISSING_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:nz="clr-namespace:NzTest">
  <nz:GhostCard x:Name="Nope"/>
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
        let mut builder = ClassBuilder::new("NzTest.Card", ClassBase::ContentControl, NoopHandler);
        builder.add_property("Caption", PropType::String);
        let _reg = builder.register().expect("class registration failed");

        assert!(
            is_component_registered("NzTest.Card"),
            "registered class should be in the Factory"
        );
        assert!(
            !is_component_registered("NzTest.GhostCard"),
            "unregistered class should not be in the Factory"
        );

        assert!(
            set_content_property("NzTest.Card", "Caption"),
            "set_content_property returned false"
        );
        assert!(
            !set_content_property("NzTest.NoSuchType", "Caption"),
            "set_content_property on unknown type should fail"
        );

        let root = FrameworkElement::parse(CARD_XAML).expect("parse(CARD_XAML) returned None");
        let card = root
            .find_name("TheCard")
            .expect("find_name(TheCard) returned None");
        assert_eq!(
            card.get_string("Caption").as_deref(),
            Some("hello world"),
            "ContentProperty did not redirect text content into Caption"
        );

        let ghost_root =
            FrameworkElement::parse(MISSING_XAML).expect("parse(MISSING_XAML) returned None");
        assert!(
            ghost_root.find_name("Nope").is_none(),
            "an unregistered type must not be instantiated"
        );
    }

    noesis_runtime::shutdown();
}

//! Per-element `Resources` dictionary: set, look up (including logical-chain
//! inheritance by children), and clear; missing keys return `None` without throwing.

use std::ffi::CStr;

use noesis_runtime::ffi::noesis_unbox_string;
use noesis_runtime::resources::ResourceDictionary;
use noesis_runtime::view::FrameworkElement;

const SCENE: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Border x:Name="Child"/>
</Grid>"##;

fn unbox(ptr: *mut std::ffi::c_void) -> Option<String> {
    // SAFETY: `ptr` is a live boxed value borrowed from a dictionary entry.
    let s = unsafe { noesis_unbox_string(ptr) };
    if s.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}

#[test]
fn element_resources_set_and_find() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut root = FrameworkElement::parse(SCENE).expect("parse scene");
        let child = root.find_name("Child").expect("find Child");

        assert!(
            root.find_resource("Accent").is_none(),
            "key must not resolve before set_resources"
        );

        let mut dict = ResourceDictionary::new();
        assert!(dict.add_string("Accent", "tango"));
        assert!(root.set_resources(&dict), "set_resources on a Grid");

        let hit = root
            .find_resource("Accent")
            .expect("Accent resolves on the owner element");
        assert_eq!(
            unbox(hit.as_ptr()).as_deref(),
            Some("tango"),
            "resolved resource unboxes to the stored string"
        );

        assert!(
            child.find_resource("Accent").is_some(),
            "child resolves the ancestor's resource through the logical chain"
        );

        assert!(
            root.find_resource("Missing").is_none(),
            "unknown key -> None"
        );

        let read_back = root.resources().expect("resources() after set");
        assert!(
            read_back.contains("Accent"),
            "read-back dictionary contains the installed key"
        );
        assert_eq!(read_back.len(), 1, "exactly the one entry we added");
    }

    noesis_runtime::shutdown();
}

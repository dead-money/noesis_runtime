//! TODO §7 — per-element `Resources` get/set + non-throwing `FindResource`.
//!
//! Fail-if-stubbed: a freshly-parsed element either has no local resources or a
//! dictionary without our key; after `set_resources` with a code-built
//! dictionary, `find_resource` resolves the key THROUGH Noesis's logical-chain
//! lookup (and unboxes to the stored string), a child element inherits it via
//! that same chain, a missing key returns `None`, and `get_resources` reads the
//! installed dictionary back (containing the key). A no-op `set_resources` would
//! leave `find_resource` empty.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test element_resources -- --nocapture`

use std::ffi::CStr;

use dm_noesis_runtime::ffi::dm_noesis_unbox_string;
use dm_noesis_runtime::resources::ResourceDictionary;
use dm_noesis_runtime::view::FrameworkElement;

const SCENE: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Border x:Name="Child"/>
</Grid>"##;

fn unbox(ptr: *mut std::ffi::c_void) -> Option<String> {
    // SAFETY: `ptr` is a live boxed value borrowed from a dictionary entry.
    let s = unsafe { dm_noesis_unbox_string(ptr) };
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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let mut root = FrameworkElement::parse(SCENE).expect("parse scene");
        let child = root.find_name("Child").expect("find Child");

        // Nothing resolves the key before we install resources.
        assert!(
            root.find_resource("Accent").is_none(),
            "key must not resolve before set_resources"
        );

        let mut dict = ResourceDictionary::new();
        assert!(dict.add_string("Accent", "tango"));
        assert!(root.set_resources(&dict), "set_resources on a Grid");

        // FindResource resolves on the element that owns the dictionary…
        let hit = root
            .find_resource("Accent")
            .expect("Accent resolves on the owner element");
        assert_eq!(
            unbox(hit.as_ptr()).as_deref(),
            Some("tango"),
            "resolved resource unboxes to the stored string"
        );

        // …and on a descendant via the logical parent chain.
        assert!(
            child.find_resource("Accent").is_some(),
            "child resolves the ancestor's resource through the logical chain"
        );

        // Negative: an unknown key is a clean None (non-throwing lookup).
        assert!(
            root.find_resource("Missing").is_none(),
            "unknown key -> None"
        );

        // get_resources reads the installed dictionary back.
        let read_back = root.get_resources().expect("get_resources after set");
        assert!(
            read_back.contains("Accent"),
            "read-back dictionary contains the installed key"
        );
        assert_eq!(read_back.len(), 1, "exactly the one entry we added");
    }

    dm_noesis_runtime::shutdown();
}

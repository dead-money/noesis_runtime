//! TODO §7 — `ResourceDictionary` access: create/own a dictionary, add a boxed
//! string under a key, look it up (hit + miss), wire a merged dictionary, parse
//! a `<ResourceDictionary>` from XAML, and install + read back the process-wide
//! application resources.
//!
//! Fail-if-stubbed: every assertion reads back THROUGH Noesis — the looked-up
//! value is unboxed and compared to the original string; a miss returns `None`;
//! a merged key resolves through the parent; the parsed dictionary's entry
//! survives the round-trip; and the installed application resources are visible
//! via `GUI::GetApplicationResources` (present + contains the key, including a
//! merged one). A no-op stub of any entrypoint would flip one of these.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p noesis_runtime --test resource_dictionary -- --nocapture`

use std::ffi::CStr;

use noesis_runtime::ffi::noesis_unbox_string;
use noesis_runtime::resources::{
    ResourceDictionary, application_resources_contains, application_resources_present,
    set_application_resources,
};

const MERGED_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    xmlns:sys="clr-namespace:System;assembly=mscorlib">
  <sys:String x:Key="Merged.Greeting">from-merged</sys:String>
</ResourceDictionary>"##;

const PARSED_XAML: &str = r##"<ResourceDictionary
    xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
    xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
    xmlns:sys="clr-namespace:System;assembly=mscorlib">
  <sys:String x:Key="Parsed.Title">parsed-value</sys:String>
</ResourceDictionary>"##;

/// Unbox a borrowed `BaseComponent*` known to hold a `BoxedValue<String>`.
fn unbox(ptr: *mut std::ffi::c_void) -> Option<String> {
    // SAFETY: `ptr` is a live boxed value borrowed from a dictionary entry;
    // noesis_unbox_string returns a borrowed C string (or null on mismatch).
    let s = unsafe { noesis_unbox_string(ptr) };
    if s.is_null() {
        return None;
    }
    Some(unsafe { CStr::from_ptr(s) }.to_string_lossy().into_owned())
}

#[test]
fn resource_dictionary_roundtrips() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // ── Build a dictionary + key→boxed-string, look it up ────────────────
        let mut dict = ResourceDictionary::new();
        assert!(dict.is_empty(), "fresh dictionary should be empty");
        assert!(dict.add_string("K", "hello"), "add_string should succeed");
        assert_eq!(dict.len(), 1, "one base-dictionary entry after add");

        let hit = dict
            .find("K")
            .expect("find(\"K\") should be Some after add");
        assert_eq!(
            unbox(hit.as_ptr()).as_deref(),
            Some("hello"),
            "looked-up value must unbox to the stored string"
        );
        assert!(dict.contains("K"), "contains(\"K\") true");

        // Negative: a missing key resolves to None / false.
        assert!(dict.find("nope").is_none(), "missing key -> None");
        assert!(!dict.contains("nope"), "missing key -> contains false");

        // ── Merged dictionary: parent resolves a child's key ─────────────────
        let merged = ResourceDictionary::parse(MERGED_XAML)
            .expect("parse of <ResourceDictionary> should succeed");
        assert!(
            merged.contains("Merged.Greeting"),
            "merged dict should contain its own key"
        );
        assert!(
            !dict.contains("Merged.Greeting"),
            "parent must NOT see the merged key before merging"
        );
        assert!(dict.add_merged(&merged), "add_merged should succeed");
        assert!(
            dict.contains("Merged.Greeting"),
            "parent should resolve the merged key after add_merged"
        );

        // ── Parse path: a standalone dictionary keeps its parsed entry ───────
        let parsed = ResourceDictionary::parse(PARSED_XAML).expect("parse should succeed");
        let parsed_hit = parsed
            .find("Parsed.Title")
            .expect("parsed dict should contain Parsed.Title");
        assert_eq!(
            unbox(parsed_hit.as_ptr()).as_deref(),
            Some("parsed-value"),
            "parsed value must round-trip"
        );
        // Parsing non-ResourceDictionary XAML fails cleanly.
        assert!(
            ResourceDictionary::parse(
                "<Grid xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>"
            )
            .is_none(),
            "parsing a non-ResourceDictionary root must be None"
        );

        // ── Application resources: install + read back ───────────────────────
        // Noesis may pre-seed an empty application dictionary at init, so the
        // discriminating precondition is "K not yet resolvable", not absence.
        assert!(
            !application_resources_contains("K"),
            "K must not resolve through app resources before install"
        );
        set_application_resources(&dict);
        assert!(
            application_resources_present(),
            "GetApplicationResources non-null after SetApplicationResources"
        );
        assert!(
            application_resources_contains("K"),
            "installed app resources should contain K"
        );
        assert!(
            application_resources_contains("Merged.Greeting"),
            "installed app resources should resolve the merged key too"
        );
        assert!(
            !application_resources_contains("absent-key"),
            "absent key -> false through app resources"
        );

        // Clear app resources before teardown so nothing dangles past shutdown.
        // SAFETY: passing null clears the installed dictionary (documented).
        unsafe { noesis_runtime::ffi::noesis_gui_set_application_resources(std::ptr::null_mut()) };
    }

    noesis_runtime::shutdown();
}

//! Inspector toggle and query smoke test: linkage / no-crash only.
//!
//! The shipped Release dylib has the Inspector compiled out, so
//! `is_inspector_connected()` always returns `false` and `update_inspector()`
//! is a no-op. The assertions pin the expected Release behaviour, not binding
//! correctness.

#[test]
fn inspector_toggles_and_queries() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }

    noesis_runtime::disable_hot_reload();
    noesis_runtime::disable_socket_init();
    noesis_runtime::disable_inspector();

    noesis_runtime::init();

    {
        assert!(
            !noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must be false with no Inspector attached"
        );

        noesis_runtime::update_inspector();

        assert!(
            !noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must remain false after update_inspector()"
        );
    }

    noesis_runtime::shutdown();
}

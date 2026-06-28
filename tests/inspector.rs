//! TODO §17 — inspector / hot-reload toggle + query smoke test.
//!
//! The `disable_*` switches must be callable BEFORE `init` without breaking
//! initialization, and the runtime queries must behave on a test process with
//! no remote Inspector attached: `is_inspector_connected()` is false and
//! `update_inspector()` is a safe no-op. On a Release dylib these features are
//! compiled out entirely, which is consistent with the same assertions.
//!
//! One `#[test]` per file because Noesis can't be re-init'd in a process, and
//! because the pre-init toggles only have meaning before the single `init`.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test inspector -- --nocapture`

#[test]
fn inspector_toggles_and_queries() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }

    // Pre-init configuration: these must not panic and must not prevent a
    // successful init below.
    dm_noesis_runtime::disable_hot_reload();
    dm_noesis_runtime::disable_socket_init();
    dm_noesis_runtime::disable_inspector();

    dm_noesis_runtime::init();

    {
        // No remote Inspector is attached in a test (and we just disabled it),
        // so the connection query must report false.
        assert!(
            !dm_noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must be false with no Inspector attached"
        );

        // The keep-alive pump must be a safe no-op without a connection.
        dm_noesis_runtime::update_inspector();

        // Still false after pumping.
        assert!(
            !dm_noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must remain false after update_inspector()"
        );
    }

    dm_noesis_runtime::shutdown();
}

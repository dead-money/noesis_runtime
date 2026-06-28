//! TODO §17 — inspector / hot-reload toggle + query smoke test.
//!
//! HONESTY NOTE — what this test does and does NOT prove:
//!
//! This is a *linkage / no-crash* smoke test. It confirms the FFI surface
//! (`disable_hot_reload`, `disable_socket_init`, `disable_inspector`,
//! `is_inspector_connected`, `update_inspector`) links, that the pre-init
//! toggles are callable before `init` without breaking initialization, and
//! that the runtime queries don't crash. It CANNOT distinguish a correct
//! binding from a stub: the shipped `libNoesis.so` (`Bin/linux_x86_64`) is a
//! Release distributable with the Inspector compiled out, so
//! `GUI::IsInspectorConnected()` returns `false` unconditionally and
//! `UpdateInspector()` is a no-op regardless of whether the binding is real.
//! The `!is_inspector_connected()` assertions therefore pin nothing about the
//! binding's correctness — they only document the expected Release behaviour.
//! A behaviourally-discriminating assertion would require an instrumented
//! (Debug/Profile) `libNoesis.so` AND a remote Inspector attached, neither of
//! which is available in this CI; if such a dylib is ever wired in, gate a
//! stronger assertion behind it.
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
        // NOTE: on a Release dylib (Inspector compiled out) this is always
        // false whether the binding is real or a stub — it is NOT proof the
        // binding works, only that the query links and returns the expected
        // Release value with no remote attached.
        assert!(
            !dm_noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must be false with no Inspector attached"
        );

        // The keep-alive pump must link and not crash without a connection.
        // On a Release dylib it is a no-op; this only proves it doesn't panic.
        dm_noesis_runtime::update_inspector();

        // Still false after pumping (again, unconditional on a Release dylib).
        assert!(
            !dm_noesis_runtime::is_inspector_connected(),
            "is_inspector_connected() must remain false after update_inspector()"
        );
    }

    dm_noesis_runtime::shutdown();
}

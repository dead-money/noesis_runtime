//! Phase 0 smoke test: links libNoesis, runs Init / `GetBuildVersion` / Shutdown.
//!
//! Requires `NOESIS_SDK_DIR` to be set at build time and `libNoesis.so` to be
//! resolvable at run time (the build script bakes an rpath on Linux, so this
//! should work without `LD_LIBRARY_PATH`).
//!
//! Optional: set `NOESIS_LICENSE_NAME` + `NOESIS_LICENSE_KEY` to suppress the
//! trial watermark when this test runs as part of CI.

#[test]
fn init_version_shutdown() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }

    dm_noesis_runtime::init();

    let v = dm_noesis_runtime::version();
    assert!(!v.is_empty(), "version should be non-empty after init");
    eprintln!("Noesis runtime: {v}");

    dm_noesis_runtime::shutdown();
}

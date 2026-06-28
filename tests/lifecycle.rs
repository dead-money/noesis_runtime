//! Smoke test: links libNoesis, runs init / version / shutdown.
//!
//! The build script bakes an rpath on Linux; `LD_LIBRARY_PATH` is not needed.

#[test]
fn init_version_shutdown() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }

    noesis_runtime::init();

    let v = noesis_runtime::version();
    assert!(!v.is_empty(), "version should be non-empty after init");
    eprintln!("Noesis runtime: {v}");

    noesis_runtime::shutdown();
}

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=NOESIS_SDK_DIR");
    println!("cargo:rerun-if-env-changed=DOCS_RS");

    // Every cpp/*.cpp is a shim source, sorted for a deterministic build.
    // Watching the directory catches added or removed sources.
    println!("cargo:rerun-if-changed=cpp/noesis_shim.h");
    println!("cargo:rerun-if-changed=cpp");
    let mut sources: Vec<PathBuf> = std::fs::read_dir("cpp")
        .expect("read cpp/ directory")
        .map(|entry| entry.expect("read cpp/ entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("cpp"))
        .collect();
    sources.sort();
    for src in &sources {
        println!("cargo:rerun-if-changed={}", src.display());
    }

    // Without `shim` the crate is plain Rust types; nothing to compile or link.
    // docs.rs has no SDK, and rustdoc needs no native objects. Set DOCS_RS
    // locally to preview that build.
    if env::var_os("CARGO_FEATURE_SHIM").is_none() || env::var_os("DOCS_RS").is_some() {
        return;
    }

    let sdk_dir = env::var("NOESIS_SDK_DIR").unwrap_or_else(|_| {
        panic!(
            "\n\nNOESIS_SDK_DIR is not set.\n\
             Extract the Noesis Native SDK and point NOESIS_SDK_DIR at the directory \
             containing Include/ and Bin/.\n\
             See noesis_runtime/README.md for setup.\n"
        )
    });
    let sdk = PathBuf::from(&sdk_dir);
    let include = sdk.join("Include");
    assert!(
        include.is_dir(),
        "NOESIS_SDK_DIR does not contain Include/ at {}",
        include.display()
    );

    let target_os = env::var("CARGO_CFG_TARGET_OS").expect("CARGO_CFG_TARGET_OS");
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").expect("CARGO_CFG_TARGET_ARCH");
    let bin_subdir = match (target_os.as_str(), target_arch.as_str()) {
        ("linux", "x86_64") => "linux_x86_64",
        ("linux", "aarch64") => "linux_arm64",
        ("windows", "x86_64") => "windows_x86_64",
        (os, arch) => panic!(
            "Unsupported target {os}-{arch}. Add a mapping to a Noesis Bin/ subdir in build.rs."
        ),
    };
    let bin = sdk.join("Bin").join(bin_subdir);
    assert!(
        bin.is_dir(),
        "Noesis Bin/ subdir not found at {}. Does this SDK include the {bin_subdir} target?",
        bin.display()
    );

    // On Linux the shared object in Bin/ is linked directly. The Windows SDK
    // ships Noesis.dll in Bin/<subdir> and its import lib Noesis.lib in
    // Lib/<subdir>.
    let link_dir = if target_os == "windows" {
        let lib = sdk.join("Lib").join(bin_subdir);
        assert!(
            lib.is_dir(),
            "Noesis Lib/ subdir not found at {}. The Windows SDK keeps the import \
             library (Noesis.lib) here, separate from the DLL in Bin/.",
            lib.display()
        );
        lib
    } else {
        bin.clone()
    };

    println!("cargo:rustc-link-search=native={}", link_dir.display());
    println!("cargo:rustc-link-lib=dylib=Noesis");

    // Surfaces to dependents as DEP_NOESIS_LIB_DIR (via `links = "Noesis"`).
    // Points at Bin/, where the runtime .so / .dll lives, so a consumer can
    // stage it for packaging.
    println!("cargo:lib_dir={}", bin.display());

    if target_os == "linux" {
        // Tests and examples find libNoesis.so without LD_LIBRARY_PATH.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", bin.display());
    } else if target_os == "windows" {
        // No rpath on Windows: copy Noesis.dll beside the test and example
        // binaries so `cargo test` / `cargo run` work without PATH changes.
        // OUT_DIR is <target>/<profile>/build/<pkg>-<hash>/out.
        let dll = bin.join("Noesis.dll");
        let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
        if let Some(profile_dir) = out_dir.ancestors().nth(3) {
            for sub in ["", "deps", "examples"] {
                let dest = profile_dir.join(sub);
                if dest.is_dir() {
                    // Best effort; PATH still works as a fallback.
                    let _ = std::fs::copy(&dll, dest.join("Noesis.dll"));
                }
            }
        }
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .files(&sources)
        .include(&include)
        .flag_if_supported("-Wno-unused-parameter");

    // Compiles the noesis_test_* entry points guarded by #ifdef NOESIS_TEST_UTILS.
    if env::var_os("CARGO_FEATURE_TEST_UTILS").is_some() {
        build.define("NOESIS_TEST_UTILS", None);
    }

    build.compile("noesis_shim");
}

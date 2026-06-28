use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=NOESIS_SDK_DIR");
    println!("cargo:rerun-if-changed=cpp/noesis_shim.h");
    println!("cargo:rerun-if-changed=cpp/noesis_shim.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_render_device.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_view.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_events.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_classes.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_markup.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_font_provider.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_texture_provider.cpp");

    let sdk_dir = env::var("NOESIS_SDK_DIR").unwrap_or_else(|_| {
        panic!(
            "\n\nNOESIS_SDK_DIR is not set.\n\
             Extract the Noesis Native SDK and point NOESIS_SDK_DIR at the directory \
             containing Include/ and Bin/.\n\
             See dm_noesis_runtime/README.md for setup.\n"
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
        "Noesis Bin/ subdir not found at {} — does this SDK include the {bin_subdir} target?",
        bin.display()
    );

    println!("cargo:rustc-link-search=native={}", bin.display());
    println!("cargo:rustc-link-lib=dylib=Noesis");

    // Publish the resolved Bin/<platform> path to downstream crates as
    // DEP_NOESIS_LIB_DIR (per cargo's `links = "Noesis"` metadata mechanism).
    // dm_noesis_bevy reads this in its own build.rs to bake the same rpath into
    // example/test binaries; rustc-link-arg below only applies to OUR own bins.
    println!("cargo:lib_dir={}", bin.display());

    if target_os == "linux" {
        // Bake the SDK Bin/ path into rpath so dm_noesis_runtime's own integration tests
        // find libNoesis.so without LD_LIBRARY_PATH. Downstream consumers do the
        // same in their build.rs via DEP_NOESIS_LIB_DIR.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", bin.display());
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .file("cpp/noesis_shim.cpp")
        .file("cpp/noesis_render_device.cpp")
        .file("cpp/noesis_view.cpp")
        .file("cpp/noesis_events.cpp")
        .file("cpp/noesis_classes.cpp")
        .file("cpp/noesis_markup.cpp")
        .file("cpp/noesis_font_provider.cpp")
        .file("cpp/noesis_texture_provider.cpp")
        .include(&include)
        .flag_if_supported("-Wno-unused-parameter");

    // The `test-utils` Cargo feature gates the dm_noesis_test_* C entrypoints
    // (defined in noesis_render_device.cpp under #ifdef DM_NOESIS_TEST_UTILS).
    if env::var_os("CARGO_FEATURE_TEST_UTILS").is_some() {
        build.define("DM_NOESIS_TEST_UTILS", None);
    }

    build.compile("dm_noesis_shim");
}

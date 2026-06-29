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
    println!("cargo:rerun-if-changed=cpp/noesis_collections.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_reflection_meta.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_controls.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_commands.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_input.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_binding.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_plain_vm.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_resources.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_visual_state.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_markup.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_font_provider.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_texture_provider.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_brushes.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_imaging.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_drawing.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_mesh.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_animation.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_geometry.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_shapes.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_svg.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_text_inlines.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_element_tree.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_formatted_text.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_typography.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_xaml.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_integration.cpp");
    println!("cargo:rerun-if-changed=cpp/noesis_diagnostics.cpp");

    // docs.rs has no SDK to link against; the FFI surface type-checks without
    // linking, so skip the native compile and let rustdoc build. Set DOCS_RS
    // locally to preview that build.
    if env::var_os("DOCS_RS").is_some() {
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

    // What the linker resolves `Noesis` against. On Linux and Android the shared
    // object in Bin/ is linked directly. The Windows package splits the two:
    // Bin/<subdir>/Noesis.dll is the runtime library with no import lib beside
    // it, and the matching Noesis.lib ships under Lib/<subdir>.
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

    // Via cargo's `links = "Noesis"` metadata, this surfaces to downstream crates
    // as DEP_NOESIS_LIB_DIR. It points at Bin/, where the runtime library (.so /
    // .dll) lives, so a consumer can stage it for packaging regardless of where
    // the import lib was.
    println!("cargo:lib_dir={}", bin.display());

    if target_os == "linux" {
        // Bake the SDK Bin/ path into rpath so integration tests find
        // libNoesis.so without LD_LIBRARY_PATH.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", bin.display());
    } else if target_os == "windows" {
        // Windows has no rpath: the loader finds Noesis.dll next to the .exe or
        // on PATH. Copy it beside the test and example binaries so they run
        // straight from `cargo test` / `cargo run`, the parity of the rpath
        // above. OUT_DIR is <target>/<profile>/build/<pkg>-<hash>/out; the
        // profile dir three levels up holds the binaries and their deps/.
        let dll = bin.join("Noesis.dll");
        let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
        if let Some(profile_dir) = out_dir.ancestors().nth(3) {
            for sub in ["", "deps", "examples"] {
                let dest = profile_dir.join(sub);
                if dest.is_dir() {
                    // Best effort: a stale copy or a missing dir is not fatal,
                    // PATH still works as a fallback (see README).
                    let _ = std::fs::copy(&dll, dest.join("Noesis.dll"));
                }
            }
        }
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
        .file("cpp/noesis_reflection_meta.cpp")
        .file("cpp/noesis_collections.cpp")
        .file("cpp/noesis_controls.cpp")
        .file("cpp/noesis_commands.cpp")
        .file("cpp/noesis_input.cpp")
        .file("cpp/noesis_binding.cpp")
        .file("cpp/noesis_plain_vm.cpp")
        .file("cpp/noesis_resources.cpp")
        .file("cpp/noesis_visual_state.cpp")
        .file("cpp/noesis_markup.cpp")
        .file("cpp/noesis_font_provider.cpp")
        .file("cpp/noesis_texture_provider.cpp")
        .file("cpp/noesis_brushes.cpp")
        .file("cpp/noesis_imaging.cpp")
        .file("cpp/noesis_drawing.cpp")
        .file("cpp/noesis_mesh.cpp")
        .file("cpp/noesis_animation.cpp")
        .file("cpp/noesis_geometry.cpp")
        .file("cpp/noesis_shapes.cpp")
        .file("cpp/noesis_svg.cpp")
        .file("cpp/noesis_text_inlines.cpp")
        .file("cpp/noesis_element_tree.cpp")
        .file("cpp/noesis_formatted_text.cpp")
        .file("cpp/noesis_typography.cpp")
        .file("cpp/noesis_xaml.cpp")
        .file("cpp/noesis_integration.cpp")
        .file("cpp/noesis_diagnostics.cpp")
        .include(&include)
        .flag_if_supported("-Wno-unused-parameter");

    // Gates the noesis_test_* C entrypoints behind #ifdef NOESIS_TEST_UTILS
    // in noesis_render_device.cpp.
    if env::var_os("CARGO_FEATURE_TEST_UTILS").is_some() {
        build.define("NOESIS_TEST_UTILS", None);
    }

    build.compile("noesis_shim");
}

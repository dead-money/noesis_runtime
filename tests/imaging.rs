//! Integration tests for the code-built `ImageSource` / `BitmapSource` family
//! (TODO §12 "Bitmaps").
//!
//! Headless object construction + read-back: no GPU is needed for the
//! centerpiece round-trips. Every assertion reads a value BACK from the live
//! Noesis object so a stubbed constructor/setter would fail:
//!
//! - [`CroppedBitmap`] source pointer identity (proves the [`TextureSource`]
//!   object crossed the FFI as a real `BitmapSource`) and crop-rect field
//!   round-trip.
//! - [`BitmapImage`] `UriSource` string round-trip (construct from a string, read
//!   the canonicalized string back).
//! - [`DynamicTextureSource`] construction + `Resize` + pixel-size read-back.
//!
//! GPU-resolved values (`TextureSource` texture, pixel dims, dpi) read back
//! null / 0 headless and are asserted as such with a note — binding a real GPU
//! texture / resolving image dims needs a host `RenderDevice` render pass.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process):
//! all owning wrappers drop inside the inner scope before `shutdown()`.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p dm_noesis_runtime --test imaging -- --nocapture`

use std::ffi::c_void;

use dm_noesis_runtime::imaging::{
    BitmapImage, BitmapSource, CroppedBitmap, DynamicTextureSource, Int32Rect, TextureSource,
};

// A render-thread TextureRenderCallback that is never invoked headless (asserted
// by the side-effect flag staying false). Returns null (no texture).
unsafe extern "C" fn never_called(_device: *mut c_void, _user: *mut c_void) -> *mut c_void {
    // If a render pass ever ran this, `user` would be flipped to 1.
    if !_user.is_null() {
        unsafe { *_user.cast::<u32>() = 1 };
    }
    core::ptr::null_mut()
}

#[test]
fn imaging_family_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        // ── TextureSource (default ctor; no GPU) ────────────────────────────
        let texture_source = TextureSource::new();
        // No host RenderDevice-created Texture is bound, so GetTexture is null.
        // This is the documented headless outcome, not a stub artifact: the
        // pointer-identity test below proves the object itself is real.
        assert!(
            texture_source.texture().is_none(),
            "TextureSource has no texture bound headless (needs a host RenderDevice Texture)"
        );
        // BitmapSource base getters resolve only on a render pass: 0 headless.
        assert_eq!(
            texture_source.pixel_size(),
            (0, 0),
            "TextureSource pixel dims are 0 until resolved on a render pass"
        );

        // ── CroppedBitmap (centerpiece, fully headless) ─────────────────────
        let mut crop = CroppedBitmap::new();
        // Fresh crop: no source, Empty (all-zero) rect.
        assert!(crop.source().is_none(), "fresh CroppedBitmap has no source");
        assert_eq!(
            crop.source_rect(),
            Int32Rect::default(),
            "fresh CroppedBitmap rect is Empty"
        );

        // Source pointer identity: SetSource stores the exact TextureSource
        // object (AddRef, not clone). Reading it back must equal the handle's
        // canonical BaseComponent* — proves the TextureSource crossed the FFI
        // as a live BitmapSource and CroppedBitmap.SetSource is not a stub.
        assert!(crop.set_source(&texture_source), "set CroppedBitmap source");
        let got = crop.source().expect("source set after set_source");
        assert_eq!(
            got.as_ptr(),
            texture_source.raw(),
            "CroppedBitmap.Source is the exact TextureSource we assigned"
        );

        // Crop-rect field round-trip through the live Int32Rect.
        let rect = Int32Rect::new(145, 58, 33, 54);
        crop.set_source_rect(rect);
        assert_eq!(
            crop.source_rect(),
            rect,
            "CroppedBitmap SourceRect field round-trip"
        );
        // A different rect, including a negative origin, to defeat any echo.
        let rect2 = Int32Rect::new(-7, 12, 1000, 1);
        crop.set_source_rect(rect2);
        assert_eq!(crop.source_rect(), rect2, "CroppedBitmap SourceRect re-set");

        // Clearing the source via a fresh BitmapImage source proves SetSource
        // accepts any BitmapSource subclass and swaps the stored pointer.
        let bitmap_image_src = BitmapImage::from_uri("clear_check.png");
        assert!(
            crop.set_source(&bitmap_image_src),
            "swap CroppedBitmap source"
        );
        let swapped = crop.source().expect("source set after swap");
        assert_eq!(
            swapped.as_ptr(),
            bitmap_image_src.raw(),
            "CroppedBitmap.Source swapped to the BitmapImage"
        );
        assert_ne!(
            swapped.as_ptr(),
            texture_source.raw(),
            "swapped source is no longer the TextureSource"
        );

        // ── BitmapImage UriSource string round-trip ─────────────────────────
        let mut image = BitmapImage::new();
        assert_eq!(
            image.uri_source(),
            "",
            "default BitmapImage has empty UriSource"
        );
        assert!(image.set_uri_source("Images/icon.png"), "set UriSource");
        assert_eq!(
            image.uri_source(),
            "Images/icon.png",
            "BitmapImage UriSource round-trip"
        );

        // Constructed-from-uri form reads the same string back.
        let image2 = BitmapImage::from_uri("Folder/aladin.png");
        assert_eq!(
            image2.uri_source(),
            "Folder/aladin.png",
            "BitmapImage::from_uri UriSource round-trip"
        );
        // Pixel dims are render-pass-resolved: 0 headless.
        assert_eq!(
            image2.pixel_size(),
            (0, 0),
            "BitmapImage pixel dims are 0 until a texture provider resolves it"
        );

        // ── DynamicTextureSource construction + Resize + pixel-size ─────────
        // `flag` would be flipped to 1 only if the render-thread callback ran;
        // headless it must stay 0 (the callback never fires without a pass).
        let mut flag: u32 = 0;
        let mut dyn_src = unsafe {
            DynamicTextureSource::new(64, 48, never_called, (&raw mut flag).cast::<c_void>())
        };
        assert_eq!(
            dyn_src.pixel_size(),
            (64, 48),
            "DynamicTextureSource reports its construction dims"
        );
        assert!(dyn_src.resize(128, 96), "DynamicTextureSource resize");
        assert_eq!(
            dyn_src.pixel_size(),
            (128, 96),
            "DynamicTextureSource pixel dims reflect Resize"
        );
        assert_eq!(
            flag, 0,
            "render-thread callback does not fire without a render pass"
        );
    }

    dm_noesis_runtime::shutdown();
}

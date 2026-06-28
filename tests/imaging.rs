//! Code-built `ImageSource` / `BitmapSource` family: headless construction +
//! read-back. GPU-resolved values (texture, pixel dims) return null/0 headless
//! and are asserted as such.
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p noesis_runtime --test imaging -- --nocapture`

use std::ffi::c_void;

use noesis_runtime::imaging::{
    BitmapImage, BitmapSource, CroppedBitmap, DynamicTextureSource, Int32Rect, TextureSource,
};

// render-thread callback that must not fire headless (flag stays 0 = proof)
unsafe extern "C" fn never_called(_device: *mut c_void, _user: *mut c_void) -> *mut c_void {
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
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let texture_source = TextureSource::new();
        // null headless — no RenderDevice Texture bound; documented outcome, not a stub
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

        let mut crop = CroppedBitmap::new();
        assert!(crop.source().is_none(), "fresh CroppedBitmap has no source");
        assert_eq!(
            crop.source_rect(),
            Int32Rect::default(),
            "fresh CroppedBitmap rect is Empty"
        );

        // SetSource AddRefs the exact object; reading back proves identity, not clone
        assert!(crop.set_source(&texture_source), "set CroppedBitmap source");
        let got = crop.source().expect("source set after set_source");
        assert_eq!(
            got.as_ptr(),
            texture_source.raw(),
            "CroppedBitmap.Source is the exact TextureSource we assigned"
        );

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

        // SetSource accepts any BitmapSource subclass and swaps the pointer
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

        // flag stays 0 headless — render-thread callback never fires without a pass
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

    noesis_runtime::shutdown();
}

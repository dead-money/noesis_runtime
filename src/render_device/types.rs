//! Mirrors of the public Noesis types in `Include/NsRender/RenderDevice.h`.
//!
//! These types cross the FFI boundary into our C++ shim and on into Noesis,
//! so any drift from the Noesis-side declarations is a hard ABI bug. Layout
//! is verified at compile time at the bottom of this file.
//!
//! ABI notes:
//! - Unscoped C++ `enum`s default to `int` (4 bytes on Linux x86-64).
//!   `#[repr(C)]` Rust enums match that.
//! - `Shader`, `SamplerState`, and `RenderState` are stored as a single
//!   `uint8_t` in `Batch`. We mirror them as `#[repr(transparent)]` newtypes
//!   over `u8` rather than Rust enums; that preserves the size *and* keeps
//!   any incoming byte value valid (no UB if Noesis adds variants we haven't
//!   mirrored yet).
//! - Bitfield ordering follows the LSB-first convention used by GCC and
//!   Clang on x86-64 / aarch64 / wasm targets.

#![allow(clippy::enum_variant_names)] // mirroring Noesis-side names verbatim

use core::mem::{align_of, size_of};
use std::os::raw::c_void;

// ────────────────────────────────────────────────────────────────────────────
// Texture formats: `Noesis::TextureFormat::Enum`
// ────────────────────────────────────────────────────────────────────────────

/// Pixel layout of a texture you create for the render device.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TextureFormat {
    /// Four-component, 8 bits per channel including alpha.
    Rgba8 = 0,
    /// Four-component, 8 bits per color channel + 8 bits unused.
    Rgbx8 = 1,
    /// Single-component, 8 bits red.
    R8 = 2,
}

/// Number of [`TextureFormat`] variants.
pub const TEXTURE_FORMAT_COUNT: usize = 3;

// ────────────────────────────────────────────────────────────────────────────
// Sampler state: `Noesis::WrapMode::Enum`, `MinMagFilter::Enum`,
// `MipFilter::Enum`, `Noesis::SamplerState`
// ────────────────────────────────────────────────────────────────────────────

/// How a sampler treats texture coordinates outside the `[0, 1]` range.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WrapMode {
    /// Clamp UV between 0.0 and 1.0.
    ClampToEdge = 0,
    /// Out-of-range coordinates return transparent zero.
    ClampToZero = 1,
    /// Tile the texture across the full coordinate range.
    Repeat = 2,
    /// Repeat with horizontal flip.
    MirrorU = 3,
    /// Repeat with vertical flip.
    MirrorV = 4,
    /// Combination of `MirrorU` and `MirrorV`.
    Mirror = 5,
}

/// Number of [`WrapMode`] variants.
pub const WRAP_MODE_COUNT: usize = 6;

/// Filtering applied when a texture is minified or magnified.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MinMagFilter {
    /// Nearest-texel sampling.
    Nearest = 0,
    /// Bilinear sampling.
    Linear = 1,
}

/// Number of [`MinMagFilter`] variants.
pub const MIN_MAG_FILTER_COUNT: usize = 2;

/// Filtering applied between mipmap levels.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MipFilter {
    /// Sample from mipmap level 0 only.
    Disabled = 0,
    /// Sample the nearest mipmap level.
    Nearest = 1,
    /// Linearly blend between mipmap levels (trilinear).
    Linear = 2,
}

/// Number of [`MipFilter`] variants.
pub const MIP_FILTER_COUNT: usize = 3;

/// Mirror of `Noesis::SamplerState`.
///
/// Packed bitfield in a single byte: bits 0-2 wrap mode, bit 3 min/mag
/// filter, bits 4-5 mip filter, bits 6-7 unused.
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct SamplerState(pub u8);

impl SamplerState {
    /// Pack the three sampler fields into the canonical byte layout.
    #[must_use]
    pub const fn new(wrap: WrapMode, minmag: MinMagFilter, mip: MipFilter) -> Self {
        let bits = (wrap as u8 & 0b111) | ((minmag as u8 & 0b1) << 3) | ((mip as u8 & 0b11) << 4);
        Self(bits)
    }

    /// Raw 3-bit wrap-mode field. Matches `WrapMode as u8` for valid values.
    #[must_use]
    pub const fn wrap_mode_raw(self) -> u8 {
        self.0 & 0b111
    }

    /// Raw 1-bit min/mag filter field. Matches `MinMagFilter as u8`.
    #[must_use]
    pub const fn minmag_filter_raw(self) -> u8 {
        (self.0 >> 3) & 0b1
    }

    /// Raw 2-bit mip filter field. Matches `MipFilter as u8`.
    #[must_use]
    pub const fn mip_filter_raw(self) -> u8 {
        (self.0 >> 4) & 0b11
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Blend & stencil: `Noesis::BlendMode::Enum`, `Noesis::StencilMode::Enum`,
// `Noesis::RenderState`
// ────────────────────────────────────────────────────────────────────────────

/// Blend equation a batch uses to combine its output with the target.
///
/// The formulas are written `color / alpha`, with `s` the source and `d` the
/// destination; all assume premultiplied alpha.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BlendMode {
    /// `cs / as`
    Src = 0,
    /// `cs + cd*(1-as) / as + ad*(1-as)`. Standard premultiplied alpha.
    SrcOver = 1,
    /// `cs * cd + cd*(1-as) / as + ad*(1-as)`.
    SrcOverMultiply = 2,
    /// `cs + cd*(1-cs) / as + ad*(1-as)`.
    SrcOverScreen = 3,
    /// Additive: `cs + cs / as + ad*(1-as)`.
    SrcOverAdditive = 4,
    /// Dual-source blending; needed for SDF subpixel rendering.
    SrcOverDual = 5,
}

/// Number of [`BlendMode`] variants.
pub const BLEND_MODE_COUNT: usize = 6;

/// Stencil (and, in some variants, depth) test a batch applies.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StencilMode {
    /// No stencil test.
    Disabled = 0,
    /// Pass where stencil equals `stencil_ref`; leave the buffer unchanged.
    EqualKeep = 1,
    /// Pass where stencil equals `stencil_ref`; increment the buffer.
    EqualIncr = 2,
    /// Pass where stencil equals `stencil_ref`; decrement the buffer.
    EqualDecr = 3,
    /// Set stencil data to 0.
    Clear = 4,
    /// Stencil disabled, depth test enabled.
    DisabledZTest = 5,
    /// Stencil and depth test both enabled.
    EqualKeepZTest = 6,
}

/// Number of [`StencilMode`] variants.
pub const STENCIL_MODE_COUNT: usize = 7;

/// Mirror of `Noesis::RenderState`.
///
/// Packed bitfield in a single byte: bit 0 colorEnable, bits 1-3 blendMode,
/// bits 4-6 stencilMode, bit 7 wireframe.
#[repr(transparent)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct RenderState(pub u8);

impl RenderState {
    /// Pack the four render-state fields into the canonical byte layout.
    #[must_use]
    pub const fn new(
        color_enable: bool,
        blend: BlendMode,
        stencil: StencilMode,
        wireframe: bool,
    ) -> Self {
        let bits = (color_enable as u8 & 0b1)
            | ((blend as u8 & 0b111) << 1)
            | ((stencil as u8 & 0b111) << 4)
            | ((wireframe as u8 & 0b1) << 7);
        Self(bits)
    }

    /// Whether color writes are enabled for the batch.
    #[must_use]
    pub const fn color_enable(self) -> bool {
        (self.0 & 0b1) != 0
    }

    /// Raw 3-bit blend-mode field. Matches `BlendMode as u8`.
    #[must_use]
    pub const fn blend_mode_raw(self) -> u8 {
        (self.0 >> 1) & 0b111
    }

    /// Raw 3-bit stencil-mode field. Matches `StencilMode as u8`.
    #[must_use]
    pub const fn stencil_mode_raw(self) -> u8 {
        (self.0 >> 4) & 0b111
    }

    /// Whether the batch renders in wireframe.
    #[must_use]
    pub const fn wireframe(self) -> bool {
        (self.0 >> 7) != 0
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Shader / vertex / format taxonomy: `Noesis::Shader` and nested types
// ────────────────────────────────────────────────────────────────────────────

/// Mirror of `Noesis::Shader`.
///
/// The C++ side is a struct with a single `uint8_t v` field. We use a
/// transparent newtype rather than a Rust enum so any incoming byte stays
/// valid. Noesis is allowed to extend the variant set in a point release
/// without us reading uninitialised discriminants.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shader(pub u8);

#[allow(non_upper_case_globals)]
impl Shader {
    // Debug
    /// Debug shader: flat per-vertex RGBA color.
    pub const RGBA: Self = Self(0);
    /// Stencil-only rendering for masks.
    pub const MASK: Self = Self(1);
    /// Clear render target.
    pub const CLEAR: Self = Self(2);

    // Path (no PPAA)
    /// Path fill, solid color.
    pub const PATH_SOLID: Self = Self(3);
    /// Path fill, linear gradient.
    pub const PATH_LINEAR: Self = Self(4);
    /// Path fill, radial gradient.
    pub const PATH_RADIAL: Self = Self(5);
    /// Path fill, texture pattern.
    pub const PATH_PATTERN: Self = Self(6);
    /// Path fill, texture pattern, clamp wrap.
    pub const PATH_PATTERN_CLAMP: Self = Self(7);
    /// Path fill, texture pattern, repeat wrap.
    pub const PATH_PATTERN_REPEAT: Self = Self(8);
    /// Path fill, texture pattern, mirror-U wrap.
    pub const PATH_PATTERN_MIRROR_U: Self = Self(9);
    /// Path fill, texture pattern, mirror-V wrap.
    pub const PATH_PATTERN_MIRROR_V: Self = Self(10);
    /// Path fill, texture pattern, mirror wrap.
    pub const PATH_PATTERN_MIRROR: Self = Self(11);

    // Path (with PPAA)
    /// Antialiased path fill, solid color.
    pub const PATH_AA_SOLID: Self = Self(12);
    /// Antialiased path fill, linear gradient.
    pub const PATH_AA_LINEAR: Self = Self(13);
    /// Antialiased path fill, radial gradient.
    pub const PATH_AA_RADIAL: Self = Self(14);
    /// Antialiased path fill, texture pattern.
    pub const PATH_AA_PATTERN: Self = Self(15);
    /// Antialiased path fill, texture pattern, clamp wrap.
    pub const PATH_AA_PATTERN_CLAMP: Self = Self(16);
    /// Antialiased path fill, texture pattern, repeat wrap.
    pub const PATH_AA_PATTERN_REPEAT: Self = Self(17);
    /// Antialiased path fill, texture pattern, mirror-U wrap.
    pub const PATH_AA_PATTERN_MIRROR_U: Self = Self(18);
    /// Antialiased path fill, texture pattern, mirror-V wrap.
    pub const PATH_AA_PATTERN_MIRROR_V: Self = Self(19);
    /// Antialiased path fill, texture pattern, mirror wrap.
    pub const PATH_AA_PATTERN_MIRROR: Self = Self(20);

    // SDF (text)
    /// SDF text glyphs, solid color.
    pub const SDF_SOLID: Self = Self(21);
    /// SDF text glyphs, linear gradient.
    pub const SDF_LINEAR: Self = Self(22);
    /// SDF text glyphs, radial gradient.
    pub const SDF_RADIAL: Self = Self(23);
    /// SDF text glyphs, texture pattern.
    pub const SDF_PATTERN: Self = Self(24);
    /// SDF text glyphs, texture pattern, clamp wrap.
    pub const SDF_PATTERN_CLAMP: Self = Self(25);
    /// SDF text glyphs, texture pattern, repeat wrap.
    pub const SDF_PATTERN_REPEAT: Self = Self(26);
    /// SDF text glyphs, texture pattern, mirror-U wrap.
    pub const SDF_PATTERN_MIRROR_U: Self = Self(27);
    /// SDF text glyphs, texture pattern, mirror-V wrap.
    pub const SDF_PATTERN_MIRROR_V: Self = Self(28);
    /// SDF text glyphs, texture pattern, mirror wrap.
    pub const SDF_PATTERN_MIRROR: Self = Self(29);

    // SDF LCD (subpixel text; needs DeviceCaps::subpixel_rendering)
    /// LCD subpixel SDF text, solid color.
    pub const SDF_LCD_SOLID: Self = Self(30);
    /// LCD subpixel SDF text, linear gradient.
    pub const SDF_LCD_LINEAR: Self = Self(31);
    /// LCD subpixel SDF text, radial gradient.
    pub const SDF_LCD_RADIAL: Self = Self(32);
    /// LCD subpixel SDF text, texture pattern.
    pub const SDF_LCD_PATTERN: Self = Self(33);
    /// LCD subpixel SDF text, texture pattern, clamp wrap.
    pub const SDF_LCD_PATTERN_CLAMP: Self = Self(34);
    /// LCD subpixel SDF text, texture pattern, repeat wrap.
    pub const SDF_LCD_PATTERN_REPEAT: Self = Self(35);
    /// LCD subpixel SDF text, texture pattern, mirror-U wrap.
    pub const SDF_LCD_PATTERN_MIRROR_U: Self = Self(36);
    /// LCD subpixel SDF text, texture pattern, mirror-V wrap.
    pub const SDF_LCD_PATTERN_MIRROR_V: Self = Self(37);
    /// LCD subpixel SDF text, texture pattern, mirror wrap.
    pub const SDF_LCD_PATTERN_MIRROR: Self = Self(38);

    // Opacity (offscreen)
    /// Offscreen opacity-group composite, solid color.
    pub const OPACITY_SOLID: Self = Self(39);
    /// Offscreen opacity-group composite, linear gradient.
    pub const OPACITY_LINEAR: Self = Self(40);
    /// Offscreen opacity-group composite, radial gradient.
    pub const OPACITY_RADIAL: Self = Self(41);
    /// Offscreen opacity-group composite, texture pattern.
    pub const OPACITY_PATTERN: Self = Self(42);
    /// Offscreen opacity-group composite, texture pattern, clamp wrap.
    pub const OPACITY_PATTERN_CLAMP: Self = Self(43);
    /// Offscreen opacity-group composite, texture pattern, repeat wrap.
    pub const OPACITY_PATTERN_REPEAT: Self = Self(44);
    /// Offscreen opacity-group composite, texture pattern, mirror-U wrap.
    pub const OPACITY_PATTERN_MIRROR_U: Self = Self(45);
    /// Offscreen opacity-group composite, texture pattern, mirror-V wrap.
    pub const OPACITY_PATTERN_MIRROR_V: Self = Self(46);
    /// Offscreen opacity-group composite, texture pattern, mirror wrap.
    pub const OPACITY_PATTERN_MIRROR: Self = Self(47);

    // Misc
    /// Upsample pass (blur/effect resolve).
    pub const UPSAMPLE: Self = Self(48);
    /// Downsample pass (blur/effect resolve).
    pub const DOWNSAMPLE: Self = Self(49);
    /// Drop-shadow generation.
    pub const SHADOW: Self = Self(50);
    /// Gaussian blur.
    pub const BLUR: Self = Self(51);
    /// User-supplied custom pixel effect.
    pub const CUSTOM_EFFECT: Self = Self(52);
}

/// Number of [`Shader`] values.
pub const SHADER_COUNT: usize = 53;

/// Mirror of `Noesis::Shader::Vertex::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VertexShader {
    /// Position only.
    Pos = 0,
    /// Position + color.
    PosColor = 1,
    /// Position + texcoord 0.
    PosTex0 = 2,
    /// Position + texcoord 0 + rect.
    PosTex0Rect = 3,
    /// Position + texcoord 0 + rect + tile.
    PosTex0RectTile = 4,
    /// Position + color + coverage.
    PosColorCoverage = 5,
    /// Position + texcoord 0 + coverage.
    PosTex0Coverage = 6,
    /// Position + texcoord 0 + coverage + rect.
    PosTex0CoverageRect = 7,
    /// Position + texcoord 0 + coverage + rect + tile.
    PosTex0CoverageRectTile = 8,
    /// Position + color + texcoord 1 (SDF).
    PosColorTex1Sdf = 9,
    /// Position + texcoord 0 + texcoord 1 (SDF).
    PosTex0Tex1Sdf = 10,
    /// Position + texcoord 0 + texcoord 1 + rect (SDF).
    PosTex0Tex1RectSdf = 11,
    /// Position + texcoord 0 + texcoord 1 + rect + tile (SDF).
    PosTex0Tex1RectTileSdf = 12,
    /// Position + color + texcoord 1.
    PosColorTex1 = 13,
    /// Position + texcoord 0 + texcoord 1.
    PosTex0Tex1 = 14,
    /// Position + texcoord 0 + texcoord 1 + rect.
    PosTex0Tex1Rect = 15,
    /// Position + texcoord 0 + texcoord 1 + rect + tile.
    PosTex0Tex1RectTile = 16,
    /// Position + color + texcoord 0 + texcoord 1.
    PosColorTex0Tex1 = 17,
    /// Position + texcoord 0 + texcoord 1 (downsample).
    PosTex0Tex1Downsample = 18,
    /// Position + color + texcoord 1 + rect.
    PosColorTex1Rect = 19,
    /// Position + color + texcoord 0 + rect + image position.
    PosColorTex0RectImagePos = 20,
}

/// Number of [`VertexShader`] variants.
pub const VERTEX_SHADER_COUNT: usize = 21;

/// Mirror of `Noesis::Shader::Vertex::Format::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VertexFormat {
    /// Position only.
    Pos = 0,
    /// Position + color.
    PosColor = 1,
    /// Position + texcoord 0.
    PosTex0 = 2,
    /// Position + texcoord 0 + rect.
    PosTex0Rect = 3,
    /// Position + texcoord 0 + rect + tile.
    PosTex0RectTile = 4,
    /// Position + color + coverage.
    PosColorCoverage = 5,
    /// Position + texcoord 0 + coverage.
    PosTex0Coverage = 6,
    /// Position + texcoord 0 + coverage + rect.
    PosTex0CoverageRect = 7,
    /// Position + texcoord 0 + coverage + rect + tile.
    PosTex0CoverageRectTile = 8,
    /// Position + color + texcoord 1.
    PosColorTex1 = 9,
    /// Position + texcoord 0 + texcoord 1.
    PosTex0Tex1 = 10,
    /// Position + texcoord 0 + texcoord 1 + rect.
    PosTex0Tex1Rect = 11,
    /// Position + texcoord 0 + texcoord 1 + rect + tile.
    PosTex0Tex1RectTile = 12,
    /// Position + color + texcoord 0 + texcoord 1.
    PosColorTex0Tex1 = 13,
    /// Position + color + texcoord 1 + rect.
    PosColorTex1Rect = 14,
    /// Position + color + texcoord 0 + rect + image position.
    PosColorTex0RectImagePos = 15,
}

/// Number of [`VertexFormat`] variants.
pub const VERTEX_FORMAT_COUNT: usize = 16;

/// Mirror of `Noesis::Shader::Vertex::Format::Attr::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VertexAttr {
    /// Position (xy), linear.
    Pos = 0,
    /// sRGB color (rgba), nointerpolation.
    Color = 1,
    /// `TexCoord0` (uv), linear.
    Tex0 = 2,
    /// `TexCoord1` (uv), linear.
    Tex1 = 3,
    /// Coverage (alpha), linear.
    Coverage = 4,
    /// Rect (x0, y0, x1, y1), nointerpolation.
    Rect = 5,
    /// Tile rect (x, y, w, h), nointerpolation.
    Tile = 6,
    /// Position (xy) + scale (zw), linear.
    ImagePos = 7,
}

/// Number of [`VertexAttr`] variants.
pub const VERTEX_ATTR_COUNT: usize = 8;

/// Mirror of `Noesis::Shader::Vertex::Format::Attr::Type::Enum`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VertexAttrType {
    /// One 32-bit float.
    Float = 0,
    /// Two 32-bit floats.
    Float2 = 1,
    /// Four 32-bit floats.
    Float4 = 2,
    /// Four 8-bit unsigned normalized integers.
    UByte4Norm = 3,
    /// Four 16-bit unsigned normalized integers.
    UShort4Norm = 4,
}

/// Number of [`VertexAttrType`] variants.
pub const VERTEX_ATTR_TYPE_COUNT: usize = 5;

// ────────────────────────────────────────────────────────────────────────────
// Static lookup tables: mirrors of the `static constexpr const uint8_t` arrays
// declared inline in `RenderDevice.h`. Length-checked at compile time against
// the corresponding `*_COUNT` constants.
// ────────────────────────────────────────────────────────────────────────────

/// Vertex-shader index for each `Shader` value. Index with `shader.0 as usize`.
pub const VERTEX_FOR_SHADER: [u8; SHADER_COUNT] = [
    0, 0, 0, 1, 2, 2, 2, 3, 4, 4, 4, 4, 5, 6, 6, 6, 7, 8, 8, 8, 8, 9, 10, 10, 10, 11, 12, 12, 12,
    12, 9, 10, 10, 10, 11, 12, 12, 12, 12, 13, 14, 14, 14, 15, 16, 16, 16, 16, 17, 18, 19, 13, 20,
];

/// Vertex-format index for each `VertexShader` value.
pub const FORMAT_FOR_VERTEX: [u8; VERTEX_SHADER_COUNT] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 9, 10, 11, 12, 13, 10, 14, 15,
];

/// Total vertex stride (bytes) for each `VertexFormat`.
pub const SIZE_FOR_FORMAT: [u8; VERTEX_FORMAT_COUNT] = [
    8, 12, 16, 24, 40, 16, 20, 28, 44, 20, 24, 32, 48, 28, 28, 44,
];

/// Bitmask of `VertexAttr` values present in each `VertexFormat`.
pub const ATTRIBUTES_FOR_FORMAT: [u8; VERTEX_FORMAT_COUNT] = [
    1, 3, 5, 37, 101, 19, 21, 53, 117, 11, 13, 45, 109, 15, 43, 167,
];

/// `VertexAttrType` index for each `VertexAttr`.
pub const TYPE_FOR_ATTR: [u8; VERTEX_ATTR_COUNT] = [1, 3, 1, 1, 0, 4, 2, 2];

/// Size in bytes for each `VertexAttrType`.
pub const SIZE_FOR_TYPE: [u8; VERTEX_ATTR_TYPE_COUNT] = [4, 8, 16, 4, 8];

// ────────────────────────────────────────────────────────────────────────────
// Frame primitives: `DeviceCaps`, `Tile`, `UniformData`
// ────────────────────────────────────────────────────────────────────────────

/// Mirror of `Noesis::DeviceCaps`.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct DeviceCaps {
    /// Offset in pixel units from top-left corner to center of pixel.
    pub center_pixel_offset: f32,
    /// When true, internal textures + offscreens use sRGB; vertex colors are
    /// expected in sRGB, target writes are linear.
    pub linear_rendering: bool,
    /// Device supports LCD subpixel rendering (needs dual-source blending).
    pub subpixel_rendering: bool,
    /// Clip-space depth range is [0, 1] rather than [-1, 1].
    pub depth_range_zero_to_one: bool,
    /// Clip-space Y is inverted (top = -1, bottom = +1).
    pub clip_space_y_inverted: bool,
}

impl Default for DeviceCaps {
    fn default() -> Self {
        // Values must match the C++ in-class member initializers
        // (depth_range_zero_to_one defaults to true, not false).
        Self {
            center_pixel_offset: 0.0,
            linear_rendering: false,
            subpixel_rendering: false,
            depth_range_zero_to_one: true,
            clip_space_y_inverted: false,
        }
    }
}

/// Mirror of `Noesis::Tile`: a region of the render target with origin at
/// the lower-left corner.
#[repr(C)]
#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, Hash)]
pub struct Tile {
    /// Left edge in pixels, measured from the target's lower-left origin.
    pub x: u32,
    /// Bottom edge in pixels, measured from the target's lower-left origin.
    pub y: u32,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Mirror of `Noesis::UniformData`: a span of dwords for uniform-buffer
/// updates, plus a content hash so the device can skip redundant uploads.
///
/// `values` points into Noesis-owned memory that lives at least until the
/// `DrawBatch` call returns.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct UniformData {
    /// Pointer to the dword array (may be null when `num_dwords == 0`).
    pub values: *const c_void,
    /// Number of 4-byte dwords at `values`.
    pub num_dwords: u32,
    /// Content hash; equal hashes guarantee equal contents.
    pub hash: u32,
}

impl Default for UniformData {
    fn default() -> Self {
        Self {
            values: core::ptr::null(),
            num_dwords: 0,
            hash: 0,
        }
    }
}

impl UniformData {
    /// Borrow the uniform bytes as a slice. Returns an empty slice when
    /// `num_dwords == 0` or `values` is null. Tightly packed; length is
    /// `num_dwords * 4` bytes.
    ///
    /// Quarantines the dereference so `unsafe_code = forbid` crates (e.g.
    /// `noesis_bevy`) can consume Noesis uniforms without opting in
    /// themselves.
    ///
    /// # Safety contract relied on
    ///
    /// Noesis guarantees `values` is valid for `num_dwords * 4` bytes for the
    /// duration of the `DrawBatch` call this `UniformData` came from. Callers
    /// must not retain the returned slice past the `draw_batch` callback
    /// where the parent [`Batch`] was passed.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        if self.num_dwords == 0 || self.values.is_null() {
            return &[];
        }
        // SAFETY: see method-level safety contract.
        unsafe {
            core::slice::from_raw_parts(self.values.cast::<u8>(), self.num_dwords as usize * 4)
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Opaque C++ resource handles + the `Batch` struct passed to `DrawBatch`
// ────────────────────────────────────────────────────────────────────────────

/// Opaque handle to a `Noesis::Texture` instance.
///
/// Your device's `create_texture` callback produces these; they then come back
/// to you in `update_texture`, in `Batch.pattern`/`ramps`/`image`/`glyphs`/`shadow`,
/// and the other texture-bearing callbacks. Only ever hold it behind a
/// `*mut Texture`: the underlying class is owned by the C++ shim, so you can
/// never construct or dereference one from Rust.
#[repr(C)]
pub struct Texture {
    _opaque: [u8; 0],
    /// Force `!Send + !Sync` and prevent direct construction.
    _marker: core::marker::PhantomData<(*mut u8, core::marker::PhantomPinned)>,
}

/// Mirror of `Noesis::Batch`: a single indexed-triangle draw call.
///
/// Hot-path payload to `RenderDevice::draw_batch`. Texture pointers are null
/// when unused. Vertex data starts at the most recent `map_vertices()` return
/// plus `vertex_offset` bytes; indices are 16 bits each and start at the most
/// recent `map_indices()` return plus `start_index * 2` bytes.
///
/// The `*_handle()` helpers translate the opaque `*mut Texture` pointers
/// (which come from Noesis and reference `RustTexture` instances inside the
/// C++ shim) into the `TextureHandle` values originally returned by
/// `RenderDevice::create_texture`. Safe to call: the shim getter does a
/// null check and reads a stored handle field; no further dereferencing.
#[repr(C)]
#[derive(Debug)]
pub struct Batch {
    /// Built-in (or custom) shader to bind for this draw.
    pub shader: Shader,
    /// Packed color-write, blend, stencil, and wireframe flags.
    pub render_state: RenderState,
    /// Reference value for the stencil test selected by `render_state`.
    pub stencil_ref: u8,
    /// When `true`, the batch renders both left and right eye images in one
    /// pass (single-pass stereo).
    pub single_pass_stereo: bool,

    /// Byte offset into the most recent `map_vertices()` buffer.
    pub vertex_offset: u32,
    /// Number of vertices in the batch.
    pub num_vertices: u32,
    /// First index, in indices (multiply by 2 for the byte offset into the
    /// most recent `map_indices()` buffer).
    pub start_index: u32,
    /// Number of 16-bit indices to draw.
    pub num_indices: u32,

    /// Pattern (brush) texture; null when unused. See
    /// [`pattern_handle`](Self::pattern_handle).
    pub pattern: *mut Texture,
    /// Gradient ramps texture; null when unused.
    pub ramps: *mut Texture,
    /// Image / offscreen input texture; null when unused.
    pub image: *mut Texture,
    /// SDF glyph atlas; null when unused.
    pub glyphs: *mut Texture,
    /// Shadow intermediate texture; null when unused.
    pub shadow: *mut Texture,

    /// Sampler state for `pattern`.
    pub pattern_sampler: SamplerState,
    /// Sampler state for `ramps`.
    pub ramps_sampler: SamplerState,
    /// Sampler state for `image`.
    pub image_sampler: SamplerState,
    /// Sampler state for `glyphs`.
    pub glyphs_sampler: SamplerState,
    /// Sampler state for `shadow`.
    pub shadow_sampler: SamplerState,

    /// Vertex-shader uniform buffers, one per slot. `num_dwords == 0` marks
    /// an unused slot.
    pub vertex_uniforms: [UniformData; 2],
    /// Pixel-shader uniform buffers, one per slot.
    pub pixel_uniforms: [UniformData; 2],

    /// Custom pixel-shader pointer used by custom effects (set on the Noesis
    /// side via `ShaderEffect::SetPixelShader` or `BrushShader::SetPixelShader`).
    /// Null unless the batch uses a custom effect.
    pub pixel_shader: *mut c_void,
}

impl Batch {
    /// Translate the pattern texture pointer into the `TextureHandle` the
    /// Rust-side device returned from `create_texture`. `None` when unused.
    #[must_use]
    pub fn pattern_handle(&self) -> Option<crate::render_device::TextureHandle> {
        handle_from_texture_ptr(self.pattern)
    }

    /// As [`pattern_handle`](Self::pattern_handle) but for the ramps texture
    /// (gradients).
    #[must_use]
    pub fn ramps_handle(&self) -> Option<crate::render_device::TextureHandle> {
        handle_from_texture_ptr(self.ramps)
    }

    /// As [`pattern_handle`](Self::pattern_handle) but for the image texture
    /// (offscreen opacity / effect input).
    #[must_use]
    pub fn image_handle(&self) -> Option<crate::render_device::TextureHandle> {
        handle_from_texture_ptr(self.image)
    }

    /// As [`pattern_handle`](Self::pattern_handle) but for the glyph atlas
    /// (SDF text).
    #[must_use]
    pub fn glyphs_handle(&self) -> Option<crate::render_device::TextureHandle> {
        handle_from_texture_ptr(self.glyphs)
    }

    /// As [`pattern_handle`](Self::pattern_handle) but for the shadow
    /// intermediate (shadow effect).
    #[must_use]
    pub fn shadow_handle(&self) -> Option<crate::render_device::TextureHandle> {
        handle_from_texture_ptr(self.shadow)
    }
}

/// Safely translate a Noesis-owned `Texture*` into its Rust-side handle.
/// The shim getter is null-safe and performs a single member read; the
/// pointer either came from `RenderDevice::create_texture` (and is live for
/// the `draw_batch` call) or is null.
fn handle_from_texture_ptr(ptr: *mut Texture) -> Option<crate::render_device::TextureHandle> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: Noesis owns Batch.* pointers and keeps them alive for the
    // duration of the `draw_batch` call. The shim getter null-checks and
    // reads `RustTexture::mHandle` without further dereferencing.
    let raw = unsafe {
        crate::render_device::ffi::noesis_texture_get_handle(ptr.cast::<core::ffi::c_void>())
    };
    core::num::NonZeroU64::new(raw).map(crate::render_device::TextureHandle)
}

// ────────────────────────────────────────────────────────────────────────────
// Layout assertions: these fire at compile time if any mirror drifts from
// the Noesis-side layout. Sizes for the byte-packed types are checked
// explicitly; the `#[repr(C)]` enums get their size from the platform's int
// representation, which already matches Noesis's unscoped enum default.
// ────────────────────────────────────────────────────────────────────────────

const _: () = assert!(size_of::<Shader>() == 1);
const _: () = assert!(align_of::<Shader>() == 1);

const _: () = assert!(size_of::<SamplerState>() == 1);
const _: () = assert!(align_of::<SamplerState>() == 1);

const _: () = assert!(size_of::<RenderState>() == 1);
const _: () = assert!(align_of::<RenderState>() == 1);

const _: () = assert!(size_of::<DeviceCaps>() == 8);
const _: () = assert!(align_of::<DeviceCaps>() == 4);

const _: () = assert!(size_of::<Tile>() == 16);
const _: () = assert!(align_of::<Tile>() == 4);

#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<UniformData>() == 16);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(align_of::<UniformData>() == 8);

#[cfg(target_pointer_width = "32")]
const _: () = assert!(size_of::<UniformData>() == 12);
#[cfg(target_pointer_width = "32")]
const _: () = assert!(align_of::<UniformData>() == 4);

// Batch layout (64-bit Itanium ABI):
//   offset  0  shader              1
//   offset  1  render_state        1
//   offset  2  stencil_ref         1
//   offset  3  single_pass_stereo  1
//   offset  4  vertex_offset       4
//   offset  8  num_vertices        4
//   offset 12  start_index         4
//   offset 16  num_indices         4
//   offset 20  -- 4 bytes padding to 8-align textures
//   offset 24  pattern .. shadow   5*8 = 40
//   offset 64  pattern_sampler ..  5*1 = 5
//   offset 69  -- 3 bytes padding to 8-align uniforms
//   offset 72  vertex_uniforms[2]  2*16 = 32
//   offset 104 pixel_uniforms[2]   2*16 = 32
//   offset 136 pixel_shader        8
//   offset 144 = total size, alignment 8
#[cfg(target_pointer_width = "64")]
const _: () = assert!(size_of::<Batch>() == 144);
#[cfg(target_pointer_width = "64")]
const _: () = assert!(align_of::<Batch>() == 8);

//! Font and text properties on elements:
//!
//! - [`FontFamily`], and the inherited `TextElement` font properties:
//!   [`set_font_size`], [`set_font_family`], [`set_foreground`],
//!   [`set_font_weight`], [`set_font_style`], [`set_font_stretch`].
//! - A subset of the OpenType `Typography` properties: [`set_capitals`],
//!   [`set_numeral_style`], [`set_fraction`], [`set_variants`],
//!   [`set_standard_ligatures`], [`set_kerning`].
//! - IME composition underlines on a `TextBox`: [`add_composition_underline`]
//!   and friends.
//!
//! Every setter has a getter that reads the effective value from the live
//! Noesis object, so an unset property reads back its inherited or default
//! value.
//!
//! [`FontFamily`] owns one reference to its Noesis object and releases it on
//! [`Drop`]. Assigning it to an element makes Noesis take its own reference, so
//! the handle may be dropped right after.
//!
//! # Listing fonts
//!
//! The SDK can list the fonts a family resolves to ([`FontFamily::num_fonts`],
//! [`FontFamily::font_name`]) but not the families installed on the system.
//! Your [font provider](crate::font_provider) decides which families exist, so
//! ask it. See "Listing installed fonts" in `LIMITATIONS.md`.

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::brushes::Brush;
use crate::ffi::{
    noesis_base_component_release, noesis_typography_font_family_create,
    noesis_typography_font_family_get_font_name, noesis_typography_font_family_get_num_fonts,
    noesis_typography_font_family_get_source, noesis_typography_get_capitals,
    noesis_typography_get_fraction, noesis_typography_get_kerning,
    noesis_typography_get_numeral_style, noesis_typography_get_standard_ligatures,
    noesis_typography_get_variants, noesis_typography_set_capitals, noesis_typography_set_fraction,
    noesis_typography_set_kerning, noesis_typography_set_numeral_style,
    noesis_typography_set_standard_ligatures, noesis_typography_set_variants,
    noesis_typography_text_box_add_composition_underline,
    noesis_typography_text_box_clear_composition_underlines,
    noesis_typography_text_box_get_composition_underline,
    noesis_typography_text_box_num_composition_underlines,
    noesis_typography_text_element_get_font_family, noesis_typography_text_element_get_font_size,
    noesis_typography_text_element_get_font_stretch, noesis_typography_text_element_get_font_style,
    noesis_typography_text_element_get_font_weight, noesis_typography_text_element_get_foreground,
    noesis_typography_text_element_set_font_family, noesis_typography_text_element_set_font_size,
    noesis_typography_text_element_set_font_stretch, noesis_typography_text_element_set_font_style,
    noesis_typography_text_element_set_font_weight, noesis_typography_text_element_set_foreground,
};
use crate::view::FrameworkElement;

// Enum ordinals cross the FFI as i32 and must match the Noesis headers.

/// Font weight. The discriminant is the OpenType `usWeightClass` value
/// (`Normal` = 400, `Bold` = 700).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontWeight {
    /// 100
    Thin = 100,
    /// 200
    ExtraLight = 200,
    /// 300
    Light = 300,
    /// 350
    SemiLight = 350,
    /// 400 (the default)
    Normal = 400,
    /// 500
    Medium = 500,
    /// 600
    SemiBold = 600,
    /// 700
    Bold = 700,
    /// 800
    ExtraBold = 800,
    /// 900
    Black = 900,
    /// 950
    ExtraBlack = 950,
}

impl FontWeight {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            100 => Some(Self::Thin),
            200 => Some(Self::ExtraLight),
            300 => Some(Self::Light),
            350 => Some(Self::SemiLight),
            400 => Some(Self::Normal),
            500 => Some(Self::Medium),
            600 => Some(Self::SemiBold),
            700 => Some(Self::Bold),
            800 => Some(Self::ExtraBold),
            900 => Some(Self::Black),
            950 => Some(Self::ExtraBlack),
            _ => None,
        }
    }
}

/// Font slant.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontStyle {
    /// Upright (the default).
    Normal = 0,
    /// Slanted (synthesised) glyphs.
    Oblique = 1,
    /// Italic (designed) glyphs.
    Italic = 2,
}

impl FontStyle {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Oblique),
            2 => Some(Self::Italic),
            _ => None,
        }
    }
}

/// Font width, from most condensed to most expanded.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontStretch {
    /// 1
    UltraCondensed = 1,
    /// 2
    ExtraCondensed = 2,
    /// 3
    Condensed = 3,
    /// 4
    SemiCondensed = 4,
    /// 5 (the default)
    Normal = 5,
    /// 6
    SemiExpanded = 6,
    /// 7
    Expanded = 7,
    /// 8
    ExtraExpanded = 8,
    /// 9
    UltraExpanded = 9,
}

impl FontStretch {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::UltraCondensed),
            2 => Some(Self::ExtraCondensed),
            3 => Some(Self::Condensed),
            4 => Some(Self::SemiCondensed),
            5 => Some(Self::Normal),
            6 => Some(Self::SemiExpanded),
            7 => Some(Self::Expanded),
            8 => Some(Self::ExtraExpanded),
            9 => Some(Self::UltraExpanded),
            _ => None,
        }
    }
}

/// OpenType capital-letter forms (`Typography.Capitals`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontCapitals {
    /// Default.
    Normal = 0,
    /// All glyphs as small caps.
    AllSmallCaps = 1,
    /// Lowercase as small caps.
    SmallCaps = 2,
    /// All glyphs as petite caps.
    AllPetiteCaps = 3,
    /// Lowercase as petite caps.
    PetiteCaps = 4,
    /// Single (unicase) case.
    Unicase = 5,
    /// Titling alternates.
    Titling = 6,
}

impl FontCapitals {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::AllSmallCaps),
            2 => Some(Self::SmallCaps),
            3 => Some(Self::AllPetiteCaps),
            4 => Some(Self::PetiteCaps),
            5 => Some(Self::Unicase),
            6 => Some(Self::Titling),
            _ => None,
        }
    }
}

/// OpenType numeral style (`Typography.NumeralStyle`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontNumeralStyle {
    /// Default.
    Normal = 0,
    /// Lining (uniform-height) figures.
    Lining = 1,
    /// Old-style (variable-height) figures.
    OldStyle = 2,
}

impl FontNumeralStyle {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Lining),
            2 => Some(Self::OldStyle),
            _ => None,
        }
    }
}

/// OpenType fraction style (`Typography.Fraction`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontFraction {
    /// Default.
    Normal = 0,
    /// Diagonal (slashed) fractions.
    Slashed = 1,
    /// Stacked (vertical) fractions.
    Stacked = 2,
}

impl FontFraction {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Slashed),
            2 => Some(Self::Stacked),
            _ => None,
        }
    }
}

/// OpenType glyph variants such as superscript and subscript
/// (`Typography.Variants`).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FontVariants {
    /// Default.
    Normal = 0,
    /// Superscript.
    Superscript = 1,
    /// Subscript.
    Subscript = 2,
    /// Ordinal.
    Ordinal = 3,
    /// Inferior.
    Inferior = 4,
    /// Ruby.
    Ruby = 5,
}

impl FontVariants {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Normal),
            1 => Some(Self::Superscript),
            2 => Some(Self::Subscript),
            3 => Some(Self::Ordinal),
            4 => Some(Self::Inferior),
            5 => Some(Self::Ruby),
            _ => None,
        }
    }
}

/// The line style of an IME [`CompositionUnderline`].
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CompositionLineStyle {
    /// No line.
    None = 0,
    /// Solid line.
    Solid = 1,
    /// Dotted line.
    Dot = 2,
    /// Dashed line.
    Dash = 3,
    /// Squiggly (wavy) line.
    Squiggle = 4,
}

impl CompositionLineStyle {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Solid),
            2 => Some(Self::Dot),
            3 => Some(Self::Dash),
            4 => Some(Self::Squiggle),
            _ => None,
        }
    }
}

/// An owning handle to a Noesis `FontFamily`, created from a source string
/// such as `"Arial"`, `"#PT Root UI"`, or a comma-separated fallback list.
///
/// Assign it with [`set_font_family`]; the element takes its own reference, so
/// the handle may be dropped afterwards.
pub struct FontFamily {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for FontFamily {}

impl FontFamily {
    /// Create a `FontFamily` from its source string.
    ///
    /// # Panics
    ///
    /// Panics if `source` contains an interior NUL byte, or if Noesis returns a
    /// null object.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let c = CString::new(source).expect("font family source contained NUL");
        // SAFETY: c.as_ptr() lives for the call; the C side copies the string
        // into the FontFamily and hands back a +1 BaseComponent*.
        let ptr = unsafe { noesis_typography_font_family_create(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_typography_font_family_create returned null"),
        }
    }

    /// Raw `Noesis::FontFamily*` (a `BaseComponent*`), borrowed for `self`'s
    /// lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The source string this family was created from.
    #[must_use]
    pub fn source(&self) -> Option<String> {
        read_source(self.ptr.as_ptr())
    }

    /// Number of fonts the family resolved to through the registered font
    /// provider. `0` with no provider or no match.
    #[must_use]
    pub fn num_fonts(&self) -> u32 {
        // SAFETY: self.ptr is a live FontFamily*.
        unsafe { noesis_typography_font_family_get_num_fonts(self.ptr.as_ptr()) }
    }

    /// Name of the resolved font at `index`, or `None` if `index` is not below
    /// [`num_fonts`](Self::num_fonts).
    #[must_use]
    pub fn font_name(&self, index: u32) -> Option<String> {
        // SAFETY: self.ptr is a live FontFamily*; the returned pointer is a
        // borrowed NUL-terminated UTF-8 name or null (out of range).
        let p = unsafe { noesis_typography_font_family_get_font_name(self.ptr.as_ptr(), index) };
        if p.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }
}

impl Drop for FontFamily {
    fn drop(&mut self) {
        // SAFETY: produced by font_family_create with a +1 ref we own.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A borrowed `FontFamily` returned by [`get_font_family`]. It holds no
/// reference of its own and cannot outlive the element borrow it came from.
pub struct FontFamilyRef<'a> {
    ptr: NonNull<c_void>,
    _marker: PhantomData<&'a ()>,
}

impl FontFamilyRef<'_> {
    /// Raw `Noesis::FontFamily*`. Compare it with [`FontFamily::raw`] to check
    /// which family is assigned.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The source string of the assigned family.
    #[must_use]
    pub fn source(&self) -> Option<String> {
        read_source(self.ptr.as_ptr())
    }
}

fn read_source(ptr: *mut c_void) -> Option<String> {
    // SAFETY: ptr is a live FontFamily*; GetSource returns a borrowed
    // NUL-terminated UTF-8 string valid while a reference is held. Copy it out.
    let p = unsafe { noesis_typography_font_family_get_source(ptr) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

/// Set `TextElement.FontSize` on `element`, in device-independent pixels. It
/// inherits down the element tree.
///
/// This and the other setters here return `false` only if Noesis rejects the
/// element as not a `DependencyObject`, which doesn't happen for a
/// [`FrameworkElement`].
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_size(element: &FrameworkElement, size: f32) -> bool {
    // SAFETY: element.raw() is a live FrameworkElement* (a DependencyObject*).
    unsafe { noesis_typography_text_element_set_font_size(element.raw(), size) }
}

/// The effective `TextElement.FontSize` of `element`.
#[must_use]
pub fn font_size(element: &FrameworkElement) -> Option<f32> {
    let mut out = 0.0_f32;
    // SAFETY: element.raw() is live; out is a valid writable f32.
    if unsafe { noesis_typography_text_element_get_font_size(element.raw(), &mut out) } {
        Some(out)
    } else {
        None
    }
}

/// Set `TextElement.FontFamily` on `element`. The element takes its own
/// reference to `family`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_family(element: &FrameworkElement, family: &FontFamily) -> bool {
    // SAFETY: both pointers are live for the call.
    unsafe { noesis_typography_text_element_set_font_family(element.raw(), family.raw()) }
}

/// The effective `TextElement.FontFamily` of `element`, or `None` if there is
/// none.
#[must_use]
pub fn get_font_family(element: &FrameworkElement) -> Option<FontFamilyRef<'_>> {
    // SAFETY: element.raw() is live; the returned pointer is a borrowed
    // FontFamily* (no +1) valid while the element holds it, or null.
    let p = unsafe { noesis_typography_text_element_get_font_family(element.raw()) };
    NonNull::new(p).map(|ptr| FontFamilyRef {
        ptr,
        _marker: PhantomData,
    })
}

/// Set `TextElement.Foreground` (the text color) on `element` to any [`Brush`]
/// from [`crate::brushes`]. The element takes its own reference.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_foreground(element: &FrameworkElement, brush: &impl Brush) -> bool {
    // SAFETY: both pointers are live for the call.
    unsafe { noesis_typography_text_element_set_foreground(element.raw(), brush.brush_raw()) }
}

/// Borrowed `Brush*` of the effective `TextElement.Foreground`, or `None`. It
/// holds no reference; compare it with a brush's raw pointer to check which
/// brush is assigned.
#[must_use]
pub fn get_foreground(element: &FrameworkElement) -> Option<NonNull<c_void>> {
    // SAFETY: element.raw() is live; returns a borrowed Brush* or null.
    let p = unsafe { noesis_typography_text_element_get_foreground(element.raw()) };
    NonNull::new(p)
}

/// Set `TextElement.FontWeight` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_weight(element: &FrameworkElement, weight: FontWeight) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_text_element_set_font_weight(element.raw(), weight as i32) }
}

/// The effective `TextElement.FontWeight` of `element`. `None` if the value is
/// not one of the named [`FontWeight`] variants.
#[must_use]
pub fn font_weight(element: &FrameworkElement) -> Option<FontWeight> {
    read_i32(element, noesis_typography_text_element_get_font_weight).and_then(FontWeight::from_raw)
}

/// Set `TextElement.FontStyle` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_style(element: &FrameworkElement, style: FontStyle) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_text_element_set_font_style(element.raw(), style as i32) }
}

/// The effective `TextElement.FontStyle` of `element`. `None` if the value is
/// not a known [`FontStyle`].
#[must_use]
pub fn font_style(element: &FrameworkElement) -> Option<FontStyle> {
    read_i32(element, noesis_typography_text_element_get_font_style).and_then(FontStyle::from_raw)
}

/// Set `TextElement.FontStretch` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_stretch(element: &FrameworkElement, stretch: FontStretch) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_text_element_set_font_stretch(element.raw(), stretch as i32) }
}

/// The effective `TextElement.FontStretch` of `element`. `None` if the value is
/// not a known [`FontStretch`].
#[must_use]
pub fn font_stretch(element: &FrameworkElement) -> Option<FontStretch> {
    read_i32(element, noesis_typography_text_element_get_font_stretch)
        .and_then(FontStretch::from_raw)
}

/// Set `Typography.Capitals` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_capitals(element: &FrameworkElement, value: FontCapitals) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_capitals(element.raw(), value as i32) }
}

/// The effective `Typography.Capitals` of `element`. `None` if the value is not
/// a known [`FontCapitals`].
#[must_use]
pub fn capitals(element: &FrameworkElement) -> Option<FontCapitals> {
    read_i32(element, noesis_typography_get_capitals).and_then(FontCapitals::from_raw)
}

/// Set `Typography.NumeralStyle` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_numeral_style(element: &FrameworkElement, value: FontNumeralStyle) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_numeral_style(element.raw(), value as i32) }
}

/// The effective `Typography.NumeralStyle` of `element`. `None` if the value is
/// not a known [`FontNumeralStyle`].
#[must_use]
pub fn numeral_style(element: &FrameworkElement) -> Option<FontNumeralStyle> {
    read_i32(element, noesis_typography_get_numeral_style).and_then(FontNumeralStyle::from_raw)
}

/// Set `Typography.Fraction` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_fraction(element: &FrameworkElement, value: FontFraction) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_fraction(element.raw(), value as i32) }
}

/// The effective `Typography.Fraction` of `element`. `None` if the value is not
/// a known [`FontFraction`].
#[must_use]
pub fn fraction(element: &FrameworkElement) -> Option<FontFraction> {
    read_i32(element, noesis_typography_get_fraction).and_then(FontFraction::from_raw)
}

/// Set `Typography.Variants` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_variants(element: &FrameworkElement, value: FontVariants) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_variants(element.raw(), value as i32) }
}

/// The effective `Typography.Variants` of `element`. `None` if the value is not
/// a known [`FontVariants`].
#[must_use]
pub fn variants(element: &FrameworkElement) -> Option<FontVariants> {
    read_i32(element, noesis_typography_get_variants).and_then(FontVariants::from_raw)
}

/// Set `Typography.StandardLigatures` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_standard_ligatures(element: &FrameworkElement, value: bool) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_standard_ligatures(element.raw(), value) }
}

/// The effective `Typography.StandardLigatures` of `element`.
#[must_use]
pub fn standard_ligatures(element: &FrameworkElement) -> Option<bool> {
    read_bool(element, noesis_typography_get_standard_ligatures)
}

/// Set `Typography.Kerning` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_kerning(element: &FrameworkElement, value: bool) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_set_kerning(element.raw(), value) }
}

/// The effective `Typography.Kerning` of `element`.
#[must_use]
pub fn kerning(element: &FrameworkElement) -> Option<bool> {
    read_bool(element, noesis_typography_get_kerning)
}

/// An underlined range of IME composition text in a `TextBox`. `start` and
/// `end` are character offsets into the text.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CompositionUnderline {
    /// Start character offset.
    pub start: u32,
    /// End character offset.
    pub end: u32,
    /// Line style.
    pub style: CompositionLineStyle,
    /// Whether the underline is drawn bold.
    pub bold: bool,
}

/// Append an IME composition underline to a `TextBox`. Returns `false` if
/// `element` is not a `TextBox`.
pub fn add_composition_underline(
    element: &FrameworkElement,
    underline: CompositionUnderline,
) -> bool {
    // SAFETY: element.raw() is live.
    unsafe {
        noesis_typography_text_box_add_composition_underline(
            element.raw(),
            underline.start,
            underline.end,
            underline.style as i32,
            underline.bold,
        )
    }
}

/// Number of IME composition underlines on a `TextBox`, or `None` if `element`
/// is not a `TextBox`.
#[must_use]
pub fn num_composition_underlines(element: &FrameworkElement) -> Option<u32> {
    // SAFETY: element.raw() is live.
    let n = unsafe { noesis_typography_text_box_num_composition_underlines(element.raw()) };
    if n < 0 { None } else { Some(n as u32) }
}

/// The IME composition underline at `index`. `None` if `index` is out of range,
/// `element` is not a `TextBox`, or the line style is not a known
/// [`CompositionLineStyle`].
#[must_use]
pub fn composition_underline(
    element: &FrameworkElement,
    index: u32,
) -> Option<CompositionUnderline> {
    let mut start = 0_u32;
    let mut end = 0_u32;
    let mut style = 0_i32;
    let mut bold = false;
    // SAFETY: element.raw() is live; all out pointers are valid writable slots.
    let ok = unsafe {
        noesis_typography_text_box_get_composition_underline(
            element.raw(),
            index,
            &mut start,
            &mut end,
            &mut style,
            &mut bold,
        )
    };
    if ok {
        Some(CompositionUnderline {
            start,
            end,
            style: CompositionLineStyle::from_raw(style)?,
            bold,
        })
    } else {
        None
    }
}

/// Clear all IME composition underlines on a `TextBox`. Returns `false` if
/// `element` is not a `TextBox`.
pub fn clear_composition_underlines(element: &FrameworkElement) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { noesis_typography_text_box_clear_composition_underlines(element.raw()) }
}

fn read_i32(
    element: &FrameworkElement,
    f: unsafe extern "C" fn(*mut c_void, *mut i32) -> bool,
) -> Option<i32> {
    let mut out = 0_i32;
    // SAFETY: element.raw() is live; out is a valid writable i32.
    if unsafe { f(element.raw(), &mut out) } {
        Some(out)
    } else {
        None
    }
}

fn read_bool(
    element: &FrameworkElement,
    f: unsafe extern "C" fn(*mut c_void, *mut bool) -> bool,
) -> Option<bool> {
    let mut out = false;
    // SAFETY: element.raw() is live; out is a valid writable bool.
    if unsafe { f(element.raw(), &mut out) } {
        Some(out)
    } else {
        None
    }
}

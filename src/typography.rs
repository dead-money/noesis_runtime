//! Typography & text properties (TODO §13): the [`FontFamily`] wrapper, the
//! `TextElement` attached font properties (size / family / foreground / weight /
//! style / stretch), a representative subset of the OpenType [`Typography`]
//! attached properties, and the IME [`CompositionUnderline`] list on a `TextBox`.
//!
//! # Ownership
//!
//! [`FontFamily`] is an owning handle over a freshly-created Noesis `FontFamily`
//! holding a single `+1` reference, released on [`Drop`] — the same idiom as the
//! brush/transform handles in [`crate::brushes`]. Assigning it to an element's
//! `TextElement.FontFamily` makes Noesis take its own reference, so the handle
//! may be dropped right after assignment.
//!
//! The `TextElement` / `Typography` accessors and the `CompositionUnderline`
//! list operate on a borrowed [`FrameworkElement`](crate::view::FrameworkElement)
//! (any element, or specifically a `TextBox` for the IME underlines). Every
//! setter has a getter that re-reads the value from the *live* Noesis object, so
//! a stubbed / no-op implementation fails the round-trip tests.
//!
//! # Font family enumeration
//!
//! 3.2.13 exposes per-family enumeration only ([`FontFamily::num_fonts`] /
//! [`FontFamily::font_name`], which resolve through the registered font
//! provider). There is no SDK API to enumerate the set of *available family
//! names* from the font system; the host font provider is the authority on which
//! families it serves. See `TODO.md` "Known SDK limitations".

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::brushes::Brush;
use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_typography_font_family_create,
    dm_noesis_typography_font_family_get_font_name, dm_noesis_typography_font_family_get_num_fonts,
    dm_noesis_typography_font_family_get_source, dm_noesis_typography_get_capitals,
    dm_noesis_typography_get_fraction, dm_noesis_typography_get_kerning,
    dm_noesis_typography_get_numeral_style, dm_noesis_typography_get_standard_ligatures,
    dm_noesis_typography_get_variants, dm_noesis_typography_set_capitals,
    dm_noesis_typography_set_fraction, dm_noesis_typography_set_kerning,
    dm_noesis_typography_set_numeral_style, dm_noesis_typography_set_standard_ligatures,
    dm_noesis_typography_set_variants, dm_noesis_typography_text_box_add_composition_underline,
    dm_noesis_typography_text_box_clear_composition_underlines,
    dm_noesis_typography_text_box_get_composition_underline,
    dm_noesis_typography_text_box_num_composition_underlines,
    dm_noesis_typography_text_element_get_font_family,
    dm_noesis_typography_text_element_get_font_size,
    dm_noesis_typography_text_element_get_font_stretch,
    dm_noesis_typography_text_element_get_font_style,
    dm_noesis_typography_text_element_get_font_weight,
    dm_noesis_typography_text_element_get_foreground,
    dm_noesis_typography_text_element_set_font_family,
    dm_noesis_typography_text_element_set_font_size,
    dm_noesis_typography_text_element_set_font_stretch,
    dm_noesis_typography_text_element_set_font_style,
    dm_noesis_typography_text_element_set_font_weight,
    dm_noesis_typography_text_element_set_foreground,
};
use crate::view::FrameworkElement;

// ── Enums (ordinals mirror the Noesis headers exactly) ───────────────────────

/// `Noesis::FontWeight` (FontProperties.h). The numeric value *is* the weight
/// (e.g. `Normal` = 400, `Bold` = 700), matching the OpenType `usWeightClass`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::FontStyle` (FontProperties.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontStyle {
    /// Upright (the default).
    Normal = 0,
    /// Slanted (synthesised) glyphs.
    Oblique = 1,
    /// Italic (designed) glyphs.
    Italic = 2,
}

/// `Noesis::FontStretch` (FontProperties.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::FontCapitals` (Typography.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::FontNumeralStyle` (Typography.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontNumeralStyle {
    /// Default.
    Normal = 0,
    /// Lining (uniform-height) figures.
    Lining = 1,
    /// Old-style (variable-height) figures.
    OldStyle = 2,
}

/// `Noesis::FontFraction` (Typography.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum FontFraction {
    /// Default.
    Normal = 0,
    /// Diagonal (slashed) fractions.
    Slashed = 1,
    /// Stacked (vertical) fractions.
    Stacked = 2,
}

/// `Noesis::FontVariants` (Typography.h).
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// `Noesis::CompositionLineStyle` (CompositionUnderline.h) — the line style of
/// an IME composition underline.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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
    fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::Solid,
            2 => Self::Dot,
            3 => Self::Dash,
            4 => Self::Squiggle,
            _ => Self::None,
        }
    }
}

// ── FontFamily ───────────────────────────────────────────────────────────────

/// An owning handle to a Noesis `FontFamily`, created from a *source* string
/// (e.g. `"Arial"`, `"#PT Root UI"`, or a comma-separated fallback list).
///
/// Holds one `+1` reference released on [`Drop`]. Assign it to an element with
/// [`set_font_family`] (which `AddRef`s on the Noesis side), after which the
/// handle may be dropped.
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
        let ptr = unsafe { dm_noesis_typography_font_family_create(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_typography_font_family_create returned null"),
        }
    }

    /// Raw `Noesis::FontFamily*` (a `BaseComponent*`), borrowed for `self`'s
    /// lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The source string used to construct this family, re-read from the live
    /// Noesis object.
    #[must_use]
    pub fn source(&self) -> Option<String> {
        read_source(self.ptr.as_ptr())
    }

    /// Number of concrete fonts the family resolved to via the registered font
    /// provider (`0` with no provider). This is the only per-family enumeration
    /// 3.2.13 offers — see the module docs.
    #[must_use]
    pub fn num_fonts(&self) -> u32 {
        // SAFETY: self.ptr is a live FontFamily*.
        unsafe { dm_noesis_typography_font_family_get_num_fonts(self.ptr.as_ptr()) }
    }

    /// Name of the resolved font at `index`, or `None` if out of range.
    #[must_use]
    pub fn font_name(&self, index: u32) -> Option<String> {
        // SAFETY: self.ptr is a live FontFamily*; the returned pointer is a
        // borrowed NUL-terminated UTF-8 name or null (out of range).
        let p = unsafe { dm_noesis_typography_font_family_get_font_name(self.ptr.as_ptr(), index) };
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A *borrowed* `FontFamily` read back from an element's `TextElement.FontFamily`
/// (see [`get_font_family`]). Does not own a reference and must not outlive the
/// element it was read from.
pub struct FontFamilyRef {
    ptr: NonNull<c_void>,
}

impl FontFamilyRef {
    /// Raw `Noesis::FontFamily*`. Use it to assert pointer identity against the
    /// [`FontFamily`] handle that was assigned.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The source string of the assigned family, re-read from the live object.
    #[must_use]
    pub fn source(&self) -> Option<String> {
        read_source(self.ptr.as_ptr())
    }
}

fn read_source(ptr: *mut c_void) -> Option<String> {
    // SAFETY: ptr is a live FontFamily*; GetSource returns a borrowed
    // NUL-terminated UTF-8 string valid while a reference is held. Copy it out.
    let p = unsafe { dm_noesis_typography_font_family_get_source(ptr) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

// ── TextElement attached font properties ─────────────────────────────────────

/// Set `TextElement.FontSize` (device-independent pixels) on `element`. Returns
/// `false` if `element` is not a `DependencyObject`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_size(element: &FrameworkElement, size: f32) -> bool {
    // SAFETY: element.raw() is a live FrameworkElement* (a DependencyObject*).
    unsafe { dm_noesis_typography_text_element_set_font_size(element.raw(), size) }
}

/// Read `TextElement.FontSize` back from the live object.
#[must_use]
pub fn font_size(element: &FrameworkElement) -> Option<f32> {
    let mut out = 0.0_f32;
    // SAFETY: element.raw() is live; out is a valid writable f32.
    if unsafe { dm_noesis_typography_text_element_get_font_size(element.raw(), &mut out) } {
        Some(out)
    } else {
        None
    }
}

/// Set `TextElement.FontFamily` on `element` (Noesis `AddRef`s the family).
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_family(element: &FrameworkElement, family: &FontFamily) -> bool {
    // SAFETY: both pointers are live for the call.
    unsafe { dm_noesis_typography_text_element_set_font_family(element.raw(), family.raw()) }
}

/// Read the borrowed `TextElement.FontFamily` currently set on `element`, or
/// `None` if unset / type mismatch.
#[must_use]
pub fn get_font_family(element: &FrameworkElement) -> Option<FontFamilyRef> {
    // SAFETY: element.raw() is live; the returned pointer is a borrowed
    // FontFamily* (no +1) valid while the element holds it, or null.
    let p = unsafe { dm_noesis_typography_text_element_get_font_family(element.raw()) };
    NonNull::new(p).map(|ptr| FontFamilyRef { ptr })
}

/// Set `TextElement.Foreground` on `element` to any [`Brush`] (Noesis `AddRef`s
/// it). Reuses [`crate::brushes`].
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_foreground(element: &FrameworkElement, brush: &impl Brush) -> bool {
    // SAFETY: both pointers are live for the call.
    unsafe { dm_noesis_typography_text_element_set_foreground(element.raw(), brush.brush_raw()) }
}

/// Raw borrowed `TextElement.Foreground` `Brush*` (no `+1`), or `None`. Use it to
/// assert pointer identity against the assigned brush.
#[must_use]
pub fn get_foreground(element: &FrameworkElement) -> Option<NonNull<c_void>> {
    // SAFETY: element.raw() is live; returns a borrowed Brush* or null.
    let p = unsafe { dm_noesis_typography_text_element_get_foreground(element.raw()) };
    NonNull::new(p)
}

/// Set `TextElement.FontWeight` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_weight(element: &FrameworkElement, weight: FontWeight) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_text_element_set_font_weight(element.raw(), weight as i32) }
}

/// Read `TextElement.FontWeight` back, as its raw numeric weight class.
#[must_use]
pub fn font_weight(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_text_element_get_font_weight)
}

/// Set `TextElement.FontStyle` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_style(element: &FrameworkElement, style: FontStyle) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_text_element_set_font_style(element.raw(), style as i32) }
}

/// Read `TextElement.FontStyle` back as its ordinal.
#[must_use]
pub fn font_style(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_text_element_get_font_style)
}

/// Set `TextElement.FontStretch` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_font_stretch(element: &FrameworkElement, stretch: FontStretch) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_text_element_set_font_stretch(element.raw(), stretch as i32) }
}

/// Read `TextElement.FontStretch` back as its ordinal.
#[must_use]
pub fn font_stretch(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_text_element_get_font_stretch)
}

// ── Typography attached DPs (representative subset) ───────────────────────────

/// Set `Typography.Capitals` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_capitals(element: &FrameworkElement, value: FontCapitals) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_capitals(element.raw(), value as i32) }
}

/// Read `Typography.Capitals` back as its ordinal.
#[must_use]
pub fn capitals(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_get_capitals)
}

/// Set `Typography.NumeralStyle` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_numeral_style(element: &FrameworkElement, value: FontNumeralStyle) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_numeral_style(element.raw(), value as i32) }
}

/// Read `Typography.NumeralStyle` back as its ordinal.
#[must_use]
pub fn numeral_style(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_get_numeral_style)
}

/// Set `Typography.Fraction` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_fraction(element: &FrameworkElement, value: FontFraction) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_fraction(element.raw(), value as i32) }
}

/// Read `Typography.Fraction` back as its ordinal.
#[must_use]
pub fn fraction(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_get_fraction)
}

/// Set `Typography.Variants` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_variants(element: &FrameworkElement, value: FontVariants) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_variants(element.raw(), value as i32) }
}

/// Read `Typography.Variants` back as its ordinal.
#[must_use]
pub fn variants(element: &FrameworkElement) -> Option<i32> {
    read_i32(element, dm_noesis_typography_get_variants)
}

/// Set `Typography.StandardLigatures` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_standard_ligatures(element: &FrameworkElement, value: bool) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_standard_ligatures(element.raw(), value) }
}

/// Read `Typography.StandardLigatures` back.
#[must_use]
pub fn standard_ligatures(element: &FrameworkElement) -> Option<bool> {
    read_bool(element, dm_noesis_typography_get_standard_ligatures)
}

/// Set `Typography.Kerning` on `element`.
#[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
pub fn set_kerning(element: &FrameworkElement, value: bool) -> bool {
    // SAFETY: element.raw() is live.
    unsafe { dm_noesis_typography_set_kerning(element.raw(), value) }
}

/// Read `Typography.Kerning` back.
#[must_use]
pub fn kerning(element: &FrameworkElement) -> Option<bool> {
    read_bool(element, dm_noesis_typography_get_kerning)
}

// ── CompositionUnderline (IME) ───────────────────────────────────────────────

/// An IME composition underline range over a `TextBox`'s text (start/end are
/// character offsets), with its line [`style`](CompositionUnderline::style) and
/// bold flag. Mirrors `Noesis::CompositionUnderline`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct CompositionUnderline {
    /// Inclusive start character offset.
    pub start: u32,
    /// Exclusive end character offset.
    pub end: u32,
    /// Line style.
    pub style: CompositionLineStyle,
    /// Whether the underline is bold.
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
        dm_noesis_typography_text_box_add_composition_underline(
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
    let n = unsafe { dm_noesis_typography_text_box_num_composition_underlines(element.raw()) };
    if n < 0 { None } else { Some(n as u32) }
}

/// Read the IME composition underline at `index` back from the live `TextBox`.
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
        dm_noesis_typography_text_box_get_composition_underline(
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
            style: CompositionLineStyle::from_i32(style),
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
    unsafe { dm_noesis_typography_text_box_clear_composition_underlines(element.raw()) }
}

// ── shared read-back helpers ─────────────────────────────────────────────────

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

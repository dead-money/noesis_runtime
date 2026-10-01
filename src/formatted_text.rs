//! Measure and lay out a string in a given font from code, without XAML or a
//! `TextBlock`.
//!
//! Configure a [`Builder`] with [`FormattedText::builder`], then call
//! [`Builder::build`]. Noesis computes glyph metrics and layout when the
//! object is constructed, so every layout constraint is a builder option and
//! the [`FormattedText`] getters read the result back. The handle owns one
//! Noesis reference and releases it on drop.
//!
//! The enum-valued options are plain `i32` ordinals; use the constants in
//! [`font_weight`], [`font_style`], [`font_stretch`], [`text_alignment`],
//! [`text_trimming`], [`text_wrapping`], [`line_stacking_strategy`] and
//! [`flow_direction`].
//!
//! # Font resolution
//!
//! The font family is a name string, resolved through the registered font
//! provider and fallback chain (see [`crate::font_provider`]). If no face
//! matches, Noesis cannot shape glyphs and every metric is zero.
//!
//! ```no_run
//! use noesis_runtime::formatted_text::FormattedText;
//!
//! let text = FormattedText::builder("Hello, world", "Fonts/#Roboto", 16.0)
//!     .max_width(120.0)
//!     .build();
//! println!("{} x {} DIPs, {} lines", text.width(), text.height(), text.num_lines());
//! ```

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    noesis_base_component_release, noesis_formatted_text_create, noesis_formatted_text_get_bounds,
    noesis_formatted_text_get_glyph_position, noesis_formatted_text_get_line_info,
    noesis_formatted_text_get_num_lines, noesis_formatted_text_has_visual_brush,
    noesis_formatted_text_hit_test, noesis_formatted_text_is_empty, noesis_formatted_text_measure,
};

/// Font weight ordinals (`NsGui/FontProperties.h`). The numeric value is the
/// CSS-style weight; `Normal`/`Regular` is `400`, `Bold` is `700`.
pub mod font_weight {
    pub const THIN: i32 = 100;
    pub const EXTRA_LIGHT: i32 = 200;
    pub const LIGHT: i32 = 300;
    pub const SEMI_LIGHT: i32 = 350;
    pub const NORMAL: i32 = 400;
    pub const MEDIUM: i32 = 500;
    pub const SEMI_BOLD: i32 = 600;
    pub const BOLD: i32 = 700;
    pub const EXTRA_BOLD: i32 = 800;
    pub const BLACK: i32 = 900;
    pub const EXTRA_BLACK: i32 = 950;
}

/// Font style ordinals (`NsGui/FontProperties.h`).
pub mod font_style {
    pub const NORMAL: i32 = 0;
    pub const OBLIQUE: i32 = 1;
    pub const ITALIC: i32 = 2;
}

/// Font stretch ordinals (`NsGui/FontProperties.h`); `Normal`/`Medium` is `5`.
pub mod font_stretch {
    pub const ULTRA_CONDENSED: i32 = 1;
    pub const EXTRA_CONDENSED: i32 = 2;
    pub const CONDENSED: i32 = 3;
    pub const SEMI_CONDENSED: i32 = 4;
    pub const NORMAL: i32 = 5;
    pub const SEMI_EXPANDED: i32 = 6;
    pub const EXPANDED: i32 = 7;
    pub const EXTRA_EXPANDED: i32 = 8;
    pub const ULTRA_EXPANDED: i32 = 9;
}

/// Text alignment ordinals (`NsGui/TextProperties.h`).
pub mod text_alignment {
    pub const LEFT: i32 = 0;
    pub const RIGHT: i32 = 1;
    pub const CENTER: i32 = 2;
    pub const JUSTIFY: i32 = 3;
}

/// Text trimming ordinals (`NsGui/TextProperties.h`).
pub mod text_trimming {
    pub const NONE: i32 = 0;
    pub const CHARACTER_ELLIPSIS: i32 = 1;
    pub const WORD_ELLIPSIS: i32 = 2;
}

/// Text wrapping ordinals (`NsGui/TextProperties.h`).
pub mod text_wrapping {
    pub const NO_WRAP: i32 = 0;
    pub const WRAP: i32 = 1;
    pub const WRAP_WITH_OVERFLOW: i32 = 2;
}

/// Line stacking strategy ordinals (`NsGui/TextProperties.h`).
pub mod line_stacking_strategy {
    pub const BLOCK_LINE_HEIGHT: i32 = 0;
    pub const MAX_HEIGHT: i32 = 1;
}

/// Flow direction ordinals (`NsGui/TextProperties.h`).
pub mod flow_direction {
    pub const LEFT_TO_RIGHT: i32 = 0;
    pub const RIGHT_TO_LEFT: i32 = 1;
}

/// Metrics of one laid-out line, from [`FormattedText::line_info`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct LineInfo {
    /// Number of glyphs on the line.
    pub num_glyphs: u32,
    /// Line box height in DIPs.
    pub height: f32,
    /// Baseline offset from the line top in DIPs.
    pub baseline: f32,
}

/// Result of [`FormattedText::hit_test`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HitTest {
    /// Character index of the glyph under the point.
    pub index: u32,
    /// Whether the point landed inside a glyph's bounds.
    pub is_inside: bool,
    /// Whether the point landed on the glyph's trailing half.
    pub is_trailing: bool,
}

/// Configures a [`FormattedText`]. Layout constraints can't be changed after
/// [`Self::build`], so set them all here.
#[derive(Clone, Debug)]
pub struct Builder {
    text: String,
    font_family: String,
    weight: i32,
    stretch: i32,
    style: i32,
    font_size: f32,
    flow_direction: i32,
    max_width: f32,
    max_height: f32,
    line_height: f32,
    text_alignment: i32,
    text_trimming: i32,
    foreground: Option<[f32; 4]>,
}

impl Builder {
    /// Starts a builder for `text` in the family named `font_family` at
    /// `font_size` DIPs. Defaults: normal weight, stretch and style,
    /// left-to-right, unconstrained width and height, natural line height, left
    /// alignment, word-ellipsis trimming, opaque black foreground.
    #[must_use]
    pub fn new(text: impl Into<String>, font_family: impl Into<String>, font_size: f32) -> Self {
        Self {
            text: text.into(),
            font_family: font_family.into(),
            weight: font_weight::NORMAL,
            stretch: font_stretch::NORMAL,
            style: font_style::NORMAL,
            font_size,
            flow_direction: flow_direction::LEFT_TO_RIGHT,
            max_width: -1.0,
            max_height: -1.0,
            line_height: 0.0,
            text_alignment: text_alignment::LEFT,
            text_trimming: text_trimming::WORD_ELLIPSIS,
            foreground: None,
        }
    }

    /// Set the font weight (see [`font_weight`]).
    #[must_use]
    pub fn weight(mut self, weight: i32) -> Self {
        self.weight = weight;
        self
    }

    /// Set the font stretch (see [`font_stretch`]).
    #[must_use]
    pub fn stretch(mut self, stretch: i32) -> Self {
        self.stretch = stretch;
        self
    }

    /// Set the font style (see [`font_style`]).
    #[must_use]
    pub fn style(mut self, style: i32) -> Self {
        self.style = style;
        self
    }

    /// Set the flow direction (see [`flow_direction`]).
    #[must_use]
    pub fn flow_direction(mut self, flow_direction: i32) -> Self {
        self.flow_direction = flow_direction;
        self
    }

    /// Constrains the layout width in DIPs; negative means unconstrained. Text
    /// wraps only when a width is set.
    #[must_use]
    pub fn max_width(mut self, max_width: f32) -> Self {
        self.max_width = max_width;
        self
    }

    /// Constrains the layout height in DIPs; negative means unconstrained.
    #[must_use]
    pub fn max_height(mut self, max_height: f32) -> Self {
        self.max_height = max_height;
        self
    }

    /// Sets a fixed line height in DIPs; `0` uses the font's natural height.
    #[must_use]
    pub fn line_height(mut self, line_height: f32) -> Self {
        self.line_height = line_height;
        self
    }

    /// Set the text alignment (see [`text_alignment`]).
    #[must_use]
    pub fn text_alignment(mut self, text_alignment: i32) -> Self {
        self.text_alignment = text_alignment;
        self
    }

    /// Set the text trimming behavior (see [`text_trimming`]).
    #[must_use]
    pub fn text_trimming(mut self, text_trimming: i32) -> Self {
        self.text_trimming = text_trimming;
        self
    }

    /// Sets the foreground color as `[r, g, b, a]`, each in `0.0..=1.0`. This is
    /// the color used when the text is drawn with
    /// [`DrawingContext::draw_text`](crate::drawing::DrawingContext::draw_text).
    #[must_use]
    pub fn foreground(mut self, rgba: [f32; 4]) -> Self {
        self.foreground = Some(rgba);
        self
    }

    /// Creates the [`FormattedText`] and computes its layout.
    ///
    /// # Panics
    ///
    /// Panics if `text` or `font_family` contain interior NUL bytes, or if
    /// Noesis fails to allocate the object.
    #[must_use]
    pub fn build(&self) -> FormattedText {
        let text = CString::new(self.text.as_str()).expect("text contained interior NUL");
        let family =
            CString::new(self.font_family.as_str()).expect("font_family contained interior NUL");
        let fg_ptr = self
            .foreground
            .as_ref()
            .map_or(core::ptr::null(), |rgba| rgba.as_ptr());
        // SAFETY: both CStrings outlive the synchronous call; `fg_ptr` is either
        // null or a 4-float buffer the C side only reads.
        let ptr = unsafe {
            noesis_formatted_text_create(
                text.as_ptr(),
                family.as_ptr(),
                self.weight,
                self.stretch,
                self.style,
                self.font_size,
                self.flow_direction,
                self.max_width,
                self.max_height,
                self.line_height,
                self.text_alignment,
                self.text_trimming,
                fg_ptr,
            )
        };
        FormattedText {
            ptr: NonNull::new(ptr).expect("noesis_formatted_text_create returned null"),
        }
    }
}

/// A laid-out run of text with read-back metrics. Create one with
/// [`FormattedText::builder`]. Lengths are in DIPs.
pub struct FormattedText {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for FormattedText {}

impl FormattedText {
    /// Starts a [`Builder`] for `text` in `font_family` at `font_size` DIPs.
    #[must_use]
    pub fn builder(
        text: impl Into<String>,
        font_family: impl Into<String>,
        font_size: f32,
    ) -> Builder {
        Builder::new(text, font_family, font_size)
    }

    /// Raw `Noesis::FormattedText*` (a `BaseComponent*`), borrowed for `self`'s
    /// lifetime. No reference is added.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Layout bounds as `[x, y, width, height]` in DIPs.
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: live FormattedText*; `out` is a 4-float buffer.
        unsafe {
            noesis_formatted_text_get_bounds(self.ptr.as_ptr(), out.as_mut_ptr());
        }
        out
    }

    /// Laid-out width in DIPs (`bounds()[2]`).
    #[must_use]
    pub fn width(&self) -> f32 {
        self.bounds()[2]
    }

    /// Laid-out height in DIPs (`bounds()[3]`).
    #[must_use]
    pub fn height(&self) -> f32 {
        self.bounds()[3]
    }

    /// Number of laid-out lines.
    #[must_use]
    pub fn num_lines(&self) -> u32 {
        // SAFETY: live FormattedText*. -1 only on a null/wrong-type handle,
        // which cannot happen for a valid `self`.
        let n = unsafe { noesis_formatted_text_get_num_lines(self.ptr.as_ptr()) };
        u32::try_from(n).unwrap_or(0)
    }

    /// Per-line metrics for `index`, or `None` if `index >= num_lines()`.
    #[must_use]
    pub fn line_info(&self, index: u32) -> Option<LineInfo> {
        let mut num_glyphs = 0u32;
        let mut height = 0.0f32;
        let mut baseline = 0.0f32;
        // SAFETY: live FormattedText*; out pointers are valid local scalars.
        let ok = unsafe {
            noesis_formatted_text_get_line_info(
                self.ptr.as_ptr(),
                index,
                &mut num_glyphs,
                &mut height,
                &mut baseline,
            )
        };
        ok.then_some(LineInfo {
            num_glyphs,
            height,
            baseline,
        })
    }

    /// Whether the `FormattedText` holds no text.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let mut out = false;
        // SAFETY: live FormattedText*; `out` is a valid bool slot.
        unsafe {
            noesis_formatted_text_is_empty(self.ptr.as_ptr(), &mut out);
        }
        out
    }

    /// Whether the `FormattedText` paints with any `VisualBrush`.
    #[must_use]
    pub fn has_visual_brush(&self) -> bool {
        let mut out = false;
        // SAFETY: live FormattedText*; `out` is a valid bool slot.
        unsafe {
            noesis_formatted_text_has_visual_brush(self.ptr.as_ptr(), &mut out);
        }
        out
    }

    /// Measures the text under different constraints and returns
    /// `(width, height)` in DIPs. Negative `max_width` / `max_height` mean
    /// unconstrained. The `i32` arguments take the constants from
    /// [`text_alignment`], [`text_wrapping`], [`text_trimming`],
    /// [`line_stacking_strategy`] and [`flow_direction`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn measure(
        &self,
        alignment: i32,
        wrapping: i32,
        trimming: i32,
        max_width: f32,
        max_height: f32,
        line_height: f32,
        line_stacking: i32,
        flow_direction: i32,
    ) -> (f32, f32) {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: live FormattedText*; out pointers are valid local scalars.
        unsafe {
            noesis_formatted_text_measure(
                self.ptr.as_ptr(),
                alignment,
                wrapping,
                trimming,
                max_width,
                max_height,
                line_height,
                line_stacking,
                flow_direction,
                &mut w,
                &mut h,
            );
        }
        (w, h)
    }

    /// Position `(x, y)` of the glyph at character index `ch_index`, or of the
    /// point just after it when `after_char` is set. Returns `(-10.0, -10.0)`
    /// when the glyph is outside the layout limits.
    #[must_use]
    pub fn glyph_position(&self, ch_index: u32, after_char: bool) -> (f32, f32) {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: live FormattedText*; out pointers are valid local scalars.
        unsafe {
            noesis_formatted_text_get_glyph_position(
                self.ptr.as_ptr(),
                ch_index,
                after_char,
                &mut x,
                &mut y,
            );
        }
        (x, y)
    }

    /// The glyph under the point `(x, y)`, in layout DIPs.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> HitTest {
        let mut index = 0u32;
        let mut is_inside = false;
        let mut is_trailing = false;
        // SAFETY: live FormattedText*; out pointers are valid local scalars.
        unsafe {
            noesis_formatted_text_hit_test(
                self.ptr.as_ptr(),
                x,
                y,
                &mut index,
                &mut is_inside,
                &mut is_trailing,
            );
        }
        HitTest {
            index,
            is_inside,
            is_trailing,
        }
    }
}

impl Drop for FormattedText {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_formatted_text_create with a +1 ref we
        // own; released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

//! Code-built `TextBlock` inline content: construct the `Inline` element family
//! ([`Run`], [`Span`], [`Bold`], [`Italic`], [`Underline`], [`Hyperlink`],
//! [`LineBreak`], [`InlineUIContainer`]) from Rust and assemble them into a
//! `TextBlock`'s (or a `Span`'s) [`InlineCollection`].
//!
//! Each inline is an owning handle over a freshly-created Noesis object holding
//! a single `+1` reference, released on [`Drop`]. Adding an inline to an
//! [`InlineCollection`] makes the collection take its own reference, so the
//! builder handle may be dropped right after the add.
//!
//! Read-back getters ([`Run::text`], [`Hyperlink::navigate_uri`],
//! [`InlineCollection::count`] / [`InlineCollection::get_raw`],
//! [`InlineUIContainer::child_raw`], [`Inline::text_decorations`]) re-read from
//! the live Noesis object rather than echoing a Rust-side cache.
//!
//! The crate owns no `FontFamily` surface.

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    noesis_base_component_release, noesis_text_inlines_bold_create,
    noesis_text_inlines_collection_add, noesis_text_inlines_collection_count,
    noesis_text_inlines_collection_get, noesis_text_inlines_hyperlink_create,
    noesis_text_inlines_hyperlink_get_navigate_uri, noesis_text_inlines_hyperlink_set_navigate_uri,
    noesis_text_inlines_inline_get_text_decorations,
    noesis_text_inlines_inline_set_text_decorations, noesis_text_inlines_italic_create,
    noesis_text_inlines_line_break_create, noesis_text_inlines_run_create,
    noesis_text_inlines_run_get_text, noesis_text_inlines_run_set_text,
    noesis_text_inlines_span_create, noesis_text_inlines_span_get_inlines,
    noesis_text_inlines_text_block_get_inlines, noesis_text_inlines_ui_container_create,
    noesis_text_inlines_ui_container_get_child, noesis_text_inlines_ui_container_set_child,
    noesis_text_inlines_underline_create,
};
use crate::view::FrameworkElement;

/// The `TextDecorations` an [`Inline`] can carry, mirroring
/// `Noesis::TextDecorations`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum TextDecorations {
    /// No decoration.
    None = 0,
    /// A line above the text.
    OverLine = 1,
    /// A line through the text baseline.
    Baseline = 2,
    /// A line under the text.
    Underline = 3,
    /// A line through the middle of the text.
    Strikethrough = 4,
}

impl TextDecorations {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::OverLine),
            2 => Some(Self::Baseline),
            3 => Some(Self::Underline),
            4 => Some(Self::Strikethrough),
            _ => None,
        }
    }
}

/// A handle to a Noesis `Inline`. Implemented by every inline type in this
/// module so [`InlineCollection::add`] accepts any of them while keeping
/// non-inline objects out, and so the shared `TextDecorations` accessors work
/// uniformly.
pub trait Inline {
    /// Borrowed `Noesis::Inline*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. Used by the collection sugar; not normally called directly.
    fn inline_raw(&self) -> *mut c_void;

    /// Apply a [`TextDecorations`] value to this inline (the base `Inline`
    /// property; affects the inline and its descendants).
    fn set_text_decorations(&self, decorations: TextDecorations) -> bool {
        // SAFETY: `inline_raw()` is a live Inline* for `self`'s lifetime.
        unsafe {
            noesis_text_inlines_inline_set_text_decorations(self.inline_raw(), decorations as i32)
        }
    }

    /// Read the [`TextDecorations`] back from the live Noesis object. `None` if
    /// the value is outside the known enum (not expected for a live inline).
    fn text_decorations(&self) -> Option<TextDecorations> {
        // SAFETY: `inline_raw()` is a live Inline* for `self`'s lifetime.
        let v = unsafe { noesis_text_inlines_inline_get_text_decorations(self.inline_raw()) };
        TextDecorations::from_raw(v)
    }
}

macro_rules! inline_handle {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        pub struct $name {
            ptr: NonNull<c_void>,
        }

        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Inline for $name {
            fn inline_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref that
                // we own; released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

inline_handle!(
    /// A `Run`: an inline holding a span of unformatted text.
    Run
);
inline_handle!(
    /// A `Span`: groups other inlines with no inherent rendering. Its nested
    /// inlines are reachable via [`Span::inlines`].
    Span
);
inline_handle!(
    /// A `Bold`: a `Span` subclass that renders its children with a bold weight.
    Bold
);
inline_handle!(
    /// An `Italic`: a `Span` subclass that renders its children italicized.
    Italic
);
inline_handle!(
    /// An `Underline`: a `Span` subclass that underlines its children.
    Underline
);
inline_handle!(
    /// A `Hyperlink`: a `Span` subclass that hosts a navigable URI
    /// ([`Hyperlink::navigate_uri`]).
    Hyperlink
);
inline_handle!(
    /// A `LineBreak`: forces a line break in flow content.
    LineBreak
);
inline_handle!(
    /// An `InlineUIContainer`: embeds a `UIElement` (via
    /// [`InlineUIContainer::set_child`]) inside flow content.
    InlineUIContainer
);

fn new_handle(ptr: *mut c_void, what: &str) -> NonNull<c_void> {
    NonNull::new(ptr).unwrap_or_else(|| panic!("{what} returned null"))
}

impl Run {
    /// Construct a `Run` with the given `text` (copied into the Run's storage).
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL, or if Noesis fails to
    /// allocate the Run (not expected after [`crate::init`]).
    #[must_use]
    pub fn new(text: &str) -> Self {
        let c = CString::new(text).expect("run text contained interior NUL");
        // SAFETY: `c` outlives the call; the C side copies the bytes.
        let ptr = unsafe { noesis_text_inlines_run_create(c.as_ptr()) };
        Self {
            ptr: new_handle(ptr, "noesis_text_inlines_run_create"),
        }
    }

    /// Replace the Run's unformatted text.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_text(&mut self, text: &str) -> bool {
        let c = CString::new(text).expect("run text contained interior NUL");
        // SAFETY: self.ptr is a live Run*; `c` outlives the call.
        unsafe { noesis_text_inlines_run_set_text(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Read the Run's text back from the live Noesis object.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live Run*; the returned pointer is borrowed
        // storage we copy out before any mutation.
        let p = unsafe { noesis_text_inlines_run_get_text(self.ptr.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is a NUL-terminated UTF-8 C string owned by the Run.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

impl Span {
    /// Construct an empty `Span`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the Span.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: hands out a +1 Span*.
        let ptr = unsafe { noesis_text_inlines_span_create() };
        Self {
            ptr: new_handle(ptr, "noesis_text_inlines_span_create"),
        }
    }

    /// The Span's nested [`InlineCollection`] (its child inlines).
    #[must_use]
    pub fn inlines(&self) -> Option<InlineCollection> {
        // SAFETY: self.ptr is a live Span*; the C side hands out a +1 collection.
        let ptr = unsafe { noesis_text_inlines_span_get_inlines(self.ptr.as_ptr()) };
        InlineCollection::from_raw(ptr)
    }
}

impl Default for Span {
    fn default() -> Self {
        Self::new()
    }
}

macro_rules! span_subclass {
    ($name:ident, $create:ident, $what:literal) => {
        impl $name {
            /// Construct an empty instance.
            ///
            /// # Panics
            ///
            /// Panics if Noesis fails to allocate the object.
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: hands out a +1 object.
                let ptr = unsafe { $create() };
                Self {
                    ptr: new_handle(ptr, $what),
                }
            }

            /// The nested [`InlineCollection`] (this is a `Span` subclass).
            #[must_use]
            pub fn inlines(&self) -> Option<InlineCollection> {
                // SAFETY: self.ptr is a live Span subclass*; +1 collection out.
                let ptr = unsafe { noesis_text_inlines_span_get_inlines(self.ptr.as_ptr()) };
                InlineCollection::from_raw(ptr)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

span_subclass!(
    Bold,
    noesis_text_inlines_bold_create,
    "noesis_text_inlines_bold_create"
);
span_subclass!(
    Italic,
    noesis_text_inlines_italic_create,
    "noesis_text_inlines_italic_create"
);
span_subclass!(
    Underline,
    noesis_text_inlines_underline_create,
    "noesis_text_inlines_underline_create"
);
span_subclass!(
    Hyperlink,
    noesis_text_inlines_hyperlink_create,
    "noesis_text_inlines_hyperlink_create"
);

impl Hyperlink {
    /// Set the URI navigated to when the hyperlink is activated.
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_navigate_uri(&mut self, uri: &str) -> bool {
        let c = CString::new(uri).expect("navigate uri contained interior NUL");
        // SAFETY: self.ptr is a live Hyperlink*; `c` outlives the call.
        unsafe { noesis_text_inlines_hyperlink_set_navigate_uri(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Read the `NavigateUri` back from the live Noesis object. `None` if unset
    /// (the C accessor returns null) or empty.
    #[must_use]
    pub fn navigate_uri(&self) -> Option<String> {
        // SAFETY: self.ptr is a live Hyperlink*; borrowed storage copied out.
        let p = unsafe { noesis_text_inlines_hyperlink_get_navigate_uri(self.ptr.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is a NUL-terminated UTF-8 C string owned by the Hyperlink.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        if s.is_empty() { None } else { Some(s) }
    }
}

impl LineBreak {
    /// Construct a `LineBreak`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: hands out a +1 LineBreak*.
        let ptr = unsafe { noesis_text_inlines_line_break_create() };
        Self {
            ptr: new_handle(ptr, "noesis_text_inlines_line_break_create"),
        }
    }
}

impl Default for LineBreak {
    fn default() -> Self {
        Self::new()
    }
}

impl InlineUIContainer {
    /// Construct an empty `InlineUIContainer` (no hosted child yet).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: hands out a +1 InlineUIContainer*.
        let ptr = unsafe { noesis_text_inlines_ui_container_create() };
        Self {
            ptr: new_handle(ptr, "noesis_text_inlines_ui_container_create"),
        }
    }

    /// Host `child` (any `UIElement`, e.g. a `Button`) inside the container.
    /// The container takes its own reference, so `child` may be dropped after.
    /// Returns `false` if `child` is not a `UIElement`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_child(&mut self, child: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live InlineUIContainer*; child.raw() is a live
        // UIElement* (FrameworkElement derives from UIElement).
        unsafe { noesis_text_inlines_ui_container_set_child(self.ptr.as_ptr(), child.raw()) }
    }

    /// Borrowed raw `BaseComponent*` of the hosted child, or null. The address
    /// matches [`FrameworkElement::raw`] of the element set, so it can be
    /// compared for identity. Does not transfer ownership.
    #[must_use]
    pub fn child_raw(&self) -> *mut c_void {
        // SAFETY: self.ptr is a live InlineUIContainer*.
        unsafe { noesis_text_inlines_ui_container_get_child(self.ptr.as_ptr()) }
    }

    /// Whether the container currently hosts a child.
    #[must_use]
    pub fn has_child(&self) -> bool {
        !self.child_raw().is_null()
    }
}

impl Default for InlineUIContainer {
    fn default() -> Self {
        Self::new()
    }
}

/// An owning handle over a live `Noesis::InlineCollection` (a
/// `UICollection<Inline>`), obtained from a `TextBlock` ([`text_block_inlines`])
/// or a [`Span`] ([`Span::inlines`]). Holds a `+1` reference released on
/// [`Drop`]; the collection is also owned by its host element, so the handle is
/// a non-exclusive view that keeps the collection alive while held.
pub struct InlineCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for InlineCollection {}

impl InlineCollection {
    fn from_raw(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Append `inline` to the collection (the collection takes its own
    /// reference). Returns the insertion index, or `None` on failure.
    pub fn add<I: Inline>(&mut self, inline: &I) -> Option<usize> {
        // SAFETY: self.ptr is a live InlineCollection*; inline_raw() is a live
        // Inline* for the call.
        let idx =
            unsafe { noesis_text_inlines_collection_add(self.ptr.as_ptr(), inline.inline_raw()) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Number of inlines currently in the collection.
    #[must_use]
    pub fn count(&self) -> usize {
        // SAFETY: self.ptr is a live InlineCollection*.
        let n = unsafe { noesis_text_inlines_collection_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// Borrowed raw `Inline*` at `index`, or null if out of range. Useful for
    /// proving structure (e.g. that a nested `Run` landed where expected) via
    /// pointer identity against the inline that was added.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live InlineCollection*; bounds checked C-side.
        unsafe { noesis_text_inlines_collection_get(self.ptr.as_ptr(), index as u32) }
    }
}

impl Drop for InlineCollection {
    fn drop(&mut self) {
        // SAFETY: produced by a get-inlines entrypoint with a +1 ref we own;
        // released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// The top-level [`InlineCollection`] of a `TextBlock`. `None` if `element` is
/// not a `TextBlock`.
#[must_use]
pub fn text_block_inlines(element: &FrameworkElement) -> Option<InlineCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; the C side DynamicCasts
    // to TextBlock and hands out a +1 collection (or null).
    let ptr = unsafe { noesis_text_inlines_text_block_get_inlines(element.raw()) };
    InlineCollection::from_raw(ptr)
}

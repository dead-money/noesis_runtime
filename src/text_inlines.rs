//! Build `TextBlock` inline content from Rust: create [`Run`], [`Span`],
//! [`Bold`], [`Italic`], [`Underline`], [`Hyperlink`], [`LineBreak`], and
//! [`InlineUIContainer`] elements and add them to a `TextBlock`'s
//! ([`text_block_inlines`]) or a `Span`'s ([`Span::inlines`])
//! [`InlineCollection`].
//!
//! Each inline handle owns one reference to its Noesis object and releases it
//! on [`Drop`]. Adding an inline to a collection makes the collection take its
//! own reference, so the handle may be dropped right after the add.
//!
//! Getters such as [`Run::text`] and [`Inline::text_decorations`] read from the
//! live Noesis object, so they see changes made by XAML or bindings too.
//!
//! Font properties (`FontFamily`, `FontSize`, `FontWeight`, ...) live in
//! [`crate::typography`].

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    noesis_base_component_release, noesis_text_inlines_bold_create,
    noesis_text_inlines_collection_add, noesis_text_inlines_collection_clear,
    noesis_text_inlines_collection_count, noesis_text_inlines_collection_get,
    noesis_text_inlines_hyperlink_create, noesis_text_inlines_hyperlink_get_navigate_uri,
    noesis_text_inlines_hyperlink_set_navigate_uri,
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

/// The line decoration an [`Inline`] draws on its text.
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

/// A handle to a Noesis `Inline`. Every inline type in this module implements
/// it, so [`InlineCollection::add`] accepts any of them and the
/// `TextDecorations` accessors work on all of them.
pub trait Inline {
    /// Borrowed `Noesis::Inline*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. You don't normally call this directly.
    fn inline_raw(&self) -> *mut c_void;

    /// Set `TextDecorations` on this inline. It applies to the inline and its
    /// children. Returns `false` if Noesis rejects the object as not an
    /// `Inline`, which doesn't happen for this module's types.
    fn set_text_decorations(&self, decorations: TextDecorations) -> bool {
        // SAFETY: `inline_raw()` is a live Inline* for `self`'s lifetime.
        unsafe {
            noesis_text_inlines_inline_set_text_decorations(self.inline_raw(), decorations as i32)
        }
    }

    /// Read `TextDecorations` from the live Noesis object. `None` if the value
    /// is not a known [`TextDecorations`] variant.
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
    /// A `Span`: groups other inlines and adds no formatting of its own. Add
    /// children through [`Span::inlines`].
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
    /// A `Hyperlink`: a `Span` subclass with a navigable URI
    /// ([`Hyperlink::set_navigate_uri`]).
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
    /// Create a `Run` holding a copy of `text`.
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

    /// Replace the Run's text. Always returns `true` for a live `Run`.
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

    /// Read the Run's text from the live Noesis object. Invalid UTF-8 is
    /// replaced with U+FFFD.
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
    /// Create an empty `Span`.
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

    /// The Span's child inlines. Add to the returned collection to nest content
    /// inside the Span.
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
            /// Create an empty instance.
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

            /// The child inlines. Add to the returned collection to nest
            /// content inside this element.
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
    /// Set the URI navigated to when the hyperlink is activated. Always returns
    /// `true` for a live `Hyperlink`.
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

    /// Read `NavigateUri` from the live Noesis object. `None` if unset or
    /// empty.
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
    /// Create a `LineBreak`.
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
    /// Create an empty `InlineUIContainer`.
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

    /// Borrowed `BaseComponent*` of the hosted child, or null. It equals
    /// [`FrameworkElement::raw`] of the element passed to
    /// [`set_child`](Self::set_child), so you can compare them for identity.
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

/// The live inline collection of a `TextBlock` ([`text_block_inlines`]) or a
/// [`Span`] ([`Span::inlines`]). The host element still owns the collection;
/// this handle holds an extra reference that keeps it alive and releases it on
/// [`Drop`].
pub struct InlineCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for InlineCollection {}

impl InlineCollection {
    fn from_raw(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Append `inline`. The collection takes its own reference, so `inline`
    /// may be dropped afterwards. Returns the insertion index, or `None` if
    /// Noesis rejects the add.
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

    /// Borrowed `Inline*` at `index`, or null if out of range. Compare it with
    /// an inline's `raw()` to check which element sits at a position.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live InlineCollection*; bounds checked C-side.
        unsafe { noesis_text_inlines_collection_get(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove all inlines. Use it to replace a `TextBlock`'s content without
    /// rebuilding the element.
    pub fn clear(&mut self) {
        // SAFETY: self.ptr is a live InlineCollection*.
        unsafe { noesis_text_inlines_collection_clear(self.ptr.as_ptr()) }
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
///
/// ```no_run
/// use noesis_runtime::text_inlines::{Bold, Inline, Run, text_block_inlines};
/// # fn demo(text_block: &noesis_runtime::view::FrameworkElement) {
/// let mut inlines = text_block_inlines(text_block).expect("not a TextBlock");
/// inlines.clear();
/// inlines.add(&Run::new("Hello, "));
/// let bold = Bold::new();
/// bold.inlines().unwrap().add(&Run::new("world"));
/// inlines.add(&bold);
/// # }
/// ```
#[must_use]
pub fn text_block_inlines(element: &FrameworkElement) -> Option<InlineCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; the C side DynamicCasts
    // to TextBlock and hands out a +1 collection (or null).
    let ptr = unsafe { noesis_text_inlines_text_block_get_inlines(element.raw()) };
    InlineCollection::from_raw(ptr)
}

//! Code-built animation & timing: construct `Storyboard`s,
//! the common animation classes (`DoubleAnimation` / `ColorAnimation` /
//! `ThicknessAnimation` / `PointAnimation`), their key-frame variants, and the
//! easing-function family from Rust, then run them off the
//! [`View`](crate::view::View) clock.
//!
//! Each handle here owns a freshly-created Noesis object holding a single `+1`
//! reference, released on [`Drop`] via `noesis_base_component_release` — the
//! same idiom as [`crate::brushes`]. Adding an animation to a [`Storyboard`], or
//! a key frame / easing function to its parent, makes Noesis take its own
//! reference, so the Rust builder handle may be dropped after wiring.
//!
//! # Running an animation
//!
//! Animations advance off the view's `TimeManager`, which is pumped by
//! [`View::update`](crate::view::View::update). Build the element tree, create a
//! `View`, then either:
//!
//! - **Storyboard route:** set [`Animation::set_target_name`] +
//!   [`Animation::set_target_property`] on each child, [`Storyboard::add_child`]
//!   them, and [`Storyboard::begin`] against the connected root element; or
//! - **Direct route:** [`Animation::begin_on`] a single animation onto a named
//!   element's dependency property (a `BeginAnimation` / `ApplyAnimationClock`
//!   equivalent off the element's view `TimeManager`).
//!
//! Then pump `view.update(t)` for increasing `t` across the duration and read
//! the animated value back through Noesis (e.g.
//! [`FrameworkElement::get_f32`](crate::view::FrameworkElement::get_f32)).

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    noesis_animation_begin_on, noesis_animation_set_easing_function, noesis_base_component_release,
    noesis_color_animation_add_keyframe, noesis_color_animation_create,
    noesis_color_animation_keyframes_create, noesis_color_animation_set_by,
    noesis_color_animation_set_from, noesis_color_animation_set_to,
    noesis_double_animation_add_keyframe, noesis_double_animation_create,
    noesis_double_animation_keyframes_create, noesis_double_animation_set_by,
    noesis_double_animation_set_from, noesis_double_animation_set_to,
    noesis_easing_function_create, noesis_easing_function_set_amplitude,
    noesis_easing_function_set_exponent, noesis_easing_function_set_oscillations,
    noesis_easing_function_set_power, noesis_easing_function_set_springiness,
    noesis_point_animation_create, noesis_point_animation_set_by, noesis_point_animation_set_from,
    noesis_point_animation_set_to, noesis_storyboard_add_child, noesis_storyboard_begin,
    noesis_storyboard_begin_handoff, noesis_storyboard_child_count, noesis_storyboard_create,
    noesis_storyboard_is_paused, noesis_storyboard_is_playing, noesis_storyboard_pause,
    noesis_storyboard_resume, noesis_storyboard_seek, noesis_storyboard_set_target_name,
    noesis_storyboard_set_target_property, noesis_storyboard_stop,
    noesis_thickness_animation_create, noesis_thickness_animation_set_by,
    noesis_thickness_animation_set_from, noesis_thickness_animation_set_to,
    noesis_timeline_get_duration_seconds, noesis_timeline_set_auto_reverse,
    noesis_timeline_set_begin_time_seconds, noesis_timeline_set_duration_auto,
    noesis_timeline_set_duration_forever, noesis_timeline_set_duration_seconds,
    noesis_timeline_set_fill_behavior, noesis_timeline_set_repeat_count,
    noesis_timeline_set_repeat_duration, noesis_timeline_set_repeat_forever,
    noesis_timeline_set_speed_ratio,
};
use crate::ffi::{
    noesis_animation_begin_storyboard_create, noesis_animation_begin_storyboard_get_handoff,
    noesis_animation_begin_storyboard_get_name, noesis_animation_begin_storyboard_get_storyboard,
    noesis_animation_begin_storyboard_set_handoff, noesis_animation_begin_storyboard_set_name,
    noesis_animation_begin_storyboard_set_storyboard, noesis_animation_int16_animation_create,
    noesis_animation_int16_animation_get_by, noesis_animation_int16_animation_get_from,
    noesis_animation_int16_animation_get_to, noesis_animation_int16_animation_set_by,
    noesis_animation_int16_animation_set_from, noesis_animation_int16_animation_set_to,
    noesis_animation_int16_keyframes_add, noesis_animation_int16_keyframes_count,
    noesis_animation_int16_keyframes_create, noesis_animation_int16_keyframes_get_key_time,
    noesis_animation_int16_keyframes_get_value, noesis_animation_int32_animation_create,
    noesis_animation_int32_animation_get_by, noesis_animation_int32_animation_get_from,
    noesis_animation_int32_animation_get_to, noesis_animation_int32_animation_set_by,
    noesis_animation_int32_animation_set_from, noesis_animation_int32_animation_set_to,
    noesis_animation_int32_keyframes_add, noesis_animation_int32_keyframes_count,
    noesis_animation_int32_keyframes_create, noesis_animation_int32_keyframes_get_key_time,
    noesis_animation_int32_keyframes_get_value, noesis_animation_int64_animation_create,
    noesis_animation_int64_animation_get_by, noesis_animation_int64_animation_get_from,
    noesis_animation_int64_animation_get_to, noesis_animation_int64_animation_set_by,
    noesis_animation_int64_animation_set_from, noesis_animation_int64_animation_set_to,
    noesis_animation_int64_keyframes_add, noesis_animation_int64_keyframes_count,
    noesis_animation_int64_keyframes_create, noesis_animation_int64_keyframes_get_key_time,
    noesis_animation_int64_keyframes_get_value, noesis_animation_keyspline_create,
    noesis_animation_keyspline_get_control_point1, noesis_animation_keyspline_get_control_point2,
    noesis_animation_keyspline_set_control_point1, noesis_animation_keyspline_set_control_point2,
    noesis_animation_matrix_keyframes_add, noesis_animation_matrix_keyframes_count,
    noesis_animation_matrix_keyframes_create, noesis_animation_matrix_keyframes_get_key_time,
    noesis_animation_matrix_keyframes_get_value, noesis_animation_object_keyframes_add,
    noesis_animation_object_keyframes_count, noesis_animation_object_keyframes_create,
    noesis_animation_object_keyframes_get_key_time, noesis_animation_object_keyframes_get_value,
    noesis_animation_rect_animation_create, noesis_animation_rect_animation_get_by,
    noesis_animation_rect_animation_get_from, noesis_animation_rect_animation_get_to,
    noesis_animation_rect_animation_set_by, noesis_animation_rect_animation_set_from,
    noesis_animation_rect_animation_set_to, noesis_animation_rect_keyframes_add,
    noesis_animation_rect_keyframes_count, noesis_animation_rect_keyframes_create,
    noesis_animation_rect_keyframes_get_key_time, noesis_animation_rect_keyframes_get_value,
    noesis_animation_size_animation_create, noesis_animation_size_animation_get_by,
    noesis_animation_size_animation_get_from, noesis_animation_size_animation_get_to,
    noesis_animation_size_animation_set_by, noesis_animation_size_animation_set_from,
    noesis_animation_size_animation_set_to, noesis_animation_size_keyframes_add,
    noesis_animation_size_keyframes_count, noesis_animation_size_keyframes_create,
    noesis_animation_size_keyframes_get_key_time, noesis_animation_size_keyframes_get_value,
};
use crate::ffi::{
    noesis_animation_boolean_keyframes_add, noesis_animation_boolean_keyframes_count,
    noesis_animation_boolean_keyframes_create, noesis_animation_boolean_keyframes_get_key_time,
    noesis_animation_boolean_keyframes_get_value, noesis_animation_parallel_timeline_add_child,
    noesis_animation_parallel_timeline_child_count, noesis_animation_parallel_timeline_create,
    noesis_animation_point_keyframes_add, noesis_animation_point_keyframes_count,
    noesis_animation_point_keyframes_create, noesis_animation_point_keyframes_get_key_time,
    noesis_animation_point_keyframes_get_value, noesis_animation_string_keyframes_add,
    noesis_animation_string_keyframes_count, noesis_animation_string_keyframes_create,
    noesis_animation_string_keyframes_get_key_time, noesis_animation_string_keyframes_get_value,
    noesis_animation_thickness_keyframes_add, noesis_animation_thickness_keyframes_count,
    noesis_animation_thickness_keyframes_create, noesis_animation_thickness_keyframes_get_key_time,
    noesis_animation_thickness_keyframes_get_value,
};
use crate::view::FrameworkElement;

/// How an easing function interpolates over the animation's progress. Ordinals
/// match `Noesis::EasingMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EasingMode {
    /// `100% - f(t)` — decelerating.
    EaseOut = 0,
    /// `f(t)` — accelerating.
    EaseIn = 1,
    /// `EaseIn` for the first half, `EaseOut` for the second.
    EaseInOut = 2,
}

/// The concrete easing curve. Ordinals match the `kind` switch in
/// `cpp/noesis_animation.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EasingKind {
    /// `t^2`.
    Quadratic = 0,
    /// `t^3`.
    Cubic = 1,
    /// `t^4`.
    Quartic = 2,
    /// `t^5`.
    Quintic = 3,
    /// Sinusoidal.
    Sine = 4,
    /// Circular arc.
    Circle = 5,
    /// Retracts slightly before moving (see [`EasingFunction::set_amplitude`]).
    Back = 6,
    /// Bouncing (see [`EasingFunction::set_oscillations`] /
    /// [`EasingFunction::set_springiness`]).
    Bounce = 7,
    /// Spring-like oscillation.
    Elastic = 8,
    /// Exponential (see [`EasingFunction::set_exponent`]).
    Exponential = 9,
    /// Configurable power (see [`EasingFunction::set_power`]).
    Power = 10,
}

/// How a timeline behaves once it reaches the end of its active period.
/// Ordinals match `Noesis::FillBehavior`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FillBehavior {
    /// Hold the final animated value after completion.
    HoldEnd = 0,
    /// Release the animated value (revert to base) after completion.
    Stop = 1,
}

/// How a newly-started animation interacts with one already running on the same
/// property. Ordinals match `Noesis::HandoffBehavior`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandoffBehavior {
    /// Snapshot the current value and replace any running animation.
    SnapshotAndReplace = 0,
    /// Compose with any running animation.
    Compose = 1,
}

/// The interpolation method of a single key frame. Ordinals match the `kind`
/// switch in `cpp/noesis_animation.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyFrameKind {
    /// Jump to the value at the key time (no interpolation).
    Discrete = 0,
    /// Linear interpolation up to the key time.
    Linear = 1,
    /// Eased interpolation (provide an [`EasingFunction`] via
    /// [`KeyFrameInterp::Easing`]).
    Easing = 2,
    /// Spline interpolation (provide a [`KeySpline`] via
    /// [`KeyFrameInterp::Spline`]).
    Spline = 3,
}

/// The interpolation aid passed alongside a key frame: an [`EasingFunction`] for
/// [`KeyFrameKind::Easing`], a [`KeySpline`] for [`KeyFrameKind::Spline`], or
/// nothing for discrete / linear frames.
#[derive(Copy, Clone)]
pub enum KeyFrameInterp<'a> {
    /// No interpolation aid (discrete / linear frames).
    None,
    /// Easing function for an [`KeyFrameKind::Easing`] frame.
    Easing(&'a EasingFunction),
    /// Key spline for a [`KeyFrameKind::Spline`] frame.
    Spline(&'a KeySpline),
}

impl KeyFrameInterp<'_> {
    fn raw(self) -> *mut c_void {
        match self {
            KeyFrameInterp::None => core::ptr::null_mut(),
            KeyFrameInterp::Easing(e) => e.raw(),
            KeyFrameInterp::Spline(s) => s.raw(),
        }
    }
}

/// A handle that exposes its borrowed `Noesis::BaseComponent*`. Implemented by
/// every owning wrapper in this module, letting any of them be used as an
/// [`ObjectAnimationUsingKeyFrames`] key-frame value.
pub trait AsComponent {
    /// Borrowed `Noesis::BaseComponent*` for `self`.
    fn component_raw(&self) -> *mut c_void;
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl AsComponent for $name {
            fn component_raw(&self) -> *mut c_void {
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

/// Common `Timeline` knobs shared by every animation type (duration, repeat,
/// auto-reverse, …). Implemented through the object's raw `Timeline*`.
pub trait Timeline {
    /// Borrowed `Noesis::Timeline*` for `self`.
    fn timeline_raw(&self) -> *mut c_void;

    /// Set the single-pass duration in seconds.
    fn set_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_seconds(self.timeline_raw(), seconds) }
    }

    /// Set `Duration="Automatic"` (resolved from the content, e.g. key frames).
    fn set_duration_auto(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_auto(self.timeline_raw()) }
    }

    /// Set `Duration="Forever"`.
    fn set_duration_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_forever(self.timeline_raw()) }
    }

    /// Read the configured single-pass duration in seconds, or `None` if the
    /// duration is `Automatic` / `Forever` (not a resolved `TimeSpan`).
    fn duration_secs(&self) -> Option<f64> {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        let s = unsafe { noesis_timeline_get_duration_seconds(self.timeline_raw()) };
        (s >= 0.0).then_some(s)
    }

    /// Delay before the timeline begins, in seconds.
    fn set_begin_time_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_begin_time_seconds(self.timeline_raw(), seconds) }
    }

    /// Play forwards then backwards each iteration when `true`.
    fn set_auto_reverse(&mut self, value: bool) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_auto_reverse(self.timeline_raw(), value) }
    }

    /// Rate at which time progresses relative to the parent (default `1.0`).
    fn set_speed_ratio(&mut self, value: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_speed_ratio(self.timeline_raw(), value) }
    }

    /// Behaviour once the active period ends (hold the end value or release it).
    fn set_fill_behavior(&mut self, behavior: FillBehavior) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_fill_behavior(self.timeline_raw(), behavior as i32) }
    }

    /// Repeat a fixed number of (possibly fractional) iterations.
    fn set_repeat_count(&mut self, count: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_count(self.timeline_raw(), count) }
    }

    /// Repeat for a fixed wall-clock duration, in seconds.
    fn set_repeat_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_duration(self.timeline_raw(), seconds) }
    }

    /// Repeat forever.
    fn set_repeat_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_forever(self.timeline_raw()) }
    }
}

/// An animation timeline that can target a property and be run via a
/// [`Storyboard`] or directly with [`begin_on`](Animation::begin_on).
pub trait Animation: Timeline {
    /// Borrowed `Noesis::AnimationTimeline*` for `self`.
    fn animation_raw(&self) -> *mut c_void;

    /// Set this animation's `Storyboard.TargetName` — the `x:Name` of the
    /// element it drives, resolved against the namescope passed to
    /// [`Storyboard::begin`].
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    fn set_target_name(&mut self, name: &str) -> bool {
        let c = CString::new(name).expect("target name contained interior NUL");
        // SAFETY: animation_raw() is a live DependencyObject*; c lives for the call.
        unsafe { noesis_storyboard_set_target_name(self.animation_raw(), c.as_ptr()) }
    }

    /// Set this animation's `Storyboard.TargetProperty` — the property path it
    /// drives (e.g. `"Opacity"`,
    /// `"(UIElement.RenderTransform).(ScaleTransform.ScaleX)"`).
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte.
    fn set_target_property(&mut self, path: &str) -> bool {
        let c = CString::new(path).expect("target property contained interior NUL");
        // SAFETY: animation_raw() is a live DependencyObject*; c lives for the call.
        unsafe { noesis_storyboard_set_target_property(self.animation_raw(), c.as_ptr()) }
    }

    /// Attach an easing function. No-op (returns `false`) for key-frame
    /// animations, whose easing is configured per key frame instead.
    fn set_easing(&mut self, easing: &EasingFunction) -> bool {
        // SAFETY: both pointers are live for the call; Noesis takes its own ref
        // to the easing function.
        unsafe { noesis_animation_set_easing_function(self.animation_raw(), easing.raw()) }
    }

    /// Start this animation directly on `target`'s `dp_name` dependency
    /// property, using the target's view `TimeManager` (a `BeginAnimation` /
    /// `ApplyAnimationClock` equivalent). `target` must be connected to a live
    /// [`View`](crate::view::View) (so it has a `TimeManager`). Returns `false`
    /// on an unknown property or a disconnected target.
    ///
    /// # Panics
    ///
    /// Panics if `dp_name` contains an interior NUL byte.
    fn begin_on(
        &mut self,
        target: &FrameworkElement,
        dp_name: &str,
        handoff: HandoffBehavior,
    ) -> bool {
        let c = CString::new(dp_name).expect("dp name contained interior NUL");
        // SAFETY: animation_raw() and target.raw() are live for the call; c lives
        // for the call; the C side resolves the DP and the TimeManager.
        unsafe {
            noesis_animation_begin_on(
                self.animation_raw(),
                target.raw(),
                c.as_ptr(),
                handoff as i32,
            )
        }
    }
}

/// A `Storyboard` — a container timeline that targets and runs its child
/// animations.
pub struct Storyboard {
    ptr: NonNull<c_void>,
}

base_component_handle!(Storyboard);

impl Timeline for Storyboard {
    fn timeline_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl Default for Storyboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Storyboard {
    /// Create an empty `Storyboard`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Storyboard*.
        let ptr = unsafe { noesis_storyboard_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_storyboard_create returned null"),
        }
    }

    /// Add a child animation. The storyboard's collection takes its own
    /// reference, so `anim` may be dropped afterwards. Returns `false` on a type
    /// mismatch.
    pub fn add_child<A: Animation>(&mut self, anim: &A) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_add_child(self.raw(), anim.animation_raw()) }
    }

    /// Number of child animations, or `None` if the handle is not a Storyboard
    /// (should not happen for a live handle).
    #[must_use]
    pub fn child_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is a live Storyboard*.
        let n = unsafe { noesis_storyboard_child_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Apply this storyboard's animations to their targets and start them.
    /// `root` is both the target tree root and the namescope used to resolve
    /// each child's [`Animation::set_target_name`]; it must be connected to a
    /// live [`View`](crate::view::View). Pass `controllable = true` to enable
    /// [`pause`](Self::pause) / [`resume`](Self::resume) / [`stop`](Self::stop)
    /// / [`seek`](Self::seek).
    pub fn begin(&mut self, root: &FrameworkElement, controllable: bool) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_begin(self.raw(), root.raw(), controllable) }
    }

    /// Like [`begin`](Self::begin), but with an explicit [`HandoffBehavior`]
    /// controlling how the storyboard's new clocks interact with animations
    /// already running on the same properties
    /// ([`SnapshotAndReplace`](HandoffBehavior::SnapshotAndReplace) replaces
    /// them; [`Compose`](HandoffBehavior::Compose) layers on top).
    pub fn begin_with_handoff(
        &mut self,
        root: &FrameworkElement,
        handoff: HandoffBehavior,
        controllable: bool,
    ) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe {
            noesis_storyboard_begin_handoff(self.raw(), root.raw(), handoff as i32, controllable)
        }
    }

    /// Pause the controllable clocks created for `root`. No-op unless the
    /// storyboard was [`begin`](Self::begin)-run with `controllable = true`.
    pub fn pause(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_pause(self.raw(), root.raw()) }
    }

    /// Resume the controllable clocks created for `root`.
    pub fn resume(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_resume(self.raw(), root.raw()) }
    }

    /// Stop the controllable clocks created for `root`.
    pub fn stop(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_stop(self.raw(), root.raw()) }
    }

    /// Seek the controllable clocks created for `root` to `seconds` from the
    /// beginning, applied on the next clock tick.
    pub fn seek(&mut self, root: &FrameworkElement, seconds: f64) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_seek(self.raw(), root.raw(), seconds) }
    }

    /// Whether a controllable storyboard is currently playing for `root`.
    #[must_use]
    pub fn is_playing(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_is_playing(self.raw(), root.raw()) }
    }

    /// Whether a controllable storyboard is currently paused for `root`.
    #[must_use]
    pub fn is_paused(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_is_paused(self.raw(), root.raw()) }
    }
}

/// An easing function applied to a From/To animation or an easing key frame.
pub struct EasingFunction {
    ptr: NonNull<c_void>,
}

base_component_handle!(EasingFunction);

impl EasingFunction {
    /// Create an easing function of `kind` with interpolation `mode`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate (not expected after [`crate::init`]).
    #[must_use]
    pub fn new(kind: EasingKind, mode: EasingMode) -> Self {
        // SAFETY: factory returns a +1-owned EasingFunctionBase*.
        let ptr = unsafe { noesis_easing_function_create(kind as i32, mode as i32) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_easing_function_create returned null"),
        }
    }

    /// Set `BackEase.Amplitude` (the retraction amount). No-op (returns `false`)
    /// on other easing kinds.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_amplitude(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_amplitude(self.raw(), value) }
    }

    /// Set `PowerEase.Power`. No-op on other kinds.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_power(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_power(self.raw(), value) }
    }

    /// Set `ExponentialEase.Exponent`. No-op on other kinds.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_exponent(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_exponent(self.raw(), value) }
    }

    /// Set `ElasticEase.Oscillations` / `BounceEase.Bounces`. No-op on other
    /// kinds.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_oscillations(&mut self, value: i32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_oscillations(self.raw(), value) }
    }

    /// Set `ElasticEase.Springiness` / `BounceEase.Bounciness`. No-op on other
    /// kinds.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_springiness(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_springiness(self.raw(), value) }
    }
}

macro_rules! animation_impls {
    ($name:ident) => {
        base_component_handle!($name);

        impl Timeline for $name {
            fn timeline_raw(&self) -> *mut c_void {
                self.raw()
            }
        }

        impl Animation for $name {
            fn animation_raw(&self) -> *mut c_void {
                self.raw()
            }
        }
    };
}

/// A `DoubleAnimation` — interpolates a `float` property linearly between
/// `From` and `To` over the duration.
pub struct DoubleAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(DoubleAnimation);

impl Default for DoubleAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl DoubleAnimation {
    /// Create an empty `DoubleAnimation` (set `From`/`To`/`Duration` next).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimation*.
        let ptr = unsafe { noesis_double_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_double_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe {
            noesis_double_animation_set_from(self.raw(), value.is_some(), value.unwrap_or(0.0))
        }
    }

    /// Set (`Some`) or clear (`None`) the ending value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe { noesis_double_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0.0)) }
    }

    /// Set (`Some`) or clear (`None`) the relative offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe { noesis_double_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0.0)) }
    }
}

/// A `ColorAnimation` — interpolates a `Color` property linearly between `From`
/// and `To`. Colors are `[r, g, b, a]`, each `0..=1`.
pub struct ColorAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(ColorAnimation);

impl Default for ColorAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorAnimation {
    /// Create an empty `ColorAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimation*.
        let ptr = unsafe { noesis_color_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_color_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting color.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_from(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending color.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_to(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative color offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_by(self.raw(), rgba.is_some(), v.as_ptr()) }
    }
}

/// A `ThicknessAnimation` — interpolates a `Thickness` property
/// (`[left, top, right, bottom]`).
pub struct ThicknessAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(ThicknessAnimation);

impl Default for ThicknessAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl ThicknessAnimation {
    /// Create an empty `ThicknessAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ThicknessAnimation*.
        let ptr = unsafe { noesis_thickness_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_thickness_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting thickness.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending thickness.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative thickness offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }
}

/// A `PointAnimation` — interpolates a `Point` property (`(x, y)`).
pub struct PointAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(PointAnimation);

impl Default for PointAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl PointAnimation {
    /// Create an empty `PointAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned PointAnimation*.
        let ptr = unsafe { noesis_point_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_point_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting point.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_from(self.raw(), value.is_some(), x, y) }
    }

    /// Set (`Some`) or clear (`None`) the ending point.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_to(self.raw(), value.is_some(), x, y) }
    }

    /// Set (`Some`) or clear (`None`) the relative point offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_by(self.raw(), value.is_some(), x, y) }
    }
}

/// A `DoubleAnimationUsingKeyFrames` — animates a `float` property through a
/// sequence of discrete / linear / eased key frames.
pub struct DoubleAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(DoubleAnimationUsingKeyFrames);

impl Default for DoubleAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl DoubleAnimationUsingKeyFrames {
    /// Create an empty key-frame double animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_double_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_double_animation_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` at `key_time_secs`. Provide the
    /// matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: f32,
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is a live keyframe animation; the interp raw pointer
        // is null or a live easing/spline object; both are only read during the
        // call.
        unsafe {
            noesis_double_animation_add_keyframe(
                self.raw(),
                kind as i32,
                key_time_secs,
                value,
                interp.raw(),
            )
        }
    }
}

/// A `ColorAnimationUsingKeyFrames` — animates a `Color` property through a
/// sequence of color key frames.
pub struct ColorAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(ColorAnimationUsingKeyFrames);

impl Default for ColorAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorAnimationUsingKeyFrames {
    /// Create an empty key-frame color animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_color_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_color_animation_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `rgba` at `key_time_secs`. Provide the
    /// matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        rgba: [f32; 4],
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is a live keyframe animation; `rgba` outlives the
        // call; the interp raw pointer is null or a live easing/spline object.
        unsafe {
            noesis_color_animation_add_keyframe(
                self.raw(),
                kind as i32,
                key_time_secs,
                rgba.as_ptr(),
                interp.raw(),
            )
        }
    }
}

/// A `RectAnimation` — interpolates a `Rect` property between `From` and `To`.
/// Rects are `[x, y, width, height]`.
pub struct RectAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(RectAnimation);

impl Default for RectAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl RectAnimation {
    /// Create an empty `RectAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned RectAnimation*.
        let ptr = unsafe { noesis_animation_rect_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_rect_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting rect.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending rect.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative rect offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Read back the `From` rect, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_from(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// Read back the `To` rect, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_to(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// Read back the `By` rect, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_by(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }
}

/// A `SizeAnimation` — interpolates a `Size` property between `From` and `To`.
/// Sizes are `[width, height]`.
pub struct SizeAnimation {
    ptr: NonNull<c_void>,
}

animation_impls!(SizeAnimation);

impl Default for SizeAnimation {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeAnimation {
    /// Create an empty `SizeAnimation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned SizeAnimation*.
        let ptr = unsafe { noesis_animation_size_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_size_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting size.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the ending size.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Set (`Some`) or clear (`None`) the relative size offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Read back the `From` size, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_from(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// Read back the `To` size, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_to(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// Read back the `By` size, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_by(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }
}

/// An `Int16Animation` — interpolates an `int16` property between `From` and
/// `To` (Noesis rounds the interpolated value).
pub struct Int16Animation {
    ptr: NonNull<c_void>,
}

animation_impls!(Int16Animation);

impl Default for Int16Animation {
    fn default() -> Self {
        Self::new()
    }
}

impl Int16Animation {
    /// Create an empty `Int16Animation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int16Animation*.
        let ptr = unsafe { noesis_animation_int16_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int16_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<i16>) -> bool {
        // SAFETY: self.raw() is a live Int16Animation* for the call.
        unsafe {
            noesis_animation_int16_animation_set_from(
                self.raw(),
                value.is_some(),
                i32::from(value.unwrap_or(0)),
            )
        }
    }

    /// Set (`Some`) or clear (`None`) the ending value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<i16>) -> bool {
        // SAFETY: self.raw() is a live Int16Animation* for the call.
        unsafe {
            noesis_animation_int16_animation_set_to(
                self.raw(),
                value.is_some(),
                i32::from(value.unwrap_or(0)),
            )
        }
    }

    /// Set (`Some`) or clear (`None`) the relative offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<i16>) -> bool {
        // SAFETY: self.raw() is a live Int16Animation* for the call.
        unsafe {
            noesis_animation_int16_animation_set_by(
                self.raw(),
                value.is_some(),
                i32::from(value.unwrap_or(0)),
            )
        }
    }

    /// Read back the `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_from(self.raw(), &mut out) };
        has.then_some(out as i16)
    }

    /// Read back the `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_to(self.raw(), &mut out) };
        has.then_some(out as i16)
    }

    /// Read back the `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_by(self.raw(), &mut out) };
        has.then_some(out as i16)
    }
}

/// An `Int32Animation` — interpolates an `int32` property between `From` and
/// `To`.
pub struct Int32Animation {
    ptr: NonNull<c_void>,
}

animation_impls!(Int32Animation);

impl Default for Int32Animation {
    fn default() -> Self {
        Self::new()
    }
}

impl Int32Animation {
    /// Create an empty `Int32Animation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int32Animation*.
        let ptr = unsafe { noesis_animation_int32_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int32_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<i32>) -> bool {
        // SAFETY: self.raw() is a live Int32Animation* for the call.
        unsafe {
            noesis_animation_int32_animation_set_from(
                self.raw(),
                value.is_some(),
                value.unwrap_or(0),
            )
        }
    }

    /// Set (`Some`) or clear (`None`) the ending value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<i32>) -> bool {
        // SAFETY: self.raw() is a live Int32Animation* for the call.
        unsafe {
            noesis_animation_int32_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Set (`Some`) or clear (`None`) the relative offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<i32>) -> bool {
        // SAFETY: self.raw() is a live Int32Animation* for the call.
        unsafe {
            noesis_animation_int32_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Read back the `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_from(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// Read back the `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_to(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// Read back the `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_by(self.raw(), &mut out) };
        has.then_some(out)
    }
}

/// An `Int64Animation` — interpolates an `int64` property between `From` and
/// `To`.
pub struct Int64Animation {
    ptr: NonNull<c_void>,
}

animation_impls!(Int64Animation);

impl Default for Int64Animation {
    fn default() -> Self {
        Self::new()
    }
}

impl Int64Animation {
    /// Create an empty `Int64Animation`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int64Animation*.
        let ptr = unsafe { noesis_animation_int64_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int64_animation_create returned null"),
        }
    }

    /// Set (`Some`) or clear (`None`) the starting value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<i64>) -> bool {
        // SAFETY: self.raw() is a live Int64Animation* for the call.
        unsafe {
            noesis_animation_int64_animation_set_from(
                self.raw(),
                value.is_some(),
                value.unwrap_or(0),
            )
        }
    }

    /// Set (`Some`) or clear (`None`) the ending value.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<i64>) -> bool {
        // SAFETY: self.raw() is a live Int64Animation* for the call.
        unsafe {
            noesis_animation_int64_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Set (`Some`) or clear (`None`) the relative offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<i64>) -> bool {
        // SAFETY: self.raw() is a live Int64Animation* for the call.
        unsafe {
            noesis_animation_int64_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Read back the `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_from(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// Read back the `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_to(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// Read back the `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_by(self.raw(), &mut out) };
        has.then_some(out)
    }
}

/// A `KeySpline` — the two cubic-Bezier control points (each in the unit square)
/// that shape a spline key frame's progress curve. Used with
/// [`KeyFrameKind::Spline`] via [`KeyFrameInterp::Spline`].
pub struct KeySpline {
    ptr: NonNull<c_void>,
}

base_component_handle!(KeySpline);

impl KeySpline {
    /// Create a `KeySpline` from its two control points `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new(control_point1: (f32, f32), control_point2: (f32, f32)) -> Self {
        // SAFETY: factory returns a +1-owned KeySpline*.
        let ptr = unsafe {
            noesis_animation_keyspline_create(
                control_point1.0,
                control_point1.1,
                control_point2.0,
                control_point2.1,
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_keyspline_create returned null"),
        }
    }

    /// Set the first control point `(x, y)`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_control_point1(&mut self, x: f32, y: f32) -> bool {
        // SAFETY: self.raw() is a live KeySpline* for the call.
        unsafe { noesis_animation_keyspline_set_control_point1(self.raw(), x, y) }
    }

    /// Set the second control point `(x, y)`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_control_point2(&mut self, x: f32, y: f32) -> bool {
        // SAFETY: self.raw() is a live KeySpline* for the call.
        unsafe { noesis_animation_keyspline_set_control_point2(self.raw(), x, y) }
    }

    /// Read back the first control point `(x, y)`.
    #[must_use]
    pub fn control_point1(&self) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok =
            unsafe { noesis_animation_keyspline_get_control_point1(self.raw(), out.as_mut_ptr()) };
        ok.then_some((out[0], out[1]))
    }

    /// Read back the second control point `(x, y)`.
    #[must_use]
    pub fn control_point2(&self) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok =
            unsafe { noesis_animation_keyspline_get_control_point2(self.raw(), out.as_mut_ptr()) };
        ok.then_some((out[0], out[1]))
    }
}

/// A `RectAnimationUsingKeyFrames` — animates a `Rect` property through
/// discrete / linear / eased / splined key frames.
pub struct RectAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(RectAnimationUsingKeyFrames);

impl Default for RectAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl RectAnimationUsingKeyFrames {
    /// Create an empty key-frame rect animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned RectAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_rect_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_rect_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` (`[x, y, width, height]`) at
    /// `key_time_secs`. Provide the matching [`KeyFrameInterp`] for
    /// [`KeyFrameKind::Easing`] / [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: [f32; 4],
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; `value` outlives the call; the interp raw
        // pointer is null or a live easing/spline object.
        unsafe {
            noesis_animation_rect_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                value.as_ptr(),
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_rect_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let ok = unsafe {
            noesis_animation_rect_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_rect_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `SizeAnimationUsingKeyFrames` — animates a `Size` property through
/// discrete / linear / eased / splined key frames.
pub struct SizeAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(SizeAnimationUsingKeyFrames);

impl Default for SizeAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl SizeAnimationUsingKeyFrames {
    /// Create an empty key-frame size animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned SizeAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_size_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_size_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` (`[width, height]`) at
    /// `key_time_secs`. Provide the matching [`KeyFrameInterp`] for
    /// [`KeyFrameKind::Easing`] / [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: [f32; 2],
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; `value` outlives the call; the interp raw
        // pointer is null or a live easing/spline object.
        unsafe {
            noesis_animation_size_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                value.as_ptr(),
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_size_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok = unsafe {
            noesis_animation_size_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_size_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// An `Int16AnimationUsingKeyFrames` — animates an `int16` property through
/// discrete / linear / eased / splined key frames.
pub struct Int16AnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(Int16AnimationUsingKeyFrames);

impl Default for Int16AnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl Int16AnimationUsingKeyFrames {
    /// Create an empty key-frame int16 animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int16AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int16_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int16_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` at `key_time_secs`. Provide the
    /// matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: i16,
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; the interp raw pointer is null or a live
        // easing/spline object.
        unsafe {
            noesis_animation_int16_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                i32::from(value),
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int16_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let ok = unsafe {
            noesis_animation_int16_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out as i16)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int16_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// An `Int32AnimationUsingKeyFrames` — animates an `int32` property through
/// discrete / linear / eased / splined key frames.
pub struct Int32AnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(Int32AnimationUsingKeyFrames);

impl Default for Int32AnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl Int32AnimationUsingKeyFrames {
    /// Create an empty key-frame int32 animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int32AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int32_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int32_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` at `key_time_secs`. Provide the
    /// matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: i32,
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; the interp raw pointer is null or a live
        // easing/spline object.
        unsafe {
            noesis_animation_int32_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                value,
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int32_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let ok = unsafe {
            noesis_animation_int32_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int32_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// An `Int64AnimationUsingKeyFrames` — animates an `int64` property through
/// discrete / linear / eased / splined key frames.
pub struct Int64AnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(Int64AnimationUsingKeyFrames);

impl Default for Int64AnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl Int64AnimationUsingKeyFrames {
    /// Create an empty key-frame int64 animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int64AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int64_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int64_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` at `key_time_secs`. Provide the
    /// matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: i64,
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; the interp raw pointer is null or a live
        // easing/spline object.
        unsafe {
            noesis_animation_int64_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                value,
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int64_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let ok = unsafe {
            noesis_animation_int64_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int64_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `PointAnimationUsingKeyFrames` — animates a `Point` property (`(x, y)`)
/// through discrete / linear / eased / splined key frames.
pub struct PointAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(PointAnimationUsingKeyFrames);

impl Default for PointAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl PointAnimationUsingKeyFrames {
    /// Create an empty key-frame point animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned PointAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_point_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_point_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` (`(x, y)`) at `key_time_secs`. Provide
    /// the matching [`KeyFrameInterp`] for [`KeyFrameKind::Easing`] /
    /// [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: (f32, f32),
        interp: KeyFrameInterp,
    ) -> bool {
        let p = [value.0, value.1];
        // SAFETY: self.raw() is live; `p` outlives the call; the interp raw
        // pointer is null or a live easing/spline object.
        unsafe {
            noesis_animation_point_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                p.as_ptr(),
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_point_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value `(x, y)` at `index`, or `None` if out of
    /// range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok = unsafe {
            noesis_animation_point_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some((out[0], out[1]))
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_point_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `ThicknessAnimationUsingKeyFrames` — animates a `Thickness` property
/// (`[left, top, right, bottom]`) through discrete / linear / eased / splined
/// key frames.
pub struct ThicknessAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(ThicknessAnimationUsingKeyFrames);

impl Default for ThicknessAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl ThicknessAnimationUsingKeyFrames {
    /// Create an empty key-frame thickness animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ThicknessAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_thickness_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_thickness_keyframes_create returned null"),
        }
    }

    /// Append a key frame reaching `value` (`[left, top, right, bottom]`) at
    /// `key_time_secs`. Provide the matching [`KeyFrameInterp`] for
    /// [`KeyFrameKind::Easing`] / [`KeyFrameKind::Spline`].
    pub fn add_key_frame(
        &mut self,
        kind: KeyFrameKind,
        key_time_secs: f64,
        value: [f32; 4],
        interp: KeyFrameInterp,
    ) -> bool {
        // SAFETY: self.raw() is live; `value` outlives the call; the interp raw
        // pointer is null or a live easing/spline object.
        unsafe {
            noesis_animation_thickness_keyframes_add(
                self.raw(),
                kind as i32,
                key_time_secs,
                value.as_ptr(),
                interp.raw(),
            )
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_thickness_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let ok = unsafe {
            noesis_animation_thickness_keyframes_get_value(
                self.raw(),
                index as i32,
                out.as_mut_ptr(),
            )
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t =
            unsafe { noesis_animation_thickness_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `BooleanAnimationUsingKeyFrames` — animates a `bool` property through
/// discrete key frames (a bool can't be interpolated, so only discrete frames
/// exist).
pub struct BooleanAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(BooleanAnimationUsingKeyFrames);

impl Default for BooleanAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl BooleanAnimationUsingKeyFrames {
    /// Create an empty key-frame boolean animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned BooleanAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_boolean_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_boolean_keyframes_create returned null"),
        }
    }

    /// Append a discrete key frame setting `value` at `key_time_secs`.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: bool) -> bool {
        // SAFETY: self.raw() is a live keyframe animation for the call.
        unsafe { noesis_animation_boolean_keyframes_add(self.raw(), key_time_secs, value) }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_boolean_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.raw() is live; `out` is a valid bool.
        let ok = unsafe {
            noesis_animation_boolean_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t =
            unsafe { noesis_animation_boolean_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `StringAnimationUsingKeyFrames` — animates a `String` property through
/// discrete key frames (a string can't be interpolated, so only discrete frames
/// exist).
pub struct StringAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(StringAnimationUsingKeyFrames);

impl Default for StringAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl StringAnimationUsingKeyFrames {
    /// Create an empty key-frame string animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned StringAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_string_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_string_keyframes_create returned null"),
        }
    }

    /// Append a discrete key frame setting `value` at `key_time_secs`.
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL byte.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: &str) -> bool {
        let c = CString::new(value).expect("key frame value contained interior NUL");
        // SAFETY: self.raw() is live; `c` outlives the call.
        unsafe { noesis_animation_string_keyframes_add(self.raw(), key_time_secs, c.as_ptr()) }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_string_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the key frame value at `index`, or `None` if out of range or the
    /// value is null.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<String> {
        // SAFETY: self.raw() is live; the returned pointer (if non-null) is a
        // borrowed NUL-terminated string valid for the read.
        let p = unsafe { noesis_animation_string_keyframes_get_value(self.raw(), index as i32) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is a live NUL-terminated C string for the duration of the copy.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_string_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// An owned `Noesis::BaseComponent` handle handed back from an
/// [`ObjectAnimationUsingKeyFrames`] key-frame value read.
pub struct OwnedComponent {
    ptr: NonNull<c_void>,
}

base_component_handle!(OwnedComponent);

/// An `ObjectAnimationUsingKeyFrames` — animates an `Object` (arbitrary
/// `BaseComponent`) property through discrete key frames. Objects can't be
/// interpolated, so only discrete frames exist.
pub struct ObjectAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(ObjectAnimationUsingKeyFrames);

impl Default for ObjectAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl ObjectAnimationUsingKeyFrames {
    /// Create an empty key-frame object animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ObjectAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_object_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_object_keyframes_create returned null"),
        }
    }

    /// Append a discrete key frame setting `value` at `key_time_secs`. The
    /// collection takes its own reference to `value`'s component, so the handle
    /// may be dropped afterwards. `value` is any owning handle in this module
    /// (its borrowed `BaseComponent*` is read).
    pub fn add_key_frame<C: AsComponent>(&mut self, key_time_secs: f64, value: &C) -> bool {
        // SAFETY: self.raw() is live; value.component_raw() is a live BaseComponent*.
        unsafe {
            noesis_animation_object_keyframes_add(self.raw(), key_time_secs, value.component_raw())
        }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_object_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the component at key frame `index` as an owned handle (`+1`
    /// reference), or `None` if out of range or the value is null.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<OwnedComponent> {
        // SAFETY: self.raw() is live; the C side hands out a +1 reference.
        let ptr = unsafe { noesis_animation_object_keyframes_get_value(self.raw(), index as i32) };
        NonNull::new(ptr).map(|ptr| OwnedComponent { ptr })
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_object_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `MatrixAnimationUsingKeyFrames` — animates a `Matrix` (`MatrixTransform`)
/// property through discrete key frames. A matrix is not componentwise
/// interpolated, so only discrete frames exist. Matrices are
/// `[m00, m01, m10, m11, m20, m21]`.
pub struct MatrixAnimationUsingKeyFrames {
    ptr: NonNull<c_void>,
}

animation_impls!(MatrixAnimationUsingKeyFrames);

impl Default for MatrixAnimationUsingKeyFrames {
    fn default() -> Self {
        Self::new()
    }
}

impl MatrixAnimationUsingKeyFrames {
    /// Create an empty key-frame matrix animation.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned MatrixAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_matrix_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_matrix_keyframes_create returned null"),
        }
    }

    /// Append a discrete key frame setting `value`
    /// (`[m00, m01, m10, m11, m20, m21]`) at `key_time_secs`.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: [f32; 6]) -> bool {
        // SAFETY: self.raw() is live; `value` outlives the call.
        unsafe { noesis_animation_matrix_keyframes_add(self.raw(), key_time_secs, value.as_ptr()) }
    }

    /// Number of key frames.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_matrix_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Read back the matrix at key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 6]> {
        let mut out = [0.0f32; 6];
        // SAFETY: self.raw() is live; `out` is a valid 6-float buffer.
        let ok = unsafe {
            noesis_animation_matrix_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// Read back the key time (seconds) at `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_matrix_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A `BeginStoryboard` trigger action — begins a [`Storyboard`] with a chosen
/// [`HandoffBehavior`] when the owning trigger fires. Useful inside a trigger's
/// action list; code-driven [`Storyboard::begin`] covers the rest.
pub struct BeginStoryboard {
    ptr: NonNull<c_void>,
}

base_component_handle!(BeginStoryboard);

impl Default for BeginStoryboard {
    fn default() -> Self {
        Self::new()
    }
}

impl BeginStoryboard {
    /// Create an empty `BeginStoryboard`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned BeginStoryboard*.
        let ptr = unsafe { noesis_animation_begin_storyboard_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_begin_storyboard_create returned null"),
        }
    }

    /// Set the storyboard this action begins. Noesis takes its own reference, so
    /// `storyboard` may be dropped afterwards.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_storyboard(&mut self, storyboard: &Storyboard) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_animation_begin_storyboard_set_storyboard(self.raw(), storyboard.raw()) }
    }

    /// Whether a storyboard has been assigned.
    #[must_use]
    pub fn has_storyboard(&self) -> bool {
        // SAFETY: self.raw() is live; the C side hands out a +1 reference we
        // release immediately after the null check.
        let ptr = unsafe { noesis_animation_begin_storyboard_get_storyboard(self.raw()) };
        if ptr.is_null() {
            false
        } else {
            // SAFETY: `ptr` is a +1-owned reference; release the borrow.
            unsafe { noesis_base_component_release(ptr) };
            true
        }
    }

    /// Set the hand-off behavior used when starting the storyboard's clocks.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_handoff(&mut self, handoff: HandoffBehavior) -> bool {
        // SAFETY: self.raw() is live for the call.
        unsafe { noesis_animation_begin_storyboard_set_handoff(self.raw(), handoff as i32) }
    }

    /// Read back the hand-off behavior, or `None` if the handle is invalid.
    #[must_use]
    pub fn handoff(&self) -> Option<HandoffBehavior> {
        // SAFETY: self.raw() is live for the call.
        match unsafe { noesis_animation_begin_storyboard_get_handoff(self.raw()) } {
            0 => Some(HandoffBehavior::SnapshotAndReplace),
            1 => Some(HandoffBehavior::Compose),
            _ => None,
        }
    }

    /// Set the `Name` used to control the started storyboard later.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_name(&mut self, name: &str) -> bool {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.raw() is live; `c` outlives the call.
        unsafe { noesis_animation_begin_storyboard_set_name(self.raw(), c.as_ptr()) }
    }

    /// Read back the `Name`, or `None` if unset.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.raw() is live; the returned pointer (if non-null) is a
        // borrowed NUL-terminated string valid for the read.
        let p = unsafe { noesis_animation_begin_storyboard_get_name(self.raw()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: `p` is a live NUL-terminated C string for the duration of the copy.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
        (!s.is_empty()).then_some(s)
    }
}

/// A `ParallelTimeline` — a code-built, nestable timeline group whose children
/// (any [`Timeline`], including animations or nested `ParallelTimeline`s) run in
/// parallel off the group's clock. Shares the [`Timeline`] knobs (duration,
/// repeat, auto-reverse, …) with every animation type.
pub struct ParallelTimeline {
    ptr: NonNull<c_void>,
}

base_component_handle!(ParallelTimeline);

impl Timeline for ParallelTimeline {
    fn timeline_raw(&self) -> *mut c_void {
        self.raw()
    }
}

impl Default for ParallelTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl ParallelTimeline {
    /// Create an empty `ParallelTimeline`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ParallelTimeline*.
        let ptr = unsafe { noesis_animation_parallel_timeline_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_parallel_timeline_create returned null"),
        }
    }

    /// Add a child timeline. The group's collection takes its own reference, so
    /// `child` may be dropped afterwards. Returns `false` on a type mismatch.
    pub fn add_child<T: Timeline>(&mut self, child: &T) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_animation_parallel_timeline_add_child(self.raw(), child.timeline_raw()) }
    }

    /// Number of child timelines, or `None` if the handle is not a timeline group
    /// (should not happen for a live handle).
    #[must_use]
    pub fn child_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is a live TimelineGroup*.
        let n = unsafe { noesis_animation_parallel_timeline_child_count(self.raw()) };
        u32::try_from(n).ok()
    }
}

/// Generates a fluent builder for a From/To/By animation type, covering
/// `from`/`to`/`by`, the common [`Timeline`] knobs and an [`EasingFunction`], in
/// one chain. The longhand `Type::new()` + `set_*` form keeps working. Builder
/// methods that fail to apply (e.g. a wrong-typed value) are silently ignored;
/// read the value back to verify, as the crate's tests do.
macro_rules! fromto_builder {
    ($anim:ident, $builder:ident, $val:ty, $vname:literal) => {
        impl $anim {
            #[doc = concat!("Start a [`", stringify!($builder), "`] for fluent construction.")]
            pub fn builder() -> $builder {
                $builder {
                    anim: <$anim>::new(),
                }
            }
        }

        #[doc = concat!("Fluent builder for a [`", stringify!($anim), "`] — sets the ", $vname,
                    " `from`/`to`/`by` plus the common timeline knobs (duration, begin time,\n\
             auto-reverse, repeat, fill behavior, speed) and an easing function, then\n\
             [`build`](Self::build)s the animation.")]
        #[must_use]
        pub struct $builder {
            anim: $anim,
        }

        impl $builder {
            #[doc = concat!("Set the starting ", $vname, " (`From`).")]
            pub fn from(mut self, value: $val) -> Self {
                let _ = self.anim.set_from(Some(value));
                self
            }

            #[doc = concat!("Set the ending ", $vname, " (`To`).")]
            pub fn to(mut self, value: $val) -> Self {
                let _ = self.anim.set_to(Some(value));
                self
            }

            #[doc = concat!("Set the relative ", $vname, " offset (`By`).")]
            pub fn by(mut self, value: $val) -> Self {
                let _ = self.anim.set_by(Some(value));
                self
            }

            /// Set the single-pass duration, in seconds.
            pub fn duration_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_duration_secs(seconds);
                self
            }

            /// Set the delay before the timeline begins, in seconds.
            pub fn begin_time_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_begin_time_secs(seconds);
                self
            }

            /// Play forwards then backwards each iteration when `true`.
            pub fn auto_reverse(mut self, value: bool) -> Self {
                let _ = self.anim.set_auto_reverse(value);
                self
            }

            /// Set the rate at which time progresses relative to the parent.
            pub fn speed_ratio(mut self, value: f32) -> Self {
                let _ = self.anim.set_speed_ratio(value);
                self
            }

            /// Set the behaviour once the active period ends.
            pub fn fill_behavior(mut self, behavior: FillBehavior) -> Self {
                let _ = self.anim.set_fill_behavior(behavior);
                self
            }

            /// Repeat a fixed number of (possibly fractional) iterations.
            pub fn repeat_count(mut self, count: f32) -> Self {
                let _ = self.anim.set_repeat_count(count);
                self
            }

            /// Repeat for a fixed wall-clock duration, in seconds.
            pub fn repeat_duration_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_repeat_duration_secs(seconds);
                self
            }

            /// Repeat forever.
            pub fn repeat_forever(mut self) -> Self {
                let _ = self.anim.set_repeat_forever();
                self
            }

            /// Attach an easing function.
            pub fn easing(mut self, easing: &EasingFunction) -> Self {
                let _ = self.anim.set_easing(easing);
                self
            }

            #[doc = concat!("Finish and return the built [`", stringify!($anim), "`].")]
            #[must_use]
            pub fn build(self) -> $anim {
                self.anim
            }
        }
    };
}

fromto_builder!(DoubleAnimation, DoubleAnimationBuilder, f32, "value");
fromto_builder!(ColorAnimation, ColorAnimationBuilder, [f32; 4], "color");
fromto_builder!(
    ThicknessAnimation,
    ThicknessAnimationBuilder,
    [f32; 4],
    "thickness"
);
fromto_builder!(PointAnimation, PointAnimationBuilder, (f32, f32), "point");
fromto_builder!(RectAnimation, RectAnimationBuilder, [f32; 4], "rect");
fromto_builder!(SizeAnimation, SizeAnimationBuilder, [f32; 2], "size");
fromto_builder!(Int16Animation, Int16AnimationBuilder, i16, "value");
fromto_builder!(Int32Animation, Int32AnimationBuilder, i32, "value");
fromto_builder!(Int64Animation, Int64AnimationBuilder, i64, "value");

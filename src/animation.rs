//! Animations built in code: storyboards, From/To/By and key-frame
//! animations, and easing functions.
//!
//! Every type here owns one reference to a new Noesis object and releases it on
//! drop. Adding an animation to a [`Storyboard`] or [`ParallelTimeline`], or an
//! easing function or [`KeySpline`] to an animation, makes Noesis take its own
//! reference, so you can drop the Rust handle after wiring it up.
//!
//! The main pieces:
//!
//! - From/To/By animations: [`DoubleAnimation`] (for Noesis's `float`
//!   properties), [`ColorAnimation`], [`ThicknessAnimation`],
//!   [`PointAnimation`], [`RectAnimation`], [`SizeAnimation`] and the integer
//!   animations. Each has a `builder()` for one-chain construction, e.g.
//!   [`DoubleAnimation::builder`].
//! - Key-frame animations such as [`DoubleAnimationUsingKeyFrames`]. Value
//!   types that can't be interpolated (`bool`, string, object, matrix) only
//!   take discrete frames.
//! - [`EasingFunction`] and [`KeySpline`] shape the interpolation.
//! - The [`Timeline`] and [`Animation`] traits hold the settings every
//!   animation shares: duration, repeat, target, easing.
//!
//! # Running an animation
//!
//! Animations advance on the view's clock, which
//! [`View::update`](crate::view::View::update) drives. The target element must
//! be in a [`View`](crate::view::View)'s tree. Then either:
//!
//! - set [`Animation::set_target_name`] and [`Animation::set_target_property`]
//!   on each animation, add them with [`Storyboard::add_child`], and call
//!   [`Storyboard::begin`] on the root element; or
//! - start one animation on one element's property with [`Animation::begin_on`].
//!
//! ```no_run
//! use noesis_runtime::animation::{Animation, DoubleAnimation, Storyboard};
//! use noesis_runtime::view::{FrameworkElement, View};
//!
//! noesis_runtime::init();
//! let root = FrameworkElement::parse(
//!     r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
//!              xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
//!            <Border x:Name="Panel" Background="Red"/>
//!          </Grid>"#,
//! )
//! .unwrap();
//! let mut view = View::create(root);
//! let root = view.content().unwrap();
//!
//! let mut fade = DoubleAnimation::builder()
//!     .from(1.0)
//!     .to(0.0)
//!     .duration_secs(0.5)
//!     .build();
//! fade.set_target_name("Panel");
//! fade.set_target_property("Opacity");
//!
//! let mut storyboard = Storyboard::new();
//! storyboard.add_child(&fade);
//! storyboard.begin(&root, false);
//!
//! for frame in 0..=30 {
//!     view.update(f64::from(frame) / 60.0);
//! }
//! ```

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

/// Which end of the animation an [`EasingFunction`]'s curve applies to.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EasingMode {
    /// Decelerating: `1 - f(1 - t)`.
    EaseOut = 0,
    /// Accelerating: `f(t)`.
    EaseIn = 1,
    /// `EaseIn` for the first half, `EaseOut` for the second.
    EaseInOut = 2,
}

/// The curve an [`EasingFunction`] follows.
// Ordinals match the `kind` switch in cpp/noesis_animation.cpp.
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
    /// Spring-like oscillation (see [`EasingFunction::set_oscillations`] /
    /// [`EasingFunction::set_springiness`]).
    Elastic = 8,
    /// Exponential (see [`EasingFunction::set_exponent`]).
    Exponential = 9,
    /// Configurable power (see [`EasingFunction::set_power`]).
    Power = 10,
}

/// What a timeline does with the animated value once its active period ends.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FillBehavior {
    /// Hold the final animated value after completion.
    HoldEnd = 0,
    /// Release the animated value (revert to base) after completion.
    Stop = 1,
}

/// How a newly started animation treats one already running on the same
/// property.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandoffBehavior {
    /// Snapshot the current value and replace any running animation.
    SnapshotAndReplace = 0,
    /// Compose with any running animation.
    Compose = 1,
}

/// How a key frame interpolates from the previous frame's value to its own.
// Ordinals match the `kind` switch in cpp/noesis_animation.cpp.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyFrameKind {
    /// Jumps to the value at the key time.
    Discrete = 0,
    /// Interpolates linearly up to the key time.
    Linear = 1,
    /// Interpolates along an [`EasingFunction`] passed as
    /// [`KeyFrameInterp::Easing`]. Without one it behaves like `Linear`.
    Easing = 2,
    /// Interpolates along a [`KeySpline`] passed as
    /// [`KeyFrameInterp::Spline`]. Without one it behaves like `Linear`.
    Spline = 3,
}

/// The curve passed with a key frame: an [`EasingFunction`] for
/// [`KeyFrameKind::Easing`], a [`KeySpline`] for [`KeyFrameKind::Spline`], or
/// nothing for discrete and linear frames. A curve that doesn't match the kind
/// is ignored.
#[derive(Copy, Clone)]
pub enum KeyFrameInterp<'a> {
    /// No curve (discrete and linear frames).
    None,
    /// Easing function for a [`KeyFrameKind::Easing`] frame.
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

/// Any handle in this module, viewed as a Noesis object. Lets
/// [`ObjectAnimationUsingKeyFrames::add_key_frame`] take any of them as a
/// value.
pub trait AsComponent {
    /// Borrowed `Noesis::BaseComponent*`, valid while `self` lives.
    fn component_raw(&self) -> *mut c_void;
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`, valid while `self` lives. No
            /// reference is added.
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
                // SAFETY: we own the single +1 ref the constructor received.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// Settings shared by every animation, [`Storyboard`] and
/// [`ParallelTimeline`]: duration, begin time, repeat, speed and fill.
///
/// Times are in seconds. Each setter returns `false` only if the handle is
/// not a Noesis `Timeline`, which can't happen for the types in this module.
pub trait Timeline {
    /// Borrowed `Noesis::Timeline*`, valid while `self` lives.
    fn timeline_raw(&self) -> *mut c_void;

    /// Sets the length of one pass, in seconds.
    fn set_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_seconds(self.timeline_raw(), seconds) }
    }

    /// Sets `Duration="Automatic"`: the length comes from the content, e.g.
    /// the last key frame or the longest child.
    fn set_duration_auto(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_auto(self.timeline_raw()) }
    }

    /// Sets `Duration="Forever"`.
    fn set_duration_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_duration_forever(self.timeline_raw()) }
    }

    /// The configured length of one pass in seconds, or `None` if the
    /// duration is `Automatic` or `Forever`.
    fn duration_secs(&self) -> Option<f64> {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        let s = unsafe { noesis_timeline_get_duration_seconds(self.timeline_raw()) };
        (s >= 0.0).then_some(s)
    }

    /// Sets the delay before the timeline starts, in seconds.
    fn set_begin_time_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_begin_time_seconds(self.timeline_raw(), seconds) }
    }

    /// When `true`, each pass plays forwards and then backwards.
    fn set_auto_reverse(&mut self, value: bool) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_auto_reverse(self.timeline_raw(), value) }
    }

    /// Sets how fast time runs relative to the parent. Defaults to `1.0`.
    fn set_speed_ratio(&mut self, value: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_speed_ratio(self.timeline_raw(), value) }
    }

    /// Sets whether the end value is held or released once the timeline
    /// finishes.
    fn set_fill_behavior(&mut self, behavior: FillBehavior) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_fill_behavior(self.timeline_raw(), behavior as i32) }
    }

    /// Repeats for `count` passes, which may be fractional.
    fn set_repeat_count(&mut self, count: f32) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_count(self.timeline_raw(), count) }
    }

    /// Repeats until `seconds` of timeline time have passed.
    fn set_repeat_duration_secs(&mut self, seconds: f64) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_duration(self.timeline_raw(), seconds) }
    }

    /// Repeats forever.
    fn set_repeat_forever(&mut self) -> bool {
        // SAFETY: timeline_raw() is a live Timeline* for the call.
        unsafe { noesis_timeline_set_repeat_forever(self.timeline_raw()) }
    }
}

/// An animation that drives one property, run through a [`Storyboard`] or
/// directly with [`begin_on`](Animation::begin_on).
pub trait Animation: Timeline {
    /// Borrowed `Noesis::AnimationTimeline*`, valid while `self` lives.
    fn animation_raw(&self) -> *mut c_void;

    /// Sets `Storyboard.TargetName`: the `x:Name` of the element this
    /// animation drives, looked up from the root passed to
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

    /// Sets `Storyboard.TargetProperty`: the property path this animation
    /// drives, e.g. `"Opacity"` or
    /// `"(UIElement.RenderTransform).(ScaleTransform.ScaleX)"`.
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte.
    fn set_target_property(&mut self, path: &str) -> bool {
        let c = CString::new(path).expect("target property contained interior NUL");
        // SAFETY: animation_raw() is a live DependencyObject*; c lives for the call.
        unsafe { noesis_storyboard_set_target_property(self.animation_raw(), c.as_ptr()) }
    }

    /// Attaches an easing function. Returns `false` and does nothing on
    /// key-frame animations, which take easing per key frame instead.
    fn set_easing(&mut self, easing: &EasingFunction) -> bool {
        // SAFETY: both pointers are live for the call; Noesis takes its own ref
        // to the easing function.
        unsafe { noesis_animation_set_easing_function(self.animation_raw(), easing.raw()) }
    }

    /// Starts this animation on `target`'s dependency property `dp_name`,
    /// without a storyboard (WPF's `BeginAnimation`). Target name and
    /// property settings are ignored.
    ///
    /// Returns `false` if `dp_name` is not a dependency property of `target`,
    /// or `target` is not in a [`View`](crate::view::View)'s tree.
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

/// A group of animations started together against one element tree.
///
/// Each child names its target with [`Animation::set_target_name`] and
/// [`Animation::set_target_property`]. The pause, resume, stop, seek and
/// query methods act on the run started for a given `root`, and only work if
/// it was started with `controllable = true`. Their `true` return does not
/// mean they had an effect.
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
    /// Creates an empty storyboard.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Storyboard*.
        let ptr = unsafe { noesis_storyboard_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_storyboard_create returned null"),
        }
    }

    /// Adds a child animation. The storyboard keeps its own reference, so
    /// `anim` can be dropped afterwards.
    pub fn add_child<A: Animation>(&mut self, anim: &A) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_add_child(self.raw(), anim.animation_raw()) }
    }

    /// Number of child animations. `None` only if the handle is not a
    /// storyboard, which can't happen for a live `Storyboard`.
    #[must_use]
    pub fn child_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is a live Storyboard*.
        let n = unsafe { noesis_storyboard_child_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// Starts every child animation on its target. Target names are looked up
    /// from `root`, which must be in a [`View`](crate::view::View)'s tree.
    /// Pass `controllable = true` to use [`pause`](Self::pause),
    /// [`resume`](Self::resume), [`stop`](Self::stop) and [`seek`](Self::seek)
    /// later. Uses [`HandoffBehavior::SnapshotAndReplace`].
    pub fn begin(&mut self, root: &FrameworkElement, controllable: bool) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_begin(self.raw(), root.raw(), controllable) }
    }

    /// Like [`begin`](Self::begin), with an explicit [`HandoffBehavior`] for
    /// animations already running on the same properties.
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

    /// Pauses the run started on `root`.
    pub fn pause(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_pause(self.raw(), root.raw()) }
    }

    /// Resumes the run started on `root`.
    pub fn resume(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_resume(self.raw(), root.raw()) }
    }

    /// Stops the run started on `root`, releasing its animated values.
    pub fn stop(&mut self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_stop(self.raw(), root.raw()) }
    }

    /// Seeks the run started on `root` to `seconds` from its start. Takes
    /// effect on the next [`View::update`](crate::view::View::update).
    pub fn seek(&mut self, root: &FrameworkElement, seconds: f64) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_seek(self.raw(), root.raw(), seconds) }
    }

    /// Whether the run started on `root` is playing.
    #[must_use]
    pub fn is_playing(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_is_playing(self.raw(), root.raw()) }
    }

    /// Whether the run started on `root` is paused.
    #[must_use]
    pub fn is_paused(&self, root: &FrameworkElement) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_storyboard_is_paused(self.raw(), root.raw()) }
    }
}

/// Shapes an animation's progress curve. Attach it to a From/To animation
/// with [`Animation::set_easing`], or to a key frame with
/// [`KeyFrameInterp::Easing`].
///
/// The `set_*` parameters each apply to specific [`EasingKind`]s and return
/// `false` on the others.
pub struct EasingFunction {
    ptr: NonNull<c_void>,
}

base_component_handle!(EasingFunction);

impl EasingFunction {
    /// Creates an easing function of `kind`, applied in `mode`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new(kind: EasingKind, mode: EasingMode) -> Self {
        // SAFETY: factory returns a +1-owned EasingFunctionBase*.
        let ptr = unsafe { noesis_easing_function_create(kind as i32, mode as i32) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_easing_function_create returned null"),
        }
    }

    /// Sets how far a [`EasingKind::Back`] curve pulls back.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_amplitude(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_amplitude(self.raw(), value) }
    }

    /// Sets the exponent of a [`EasingKind::Power`] curve.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_power(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_power(self.raw(), value) }
    }

    /// Sets the exponent of an [`EasingKind::Exponential`] curve.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_exponent(&mut self, value: f32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_exponent(self.raw(), value) }
    }

    /// Sets the oscillation count of an [`EasingKind::Elastic`] curve, or the
    /// bounce count of an [`EasingKind::Bounce`] curve.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_oscillations(&mut self, value: i32) -> bool {
        // SAFETY: self.raw() is a live easing function for the call.
        unsafe { noesis_easing_function_set_oscillations(self.raw(), value) }
    }

    /// Sets the springiness of an [`EasingKind::Elastic`] curve, or the
    /// bounciness of an [`EasingKind::Bounce`] curve.
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

/// Animates a `float` property from `From` to `To` (or by `By`). Noesis's
/// `Double` animations target `float` properties such as `Opacity`.
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
    /// Creates an animation with no values set. Set `From`/`To`/`By` and a
    /// duration next, or use [`DoubleAnimation::builder`].
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimation*.
        let ptr = unsafe { noesis_double_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_double_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting value (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe {
            noesis_double_animation_set_from(self.raw(), value.is_some(), value.unwrap_or(0.0))
        }
    }

    /// Sets (`Some`) or clears (`None`) the ending value (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe { noesis_double_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0.0)) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<f32>) -> bool {
        // SAFETY: self.raw() is a live DoubleAnimation* for the call.
        unsafe { noesis_double_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0.0)) }
    }
}

/// Animates a `Color` property. Colors are `[r, g, b, a]`, each `0..=1`.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimation*.
        let ptr = unsafe { noesis_color_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_color_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting color (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_from(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the ending color (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_to(self.raw(), rgba.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, rgba: Option<[f32; 4]>) -> bool {
        let v = rgba.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ColorAnimation*; `v` outlives the call.
        unsafe { noesis_color_animation_set_by(self.raw(), rgba.is_some(), v.as_ptr()) }
    }
}

/// Animates a `Thickness` property. Values are `[left, top, right, bottom]`.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ThicknessAnimation*.
        let ptr = unsafe { noesis_thickness_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_thickness_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting thickness (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the ending thickness (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live ThicknessAnimation*; `v` outlives the call.
        unsafe { noesis_thickness_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }
}

/// Animates a `Point` property. Values are `(x, y)`.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned PointAnimation*.
        let ptr = unsafe { noesis_point_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_point_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting point (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_from(self.raw(), value.is_some(), x, y) }
    }

    /// Sets (`Some`) or clears (`None`) the ending point (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_to(self.raw(), value.is_some(), x, y) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<(f32, f32)>) -> bool {
        let (x, y) = value.unwrap_or((0.0, 0.0));
        // SAFETY: self.raw() is a live PointAnimation* for the call.
        unsafe { noesis_point_animation_set_by(self.raw(), value.is_some(), x, y) }
    }
}

/// Animates a `float` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned DoubleAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_double_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_double_animation_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

/// Animates a `Color` property through a sequence of key frames. Colors are
/// `[r, g, b, a]`, each `0..=1`.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ColorAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_color_animation_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_color_animation_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `rgba` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

/// Animates a `Rect` property. Rects are `[x, y, width, height]`.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned RectAnimation*.
        let ptr = unsafe { noesis_animation_rect_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_rect_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting rect (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the ending rect (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 4]>) -> bool {
        let v = value.unwrap_or([0.0; 4]);
        // SAFETY: self.raw() is a live RectAnimation*; `v` outlives the call.
        unsafe { noesis_animation_rect_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// The `From` rect, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_from(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// The `To` rect, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_to(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// The `By` rect, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let has = unsafe { noesis_animation_rect_animation_get_by(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }
}

/// Animates a `Size` property. Sizes are `[width, height]`.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned SizeAnimation*.
        let ptr = unsafe { noesis_animation_size_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_size_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting size (`From`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_from(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_from(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the ending size (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_to(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<[f32; 2]>) -> bool {
        let v = value.unwrap_or([0.0; 2]);
        // SAFETY: self.raw() is a live SizeAnimation*; `v` outlives the call.
        unsafe { noesis_animation_size_animation_set_by(self.raw(), value.is_some(), v.as_ptr()) }
    }

    /// The `From` size, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_from(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// The `To` size, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_to(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }

    /// The `By` size, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let has = unsafe { noesis_animation_size_animation_get_by(self.raw(), out.as_mut_ptr()) };
        has.then_some(out)
    }
}

/// Animates an `int16` property.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int16Animation*.
        let ptr = unsafe { noesis_animation_int16_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int16_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting value (`From`).
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

    /// Sets (`Some`) or clears (`None`) the ending value (`To`).
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

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
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

    /// The `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_from(self.raw(), &mut out) };
        has.then_some(out as i16)
    }

    /// The `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_to(self.raw(), &mut out) };
        has.then_some(out as i16)
    }

    /// The `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int16_animation_get_by(self.raw(), &mut out) };
        has.then_some(out as i16)
    }
}

/// Animates an `int32` property.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int32Animation*.
        let ptr = unsafe { noesis_animation_int32_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int32_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting value (`From`).
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

    /// Sets (`Some`) or clears (`None`) the ending value (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<i32>) -> bool {
        // SAFETY: self.raw() is a live Int32Animation* for the call.
        unsafe {
            noesis_animation_int32_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<i32>) -> bool {
        // SAFETY: self.raw() is a live Int32Animation* for the call.
        unsafe {
            noesis_animation_int32_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// The `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_from(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// The `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_to(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// The `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let has = unsafe { noesis_animation_int32_animation_get_by(self.raw(), &mut out) };
        has.then_some(out)
    }
}

/// Animates an `int64` property.
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
    /// Creates an animation with no values set.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int64Animation*.
        let ptr = unsafe { noesis_animation_int64_animation_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int64_animation_create returned null"),
        }
    }

    /// Sets (`Some`) or clears (`None`) the starting value (`From`).
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

    /// Sets (`Some`) or clears (`None`) the ending value (`To`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_to(&mut self, value: Option<i64>) -> bool {
        // SAFETY: self.raw() is a live Int64Animation* for the call.
        unsafe {
            noesis_animation_int64_animation_set_to(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// Sets (`Some`) or clears (`None`) the offset (`By`).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_by(&mut self, value: Option<i64>) -> bool {
        // SAFETY: self.raw() is a live Int64Animation* for the call.
        unsafe {
            noesis_animation_int64_animation_set_by(self.raw(), value.is_some(), value.unwrap_or(0))
        }
    }

    /// The `From` value, or `None` if unset.
    #[must_use]
    pub fn from(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_from(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// The `To` value, or `None` if unset.
    #[must_use]
    pub fn to(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_to(self.raw(), &mut out) };
        has.then_some(out)
    }

    /// The `By` value, or `None` if unset.
    #[must_use]
    pub fn by(&self) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let has = unsafe { noesis_animation_int64_animation_get_by(self.raw(), &mut out) };
        has.then_some(out)
    }
}

/// The two cubic Bezier control points that shape a spline key frame's
/// progress curve. Coordinates are in the unit square. Pass it as
/// [`KeyFrameInterp::Spline`] with [`KeyFrameKind::Spline`].
pub struct KeySpline {
    ptr: NonNull<c_void>,
}

base_component_handle!(KeySpline);

impl KeySpline {
    /// Creates a spline from its two control points, each `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
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

    /// Sets the first control point.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_control_point1(&mut self, x: f32, y: f32) -> bool {
        // SAFETY: self.raw() is a live KeySpline* for the call.
        unsafe { noesis_animation_keyspline_set_control_point1(self.raw(), x, y) }
    }

    /// Sets the second control point.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_control_point2(&mut self, x: f32, y: f32) -> bool {
        // SAFETY: self.raw() is a live KeySpline* for the call.
        unsafe { noesis_animation_keyspline_set_control_point2(self.raw(), x, y) }
    }

    /// The first control point. Always `Some` for a live handle.
    #[must_use]
    pub fn control_point1(&self) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok =
            unsafe { noesis_animation_keyspline_get_control_point1(self.raw(), out.as_mut_ptr()) };
        ok.then_some((out[0], out[1]))
    }

    /// The second control point. Always `Some` for a live handle.
    #[must_use]
    pub fn control_point2(&self) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok =
            unsafe { noesis_animation_keyspline_get_control_point2(self.raw(), out.as_mut_ptr()) };
        ok.then_some((out[0], out[1]))
    }
}

/// Animates a `Rect` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned RectAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_rect_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_rect_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_rect_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        // SAFETY: self.raw() is live; `out` is a valid 4-float buffer.
        let ok = unsafe {
            noesis_animation_rect_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_rect_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `Size` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned SizeAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_size_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_size_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_size_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok = unsafe {
            noesis_animation_size_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_size_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates an `int16` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int16AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int16_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int16_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int16_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i16> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let ok = unsafe {
            noesis_animation_int16_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out as i16)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int16_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates an `int32` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int32AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int32_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int32_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int32_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i32> {
        let mut out = 0i32;
        // SAFETY: self.raw() is live; `out` is a valid i32.
        let ok = unsafe {
            noesis_animation_int32_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int32_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates an `int64` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned Int64AnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_int64_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_int64_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_int64_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<i64> {
        let mut out = 0i64;
        // SAFETY: self.raw() is live; `out` is a valid i64.
        let ok = unsafe {
            noesis_animation_int64_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_int64_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `Point` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned PointAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_point_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_point_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_point_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<(f32, f32)> {
        let mut out = [0.0f32; 2];
        // SAFETY: self.raw() is live; `out` is a valid 2-float buffer.
        let ok = unsafe {
            noesis_animation_point_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some((out[0], out[1]))
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_point_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `Thickness` property through a sequence of key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ThicknessAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_thickness_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_thickness_keyframes_create returned null"),
        }
    }

    /// Appends a key frame that reaches `value` at `key_time_secs`. Pass the
    /// curve for [`KeyFrameKind::Easing`] or [`KeyFrameKind::Spline`] frames
    /// in `interp`. Returns `false` if Noesis rejected the frame.
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

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_thickness_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
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

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t =
            unsafe { noesis_animation_thickness_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `bool` property through discrete key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned BooleanAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_boolean_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_boolean_keyframes_create returned null"),
        }
    }

    /// Appends a discrete key frame that sets `value` at `key_time_secs`.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: bool) -> bool {
        // SAFETY: self.raw() is a live keyframe animation for the call.
        unsafe { noesis_animation_boolean_keyframes_add(self.raw(), key_time_secs, value) }
    }

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_boolean_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.raw() is live; `out` is a valid bool.
        let ok = unsafe {
            noesis_animation_boolean_keyframes_get_value(self.raw(), index as i32, &mut out)
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t =
            unsafe { noesis_animation_boolean_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `String` property through discrete key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned StringAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_string_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_string_keyframes_create returned null"),
        }
    }

    /// Appends a discrete key frame that sets `value` at `key_time_secs`.
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL byte.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: &str) -> bool {
        let c = CString::new(value).expect("key frame value contained interior NUL");
        // SAFETY: self.raw() is live; `c` outlives the call.
        unsafe { noesis_animation_string_keyframes_add(self.raw(), key_time_secs, c.as_ptr()) }
    }

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_string_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The value of key frame `index`, or `None` if out of range.
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

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_string_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// An owned reference to a Noesis object, returned by
/// [`ObjectAnimationUsingKeyFrames::key_frame_value`]. Releases it on drop.
pub struct OwnedComponent {
    ptr: NonNull<c_void>,
}

base_component_handle!(OwnedComponent);

/// Animates an object-typed property (any Noesis object, e.g. a brush) through
/// discrete key frames.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ObjectAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_object_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_object_keyframes_create returned null"),
        }
    }

    /// Appends a discrete key frame that sets `value` at `key_time_secs`.
    /// The animation keeps its own reference, so `value` can be dropped
    /// afterwards.
    pub fn add_key_frame<C: AsComponent>(&mut self, key_time_secs: f64, value: &C) -> bool {
        // SAFETY: self.raw() is live; value.component_raw() is a live BaseComponent*.
        unsafe {
            noesis_animation_object_keyframes_add(self.raw(), key_time_secs, value.component_raw())
        }
    }

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_object_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The object at key frame `index`, or `None` if `index` is out of range
    /// or the frame's value is null.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<OwnedComponent> {
        // SAFETY: self.raw() is live; the C side hands out a +1 reference.
        let ptr = unsafe { noesis_animation_object_keyframes_get_value(self.raw(), index as i32) };
        NonNull::new(ptr).map(|ptr| OwnedComponent { ptr })
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_object_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// Animates a `Matrix` property, such as `MatrixTransform.Matrix`, through
/// discrete key frames. Matrices are `[m00, m01, m10, m11, m20, m21]`, where
/// `m20, m21` is the translation.
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
    /// Creates an animation with no key frames.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned MatrixAnimationUsingKeyFrames*.
        let ptr = unsafe { noesis_animation_matrix_keyframes_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_matrix_keyframes_create returned null"),
        }
    }

    /// Appends a discrete key frame that sets `value` at `key_time_secs`.
    pub fn add_key_frame(&mut self, key_time_secs: f64, value: [f32; 6]) -> bool {
        // SAFETY: self.raw() is live; `value` outlives the call.
        unsafe { noesis_animation_matrix_keyframes_add(self.raw(), key_time_secs, value.as_ptr()) }
    }

    /// Number of key frames. `None` only for an invalid handle.
    #[must_use]
    pub fn key_frame_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is live.
        let n = unsafe { noesis_animation_matrix_keyframes_count(self.raw()) };
        u32::try_from(n).ok()
    }

    /// The matrix at key frame `index`, or `None` if out of range.
    #[must_use]
    pub fn key_frame_value(&self, index: u32) -> Option<[f32; 6]> {
        let mut out = [0.0f32; 6];
        // SAFETY: self.raw() is live; `out` is a valid 6-float buffer.
        let ok = unsafe {
            noesis_animation_matrix_keyframes_get_value(self.raw(), index as i32, out.as_mut_ptr())
        };
        ok.then_some(out)
    }

    /// The key time of frame `index` in seconds, or `None` if `index` is out
    /// of range or the key time is not a fixed time (e.g. `Uniform` or a
    /// percentage set from XAML).
    #[must_use]
    pub fn key_frame_time(&self, index: u32) -> Option<f64> {
        // SAFETY: self.raw() is live.
        let t = unsafe { noesis_animation_matrix_keyframes_get_key_time(self.raw(), index as i32) };
        (t >= 0.0).then_some(t)
    }
}

/// A trigger action that begins a [`Storyboard`] when its trigger fires. To
/// start a storyboard from code, call [`Storyboard::begin`] instead.
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
    /// Creates an action with no storyboard.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned BeginStoryboard*.
        let ptr = unsafe { noesis_animation_begin_storyboard_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_animation_begin_storyboard_create returned null"),
        }
    }

    /// Sets the storyboard this action begins. The action keeps its own
    /// reference, so `storyboard` can be dropped afterwards.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_storyboard(&mut self, storyboard: &Storyboard) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_animation_begin_storyboard_set_storyboard(self.raw(), storyboard.raw()) }
    }

    /// Whether a storyboard is set.
    #[must_use]
    pub fn has_storyboard(&self) -> bool {
        // SAFETY: self.raw() is live; the getter hands out a +1 ref.
        let ptr = unsafe { noesis_animation_begin_storyboard_get_storyboard(self.raw()) };
        if ptr.is_null() {
            false
        } else {
            // SAFETY: drop the +1 the getter handed out.
            unsafe { noesis_base_component_release(ptr) };
            true
        }
    }

    /// Sets how the storyboard treats animations already running on the same
    /// properties.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_handoff(&mut self, handoff: HandoffBehavior) -> bool {
        // SAFETY: self.raw() is live for the call.
        unsafe { noesis_animation_begin_storyboard_set_handoff(self.raw(), handoff as i32) }
    }

    /// The configured [`HandoffBehavior`]. `None` only for an invalid
    /// handle.
    #[must_use]
    pub fn handoff(&self) -> Option<HandoffBehavior> {
        // SAFETY: self.raw() is live for the call.
        match unsafe { noesis_animation_begin_storyboard_get_handoff(self.raw()) } {
            0 => Some(HandoffBehavior::SnapshotAndReplace),
            1 => Some(HandoffBehavior::Compose),
            _ => None,
        }
    }

    /// Sets the action's `Name`, which other trigger actions use to refer to
    /// the storyboard it started.
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

    /// The action's `Name`, or `None` if unset or empty.
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

/// A group of timelines that run together on the group's clock. Children can
/// be animations or nested groups, and the group's own [`Timeline`] settings
/// (begin time, repeat, speed, ...) apply to all of them.
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
    /// Creates an empty group.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: factory returns a +1-owned ParallelTimeline*.
        let ptr = unsafe { noesis_animation_parallel_timeline_create() };
        Self {
            ptr: NonNull::new(ptr)
                .expect("noesis_animation_parallel_timeline_create returned null"),
        }
    }

    /// Adds a child timeline. The group keeps its own reference, so `child`
    /// can be dropped afterwards.
    pub fn add_child<T: Timeline>(&mut self, child: &T) -> bool {
        // SAFETY: both pointers are live for the call.
        unsafe { noesis_animation_parallel_timeline_add_child(self.raw(), child.timeline_raw()) }
    }

    /// Number of child timelines. `None` only if the handle is not a timeline
    /// group, which can't happen for a live `ParallelTimeline`.
    #[must_use]
    pub fn child_count(&self) -> Option<u32> {
        // SAFETY: self.raw() is a live TimelineGroup*.
        let n = unsafe { noesis_animation_parallel_timeline_child_count(self.raw()) };
        u32::try_from(n).ok()
    }
}

macro_rules! fromto_builder {
    ($anim:ident, $builder:ident, $val:ty, $vname:literal) => {
        impl $anim {
            #[doc = concat!("Starts a [`", stringify!($builder), "`].")]
            pub fn builder() -> $builder {
                $builder {
                    anim: <$anim>::new(),
                }
            }
        }

        #[doc = concat!("Builds a [`", stringify!($anim), "`] in one chain: the ", $vname)]
        #[doc = " `from`/`to`/`by`, the [`Timeline`] settings and an easing function."]
        #[doc = "Call [`build`](Self::build) to finish. A setting that fails to apply is ignored."]
        #[must_use]
        pub struct $builder {
            anim: $anim,
        }

        impl $builder {
            #[doc = concat!("Sets the starting ", $vname, " (`From`).")]
            pub fn from(mut self, value: $val) -> Self {
                let _ = self.anim.set_from(Some(value));
                self
            }

            #[doc = concat!("Sets the ending ", $vname, " (`To`).")]
            pub fn to(mut self, value: $val) -> Self {
                let _ = self.anim.set_to(Some(value));
                self
            }

            #[doc = concat!("Sets the ", $vname, " offset (`By`).")]
            pub fn by(mut self, value: $val) -> Self {
                let _ = self.anim.set_by(Some(value));
                self
            }

            /// Sets the length of one pass, in seconds.
            pub fn duration_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_duration_secs(seconds);
                self
            }

            /// Sets the delay before the animation starts, in seconds.
            pub fn begin_time_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_begin_time_secs(seconds);
                self
            }

            /// When `true`, each pass plays forwards and then backwards.
            pub fn auto_reverse(mut self, value: bool) -> Self {
                let _ = self.anim.set_auto_reverse(value);
                self
            }

            /// Sets how fast time runs relative to the parent.
            pub fn speed_ratio(mut self, value: f32) -> Self {
                let _ = self.anim.set_speed_ratio(value);
                self
            }

            /// Sets whether the end value is held or released when the
            /// animation finishes.
            pub fn fill_behavior(mut self, behavior: FillBehavior) -> Self {
                let _ = self.anim.set_fill_behavior(behavior);
                self
            }

            /// Repeats for `count` passes, which may be fractional.
            pub fn repeat_count(mut self, count: f32) -> Self {
                let _ = self.anim.set_repeat_count(count);
                self
            }

            /// Repeats until `seconds` of timeline time have passed.
            pub fn repeat_duration_secs(mut self, seconds: f64) -> Self {
                let _ = self.anim.set_repeat_duration_secs(seconds);
                self
            }

            /// Repeats forever.
            pub fn repeat_forever(mut self) -> Self {
                let _ = self.anim.set_repeat_forever();
                self
            }

            /// Attaches an easing function.
            pub fn easing(mut self, easing: &EasingFunction) -> Self {
                let _ = self.anim.set_easing(easing);
                self
            }

            #[doc = concat!("Returns the finished [`", stringify!($anim), "`].")]
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
